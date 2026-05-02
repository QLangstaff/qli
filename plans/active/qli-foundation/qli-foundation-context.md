# Context: qli — Polyglot Code Analysis CLI + Extension Framework
**Last Updated:** 2026-05-02

## SESSION PROGRESS

- **2026-05-02 — Fail-fast / fail-loud diagnostic pass (post-1F).** Codified the diagnostic tier policy at the top of [`qli-ext::lib`](../../../crates/qli-ext/src/lib.rs): (1) process-fatal via `anyhow`, (2) dispatch-fatal via typed `DispatchError`, (3) must-see warning via `eprintln!("warning: ...")` — never `tracing::warn!` because `-q` would silence it, (4) trace via `tracing`. Rule of thumb: if `.ok()` on a `Result` changes user-visible behaviour, you've picked the wrong tier. Validation belongs at the earliest boundary so errors point at the source, not the symptom. Fixes applied to align existing code with the policy:
    - **Manifest-time SecretSpec validation** ([`crates/qli-ext/src/manifest.rs::validate_secret_spec`](../../../crates/qli-ext/src/manifest.rs)): rejects empty `env`, `=` in `env`, NUL in `env`, empty `ref`. New variants `ManifestError::InvalidSecretEnv { env, reason: &'static str }` and `ManifestError::InvalidSecretRef { env, reason: &'static str }`. Without this, a malformed `[[secrets]]` would crash `Command::env` deep inside dispatch with no pointer to the manifest.
    - **Resolved-value NUL check** in `dispatch::apply_secret_env` returns `DispatchError::SecretValueInvalid { env, reason }` (value omitted from message) before `Command::env` can panic.
    - **Audit append flock** ([`audit::write_locked`](../../../crates/qli-ext/src/audit.rs)): macOS `PIPE_BUF = 512` is below the size of a long audit line, so concurrent dispatchers could interleave records under `O_APPEND` alone. Now takes an exclusive `nix::fcntl::Flock` on Unix; the kernel releases the lock at fd close. Required adding `fs` to `nix`'s feature list.
    - **Signal-handler install failure** ([`crates/qli/src/signal.rs::install`](../../../crates/qli/src/signal.rs)): `tracing::warn!` → `eprintln!("warning: ... Ctrl+C will not forward to running extensions")`. Behaviour-affecting; `-q` must not hide it.
    - **XDG data-dir failure** in [`crates/qli/src/main.rs`](../../../crates/qli/src/main.rs): replaced silent `map_or_else(|_| PathBuf::new(), …)` with an explicit `match` that prints a loud warning naming the env vars to set, then proceeds with an empty discovery so built-in subcommands still work.
    - Tests: `qli-ext` unit tests 35 → 40 (added 4 manifest validation + 1 dispatch NUL rejection); integration test (`secrets_never_leak`) unchanged. `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.
- **2026-05-02 — Phase 1F complete.** Dispatcher with guardrails landed end-to-end.
    - **New modules in `qli-ext`:** [`audit`](../../../crates/qli-ext/src/audit.rs) (JSONL `AuditEvent::{Start,Finish,Interrupted}` + `expand_path` via `shellexpand::full_with_context` with caller-supplied XDG defaults), [`secrets`](../../../crates/qli-ext/src/secrets.rs) (`SecretsResolver` trait, `ResolvedSecret`, `SecretsError`, in-process `TestResolver`), [`guard`](../../../crates/qli-ext/src/guard.rs) (`print_banner`, `check_requires_env`, `run_confirm` with `ConfirmPrompt` trait + `TtyConfirm` dialoguer backend).
    - **`dispatch.rs` rewritten:** [`dispatch::run`](../../../crates/qli-ext/src/dispatch.rs) executes the 8-step guard sequence (banner → requires_env → confirm → secrets → audit-start → spawn → wait → audit-finish/interrupted). `DispatchOptions { assume_yes, resolver, confirm, signals, audit_path_defaults }` gathers the binary's plug-ins. `DispatchSignals` is the shared `Arc` between dispatcher and ctrlc handler — holds an `AtomicBool` interrupt flag and a `Mutex<Option<u32>>` child-PID slot. `on_signal()` forwards SIGTERM to the child via `nix::sys::signal::kill` (workspace `unsafe_code = "forbid"` rules out `libc::kill` direct calls).
    - **Binary wiring:** [`crates/qli/src/cli.rs`](../../../crates/qli/src/cli.rs) gains a `--yes / -y` global flag. [`crates/qli/src/signal.rs::install`](../../../crates/qli/src/signal.rs) returns `Arc<DispatchSignals>` instead of `Arc<AtomicBool>`. [`crates/qli/src/main.rs`](../../../crates/qli/src/main.rs) constructs `DispatchOptions` with: a `StubResolver` that fails closed for any `[[secrets]]` (Phase 1G replaces it with the real `OnePassword`/`Env` providers), `tty_confirm()`, and `audit_path_defaults()` pre-resolving `XDG_STATE_HOME`/`XDG_DATA_HOME`/`XDG_CACHE_HOME`/`XDG_CONFIG_HOME` from `etcetera::Xdg` so a manifest written `audit_log = "$XDG_STATE_HOME/qli/prod-audit.log"` works without the user exporting the var.
    - **Tests:** 35 unit tests in `qli-ext` (audit/secrets/guard/dispatch + existing manifest/discovery), plus the integration test `crates/qli-ext/tests/secrets_never_leak.rs` that subprocess-spawns the dispatcher across happy / env_fail / confirm_decline / child_fail scenarios with distinct sentinel secrets and asserts they never appear in stdout, stderr, or the audit log. The signal-forwarding behaviour is exercised by `dispatch::tests::signal_forwarding_writes_interrupted_audit_and_exits_with_signal_code` — calls `signals.on_signal()` from a helper thread mid-`sleep 60`, asserts exit 143 + `Interrupted{signal:"SIGTERM"}` audit record.
    - **Manual smoke verified** (transient `XDG_*` overrides): missing `QLI_ENV` errors with `export QLI_ENV=prod` suggestion; non-TTY without `--yes` refuses with the documented message; `--yes` happy path writes start+finish audit lines with `env_var_names` populated and no secret values; `dev hello` (no guards) still runs; a manifest with `[[secrets]]` fails closed with the Phase 1G stub message.
    - **Color-routing decision recorded** below; the existing env-mutation shim stays.
    - **New deps in `qli-ext`:** `chrono` (clock + serde), `dialoguer` (default-features=false), `serde_json`, `shellexpand`, plus unix-only `nix` (signal feature).
    - **Known 1F simplifications (deliberate, not blockers):**
        - **SIGINT label fidelity.** The ctrlc handler doesn't know which signal fired (`ctrlc::set_handler` doesn't expose it), so `on_signal()` always forwards SIGTERM. Terminal Ctrl+C is unaffected — the kernel broadcasts SIGINT to the foreground process group, the child dies with SIGINT, exit code is 130; the dispatcher's later SIGTERM forward arrives after the child is gone. Programmatic `kill -INT <parent>` only reaches the parent, so the forwarded SIGTERM kills the child and the audit reads `signal: "SIGTERM"` with exit 143 even though the originating signal was SIGINT. If the SIGINT label matters in later phases, switch from `ctrlc` to `signal-hook` or `nix::sys::signal::sigaction` to capture the originating signal.
        - **No SIGTERM → SIGKILL escalation.** A child that traps SIGTERM (e.g. a Python script with `signal.signal(SIGTERM, …)`) will hang the dispatcher in `child.wait()`. The plan text says "wait briefly"; the 1F implementation does not implement a grace period + kill. Add it (likely in a 1G/1H polish pass, or paired with the prod fixtures in 1H) when a real-world child first surfaces the issue.
        - **PID-registration race.** Tiny window between `child.spawn()` and `signals.set_child(child.id())` where a signal would set the interrupt flag without forwarding to the child. Result: child runs to completion, audit records `interrupted` with `exit_code: 0`. Sub-millisecond window; not worth pre-spawn registration gymnastics.
    - **Open follow-ups for 1G:** real `OnePassword` (`op read`) and `Env` providers replacing `StubResolver`; the trait is frozen so 1G is purely wiring.
- **2026-05-02 — Phase 1E complete.** Discovery + dynamic clap dispatch landed.
    - `crates/qli-ext/src/discovery.rs` — `discover(extensions_root)` walks the XDG dir for `<group>/_manifest.toml` + executables (skip `_*`, require execute bit on Unix), then walks PATH for `qli-<group>-<name>` binaries. Returns `Discovery { groups, warnings }`; pure (no I/O for warnings — caller decides). Collision rule: XDG wins, warn. PATH binary referencing an unknown group: warn. Reserved group names (`analyze`, `completions`, `ext`, `help`, `index`, `lsp`, `mcp`, `self-update`) skipped with warning to avoid panicking clap when future phases register those subcommands. Malformed PATH names (`qli-`, `qli-foo` no separator, `qli-foo-` trailing-empty) warn and skip per advisor blind spot.
    - `crates/qli-ext/src/dispatch.rs::run` — bare `Command::spawn`/`.status()`, returns child's exit code (Unix signal exits → `128 + signo`). Phase 1F will wrap this spawn with the guard sequence; intentionally no guards yet.
    - `crates/qli/src/main.rs` — restructured to call `Cli::command()`, loop over `discovery.groups` adding synthesized subcommands, then `get_matches`. Globals (`verbose`/`quiet`/`color`) pulled via `get_count`/`get_flag`/`get_one` (not `Cli::from_arg_matches`, which fails on unknown subcommands). Discovery warnings print BEFORE `get_matches` so they fire on `--help`/`--version`/parse-error paths.
    - `crates/qli/src/ext.rs` — Option B clap synthesis. Each group becomes a clap subcommand (with `arg_required_else_help(true)`). Each extension is a leaf subcommand with `disable_help_flag/version_flag(true)` + `trailing_var_arg/allow_hyphen_values/num_args(0..)` `OsString` positional, so `--help` and arbitrary args reach the script verbatim. Names are leaked once at startup (clap's `Str` only converts from `&'static str`).
    - Tests: 7 new unit tests in `discovery::tests` (16 total in qli-ext). Workspace `cargo build`/`clippy --all-targets -D warnings`/`fmt --check`/`test` all green. Manual smoke verified: XDG-vs-PATH collision wins XDG, PATH-only group warns, reserved-name shadow warns, malformed PATH names warn, non-executable file warns, args + exit code propagate.
    - Open call-out: per-extension `about` shows `XDG: <path>` / `PATH: <path>` because the Phase 1D manifest has no per-extension description field. The 1E verify mentioned "from manifest if specified"; a `[extensions.<name>] description` table would be a 1D schema bump — left to Phase 1F or later if the user wants it.
    - Next: Phase 1F dispatcher with guardrails (banner → `requires_env` → confirm → secrets → audit-start → spawn → audit-finish, plus the deferred color-routing decision).
- **2026-05-01 — Phase 1D complete.** Manifest schema in `crates/qli-ext/src/manifest.rs` with `Manifest`, `SecretSpec`, `SecretProvider`, `ManifestError`, `CURRENT_SCHEMA_VERSION`. `Manifest: FromStr` parses TOML; `schema_version > 1` → `SchemaVersionTooNew`, `schema_version < 1` (e.g. `0`) → `SchemaVersionInvalid` (both typed `{ found, supported }`). Field shapes: `requires_env: HashMap<String,String>` with `#[serde(default)]` (no `Option` wrapper — empty map and "absent" are equivalent); `audit_log: Option<String>` (literal, pre-expansion — dispatcher converts to `PathBuf` in 1F). `SecretProvider` uses `#[serde(rename_all = "snake_case")]` so TOML reads `provider = "one_password"` / `provider = "env"`, consistent with `schema_version` / `requires_env` / `audit_log` key style. All structs use `#[serde(deny_unknown_fields)]`; `ref` field renamed to `reference` via `#[serde(rename = "ref")]`. Deps added to `qli-ext`: `serde 1` (derive), `toml 0.8`, `thiserror 2`. Schema documented in `extensions/README.md`. 9 unit tests pass (minimal/full parse, version-too-new, version-zero-invalid, missing field, unknown field, unknown provider, stale-PascalCase rejection, `ref` round-trip). Workspace clippy + fmt + test all green.
- **Phases 0, 1A, 1B, 1C complete** before this session per `qli-foundation-tasks.md`.

## Repository

- **State at plan time:** newly created git repo containing only `LICENSE` and `README.md` (placeholder). No Rust scaffolding yet.
- **GitHub:** `QLangstaff/qli`.

## Target Layout

```
qli/
├── Cargo.toml                          # workspace
├── rust-toolchain.toml                 # pinned stable, refreshed quarterly
├── Cargo.lock
├── .github/workflows/
│   ├── ci.yml                          # lint + test + build matrix
│   └── release.yml                     # generated by cargo-dist
├── crates/
│   ├── qli/                            # main binary — clap dispatcher
│   ├── qli-core/                       # engine traits — pure, no I/O
│   ├── qli-lang/                       # Language adapter trait + tree-sitter glue
│   ├── qli-lang-python/                # phase 2
│   ├── qli-lang-typescript/            # phase 2
│   ├── qli-lang-csharp/                # phase 2.5
│   ├── qli-lang-angular/               # phase 2.6
│   ├── qli-outputs/                    # human / jsonl / scip / lsp formatters
│   ├── qli-lsp/                        # tower-lsp server (phase 3)
│   ├── qli-scip/                       # SCIP emitter (phase 4)
│   └── qli-ext/                        # extension discovery + dispatch + guardrails
├── extensions/                         # default extensions shipped with the repo
│   ├── dev/_manifest.toml
│   ├── dev/hello                       # stub bash/python script
│   ├── prod/_manifest.toml             # requires_env=prod, confirm=true, audit=true
│   ├── prod/hello
│   ├── org/_manifest.toml
│   └── org/hello
├── claude-code-plugin/                 # phase 1.5
│   ├── skill.md
│   ├── commands/
│   └── mcp.json                        # optional MCP server config
└── plans/
    └── active/qli-foundation/
        ├── qli-foundation-plan.md
        ├── qli-foundation-context.md
        └── qli-foundation-tasks.md
```

## Runtime Paths (XDG)

- **Config:** `$XDG_CONFIG_HOME/qli/config.toml` (default `~/.config/qli/`)
- **Cache:** `$XDG_CACHE_HOME/qli/` (default `~/.cache/qli/`) — content-hashed analysis cache.
- **State:** `$XDG_STATE_HOME/qli/` (default `~/.local/state/qli/`) — audit logs, last-update timestamps.
- **Data (extensions):** `$XDG_DATA_HOME/qli/extensions/<group>/<name>` (default `~/.local/share/qli/extensions/`).
- Repo-shipped defaults under `extensions/` are installed into the data dir on first run, or merged at dispatch time.

## Dependencies (Rust crates)

### Core CLI
- `clap` (4.x, derive macros) — argument parsing.
- `clap_complete` — shell completion generation.
- `anyhow` + `thiserror` — error handling (anyhow for application errors, thiserror for library errors in `qli-core`).
- `serde` + `serde_json` + `toml` — manifest + config + jsonl output.
- `directories` — XDG path resolution cross-platform.
- `tracing` + `tracing-subscriber` — structured logging to stderr.
- `include_dir` — embed `extensions/` defaults into the binary at compile time, so installed binaries (cargo/brew/curl) carry working defaults without needing the repo on disk.
- TTY detection: use `std::io::IsTerminal` (stable since Rust 1.70 — no extra crate needed).
- Color output: `anstream` + `anstyle` (modern Rust standard, used by clap internally; handles `NO_COLOR`, `CLICOLOR`, Windows terminals correctly).
- Cross-platform signal handling: `ctrlc` crate (abstracts SIGINT on Unix and Ctrl+C on Windows; SIGTERM differs by platform).

### Extension dispatch
- `which` — locate `qli-<group>-<name>` on PATH.
- `walkdir` — discover extensions under XDG data dir.
- `dialoguer` — confirm prompts (TTY-aware).
- `chrono` — audit log timestamps.

### Analysis engine (Phase 2+)
- `tree-sitter` — parser core.
- `tree-sitter-python`, `tree-sitter-typescript`, `tree-sitter-c-sharp`, `tree-sitter-angular`, `tree-sitter-html` — grammars per language.
- `blake3` or `xxhash-rust` — content hashing for cache keys.
- `dashmap` — concurrent cache.

### LSP (Phase 3)
- `tower-lsp` — LSP server framework.
- `lsp-types` — pulled in transitively.
- `tokio` — async runtime.

### SCIP (Phase 4)
- `scip` (Sourcegraph's official Rust crate) — protobuf types and emitter.
- `prost` — pulled in transitively.

### Distribution (Phase 1.5)
- `cargo-dist` — cross-compile, GitHub release, Homebrew tap, curl installer (this is a build-time tool, not a dependency).
- `self_update` crate — runtime self-update for binary installs (gated by detected install method).

## External Tools

- **`op`** (1Password CLI) — invoked via `std::process::Command` for secrets injection. Optional; manifest declares which extensions require it.
- **`scip`** (Sourcegraph CLI) — used in Phase 4 tests to validate emitted indexes.
- **`gh`** (GitHub CLI) — used in `release.yml` and for Homebrew tap publishing.

## Design Decisions

### Resolved blocking decisions (locked before Phase 0)

- **Crate publishing strategy: publish all workspace crates to crates.io under the `qli-*` prefix.** Trade-off: more release toil (must bump and publish in topological order on every release) in exchange for keeping the modular architecture (`qli-core`, `qli-ext`, language adapters as separate crates). Use `release-plz` or a custom script for ordered publishing. Required because `cargo install qli` resolves all transitive crates from the registry, not from the workspace path deps.
- **Embedded extension defaults via `include_dir!`.** Repo's `extensions/` directory is compiled into the binary at build time. `qli ext install-defaults` copies these embedded defaults to `$XDG_DATA_HOME/qli/extensions/` for the user to edit. User-installed extensions always override embedded defaults at dispatch time. Net: a fresh `cargo install qli` / `brew install qli` / curl install has working defaults with no network or repo access.
- **Phase 4 SCIP requires a real symbol/reference analyzer.** Adding **Phase 2I — definition + reference extractor** as an explicit prerequisite for Phase 4. Extracts function/class definitions and call sites; gives Phase 3 LSP go-to-def something to do, gives Phase 4 SCIP real symbols to emit. Phase 4 acceptance now requires Phase 2I shipped.

### Architectural decisions

- **Single binary with subcommands, not multiple binaries.** `qli dev`, `qli prod`, `qli org`, `qli analyze`, `qli lsp`, `qli index` — one binary, group affordance via folders. (Earlier we considered `qli-dev`/`qli-prod` as separate binaries; rejected because `prod` safety can be enforced strictly enough by manifest + dispatcher.)
- **Built-in core, external extensions.** The `git`/`gh`/`cargo`/`kubectl` model. Extensions can be in any language. Built-ins are Rust for speed and tight integration with the engine.
- **Group manifests, not per-script directives.** Safety lives in `_manifest.toml`, applied uniformly to every script in the group. A bash script can't accidentally bypass the prod confirm.
- **Engine purity is non-negotiable.** `qli-core` never touches stdout, never uses clap, never reads files directly (it gets passed bytes). This is what makes LSP, CLI, and SCIP frontends share it cleanly.
- **Outputs are pluggable formatters.** Adding `--format yaml` later = one new file in `qli-outputs/`. Engine never knows.
- **Lazy import equivalent in Rust.** Heavy crates (tree-sitter grammars, scip) live in their own crates and are only linked into binaries that need them. The main `qli` binary depends on all of them, so binary size grows; if startup becomes an issue, gate via cargo features later.
- **`rust-toolchain.toml` pinned to current latest stable; bumped quarterly.** Currently `1.95.0`. MSRV (`rust-version` in `Cargo.toml`) tracked separately and lags the pin. Following the ruff / uv convention for modern Rust binary tools shipped via cargo-dist — both pin specific recent versions (no major project pins to literal `"stable"`). Rationale: reproducible CI / dev / release across the cross-compile matrix. cargo-dist itself recommends `rust-toolchain.toml` over its own deprecated `rust-toolchain-version` config for projects that want pinning. Alternative considered: drop the pin entirely (ripgrep / tokio / cargo pattern) — rejected because we already have a quarterly refresh task on the books and reproducibility wins are real for the multi-platform release matrix. Decision is reversible.
- **Manifest schema versioned.** `schema_version = 1` on every `_manifest.toml`. Dispatcher rejects unknown versions with a clear error.
- **`self-update` is a stub in Phase 1, real in Phase 1.5.** Solving self-update before any binaries exist is solving a non-problem; reserving the subcommand keeps UX consistent.
- **Claude Code plugin is a wrapper, not a precondition.** The CLI is a CLI first. The plugin is a thin shell over a working binary; it doesn't dictate any architectural choices.
- **Seed languages = Python + TypeScript.** Picked to exercise the polyglot trait with two genuinely different grammars before piling on. C# (Phase 2.5) and Angular (Phase 2.6) are explicit later milestones, not bolt-ons.
- **Trivial seed analyzer (TODO/FIXME extractor).** Phase 2's job is to prove the architecture across languages, not to ship real analysis. Real analyzers come after Phase 2 is solid.

### Open design decisions

_None at present._

### Color routing decision (resolved 2026-05-02)

Resolution: **keep the env-mutation shim** ([`apply_color_choice`](../../../crates/qli/src/cli.rs)). Phase 1F's first-party colored output (banner, confirm prompt) is plain text + `dialoguer::Confirm`'s defaults, both of which already honour `NO_COLOR` / `CLICOLOR_FORCE` via `anstream` / `console`. Threading a color-state struct through every future printer (ripgrep's pattern) or building cargo's pre-scan + `Shell` abstraction was out of proportion for the current call sites; the env-mutation shim keeps it simple and covers `--help` reliably.

Known limitations carried forward (revisit when one becomes a real user-visible problem, not earlier):
- Edition 2024 will make `std::env::set_var` `unsafe`; the workspace is on edition 2021 today.
- clap parse errors render before `apply_color_choice` runs; they aren't gated by `--color`. A pre-scan layer would fix that.
- No `ansi` fourth value (ripgrep). Reconsider if/when Windows support lands.

Original alternatives considered: env mutation (chosen), cargo-style pre-scan + `Shell`, ripgrep-style threaded color-state, clap-direct `Command::color`. Trade-offs unchanged from prior table.

## Constraints

- **macOS-first development** (user's primary OS), but all CI matrices and release binaries cover macOS arm64/x86_64, Linux x86_64/arm64, Windows x86_64.
- **No paid services.** Distribution stays on free tiers (GitHub releases, crates.io, Homebrew tap).
- **No telemetry.** The tool is single-operator; user may add later if multi-user lands.
- **Solo author.** Plan must be self-contained and resumable across sessions; tasks must be small enough to pick up cold.
- **Unix-style discipline.** stdout=data / stderr=chatter, exit codes 0/1/2/130, GNU long flags, kebab-case command names.
- **No backwards-compat carrying yet.** Pre-1.0; manifest schema, output JSON, and CLI flags can change with minor version bumps. After 1.0 these become API surface.
- **Rust 2021 edition.** Toolchain pin (1.95.0) and MSRV (1.85) both support edition 2024, but the workspace stays on 2021 for now. Edition 2024 makes `std::env::set_var` / `remove_var` `unsafe` (race-y in multi-threaded programs); migrating means auditing `apply_color_choice` and any future env mutation. Defer until there's a reason.
- **License:** repo already has a `LICENSE` file — preserve it; per-crate `Cargo.toml` declares matching `license = "..."`.
