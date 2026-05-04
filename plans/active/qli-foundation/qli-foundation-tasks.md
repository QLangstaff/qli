# Task Checklist: qli — Polyglot Code Analysis CLI + Extension Framework
**Last Updated:** 2026-05-03 (Phase 1L complete pending CI; Push B tests added)

Each phase ships a working artifact. Don't start phase N+1 until phase N's "verify" tasks pass.

## Resolved structural decisions

Three structural forks (crate publishing, embedded extension defaults, SCIP prerequisite) are locked — see [`qli-foundation-context.md` → Resolved blocking decisions](qli-foundation-context.md#resolved-blocking-decisions-locked-before-phase-0). The phases below assume them.

## Phase 0: Repo bootstrap

- [x] **Verify crate name availability on crates.io** for `qli`, `qli-core`, `qli-ext`, `qli-lang`, `qli-lang-python`, `qli-lang-typescript`, `qli-lang-csharp`, `qli-lang-angular`, `qli-outputs`, `qli-lsp`, `qli-scip`. Lock the chosen name before any code.
- [x] Reserve crate names by publishing empty `0.0.0` placeholders. All 11 `qli-*` support crates (`qli-core`, `qli-ext`, `qli-lang`, `qli-lang-python`, `qli-lang-typescript`, `qli-lang-csharp`, `qli-lang-angular`, `qli-outputs`, `qli-lsp`, `qli-scip`, `qli-analyzers`) published as `0.0.0` via `scripts/reserve-placeholder-crates.sh` on 2026-04-30. The main `qli` name is in the parallel reclaim flow.
- [x] Initialize Cargo workspace at repo root (`Cargo.toml` with `[workspace]`, `members = ["crates/*"]`, shared `[workspace.package]` for version/license/edition/repository/rust-version). `crates/.gitkeep` keeps the empty members directory tracked.
- [x] Add `rust-toolchain.toml` pinning `channel = "1.83.0"`, `components = ["rustfmt", "clippy"]`.
- [x] Add `.gitignore` for Rust (`/target`, `**/target`, IDE dirs, `.DS_Store`, env files).
- [x] Add `.editorconfig` (4-space tabs for Rust, LF line endings, trim trailing whitespace; 2-space for TOML/YAML/JSON; tab for Makefile).
- [x] Add `rustfmt.toml` with project conventions (`edition = "2021"`, `max_width = 100`, `newline_style = "Unix"`, `use_field_init_shorthand = true`). Stable rustfmt options only — `imports_granularity` is nightly-gated and was deliberately omitted.
- [x] Lints configured via `[workspace.lints]` in `Cargo.toml` (stable since Rust 1.74) — `clippy::all` warn, `clippy::pedantic` warn, with selective allows for noisy lints (`module_name_repetitions`, `must_use_candidate`, `missing_errors_doc`, `missing_panics_doc`); `unsafe_code = "forbid"` and `missing_debug_implementations = "warn"` on the rust group. Workspace lints opt in per crate via `lints.workspace = true`. CI denies warnings via `cargo clippy -- -D warnings`.
- [x] Confirmed existing `LICENSE` is MIT; recorded as `license = "MIT"` in `[workspace.package]`.
- [x] Replace placeholder `README.md` with a stub that describes the project and links to `plans/active/qli-foundation/`. Status line updated to note the `qli-*` prefix is reserved.
- [x] Verify: workspace manifest parses on a fresh clone. `cargo metadata --no-deps` succeeds (exit 0). `cargo check` is not the right verify here — an empty workspace has nothing to check; that gate fires in Phase 1A once member crates exist. `rust-toolchain.toml` was honored (triggered auto-install of 1.83.0 via rustup).

## Phase 1: Skeleton + Extension Dispatch

### 1A: Workspace crates (stubs)

- [x] Created stub crates: `qli` (binary, `crates/qli/src/main.rs`), `qli-core`, `qli-lang`, `qli-outputs`, `qli-ext` (libraries, each with `src/lib.rs` doc-only stub). Each Cargo.toml inherits version/edition/license/repository/rust-version from `[workspace.package]` and opts into workspace lints via `[lints] workspace = true`.
- [x] Wired dependency: `qli` → `qli-ext` (path dep with version `0.0.0`). `qli-ext` does **not** depend on `qli-outputs` (decoupled per plan: Phase 1 dispatcher will print banners/errors directly via `anstream`; `qli-outputs` is for analysis output formatters in Phase 2+). Other deps (`qli-core`, `qli-lsp`, `qli-scip`, language adapters) wired in their respective phases.
- [x] Verified: `cargo build` succeeds (all 5 crates compile clean); `cargo run -p qli` prints the stub message; `cargo clippy --workspace -- -D warnings` clean.

### 1B: Core CLI scaffolding (in `qli` crate)

- [x] Added `clap` (derive) with workspace root command `qli`. CLI struct in `crates/qli/src/cli.rs` with global `--verbose` (count), `--quiet`, `--color={auto,always,never}` flags.
- [x] `--version` auto-wired via clap (prints `qli 0.0.0` from `CARGO_PKG_VERSION`).
- [x] `qli completions <shell>` implemented using `clap_complete::generate` (bash, zsh, fish, powershell, elvish — all clap_complete defaults).
- [x] `tracing-subscriber` to stderr; default `warn`, `-v`/`-vv`/`-vvv` → info/debug/trace, `-q` → error. `RUST_LOG` precedence (bare level overrides; target directive refines) documented inline in `crates/qli/src/logging.rs` and the `--verbose` help text.
- [x] Exit code constants in `crates/qli/src/exit.rs`: `SUCCESS=0`, `ERROR=1`, `USAGE=2`, `SIGINT=130`, `SIGTERM=143`. SIGINT/SIGTERM are reserved for use in Phase 1F dispatcher (no long-running ops in 1B).
- [x] `ctrlc` crate (with `termination` feature for SIGTERM on Unix) installed via `crates/qli/src/signal.rs::install()`. Returns an `Arc<AtomicBool>` flipped to `true` on signal; long-running ops in later phases will poll it.
- [x] TTY detection via `std::io::IsTerminal` from stdlib (no extra crate). `clap` and `anstream` consult standard env vars internally; `apply_color_choice` translates `--color=always|never` to `CLICOLOR_FORCE` / `NO_COLOR` env vars before any output.
- [x] Color output: `anstream` + `anstyle` deps added; `--color` flag wired; `NO_COLOR=1` auto-respected by clap (verified — `qli --help` produces no ANSI escapes under `NO_COLOR=1`).
- [x] `--help` examples on the root command and on `completions` subcommand via clap's `after_help` (`ROOT_AFTER_HELP`, `COMPLETIONS_AFTER_HELP` constants in `cli.rs`). Future subcommands (analyze, lsp, index, ext) will follow the same pattern.
- [x] MSRV bumped 1.83 → 1.85 (edition 2024 dep requirement); toolchain pin set to 1.95.0. Rationale in [context.md → toolchain pin entry](qli-foundation-context.md#architectural-decisions).
- [x] Verified: `qli --version` prints `qli 0.0.0`; `qli --help` shows examples; `qli completions zsh` produces a valid zsh completion script; `NO_COLOR=1 qli --help` has zero ANSI escapes; `cargo build` and `cargo clippy --workspace -- -D warnings` both clean. SIGINT/SIGTERM verifies deferred to Phase 1F (no long-running ops in 1B to interrupt).

### 1C: XDG path resolution

- [x] Used `etcetera` crate (not `directories`) with `etcetera::base_strategy::Xdg`. Reason: `directories` follows OS-native conventions (macOS = `~/Library/Application Support/qli`); the plan calls for strict XDG even on macOS, and `etcetera::Xdg` provides exactly that behavior cross-platform.
- [x] Path resolution lives in the `qli` binary (`crates/qli/src/paths.rs`), not in any library crate. Library crates (qli-core, qli-ext, ...) take paths as parameters — keeps them stateless and pure.
- [x] Public helpers: `config_dir()`, `cache_dir()`, `state_dir()`, `data_dir()` — each returns `Result<PathBuf>` for `<XDG base>/qli`. `ensure_all()` is best-effort: logs warnings on failure but does not error (individual ops handle their own errors with full context).
- [x] Wired `paths::ensure_all()` into `main()` after logging init (so warnings are visible) and before subcommand dispatch.
- [x] Verified on macOS: with `XDG_*` env vars unset, all four dirs are created at the documented defaults (`~/.config/qli`, `~/.cache/qli`, `~/.local/state/qli`, `~/.local/share/qli`). Trace events under `-vvv` confirm each dir was ensured. Linux verify deferred (same `etcetera::Xdg` code path; high confidence).

### 1D: Extension manifest schema

- [x] `_manifest.toml` schema in [`crates/qli-ext/src/manifest.rs`](../../../crates/qli-ext/src/manifest.rs): `Manifest` + `SecretSpec` + `SecretProvider` (`OnePassword` / `Env`). `#[serde(deny_unknown_fields)]` everywhere; `CURRENT_SCHEMA_VERSION = 1`.
- [x] Schema-version mismatches → typed `ManifestError::{SchemaVersionTooNew, SchemaVersionInvalid}` with `{found, supported}` payload.
- [x] Parse-time `SecretSpec` validation (`validate_secret_spec`) rejects empty `env`/`ref`, `=` in `env`, NUL in `env` — fail at the manifest boundary, not at `Command::env`.
- [x] Schema documented in [`extensions/README.md`](../../../extensions/README.md).
- [x] Verify: 13 unit tests in `manifest::tests`. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1E: Extension discovery

- [x] XDG discovery in [`crates/qli-ext/src/discovery.rs`](../../../crates/qli-ext/src/discovery.rs): `scan_xdg_root` walks `$XDG_DATA_HOME/qli/extensions/<group>/_manifest.toml` + executable files (skip `_*`); flat structure. `discover` takes `extensions_root: &Path` so 1H can layer in embedded defaults.
- [x] PATH discovery: `qli-<group>-<name>` binaries via `scan_path_for_qli_binaries`. Malformed names (`qli-`, `qli-foo` no separator, `qli-foo-`) warn and skip. PATH binaries referencing unknown groups warn and skip.
- [x] **Collision rule**: XDG wins; warning names both paths and points at `qli ext which`.
- [x] **Reserved-name guard** (`RESERVED_GROUP_NAMES`): `analyze`, `completions`, `ext`, `help`, `index`, `lsp`, `mcp`, `self-update` skipped with warning to avoid clap duplicate-subcommand panic.
- [x] **Clap dynamic dispatch (Option B)** in [`crates/qli/src/ext.rs`](../../../crates/qli/src/ext.rs): `Cli::command()` for the static derive tree, then loop over `discovery.groups` adding `ext::build_group_command(group)`. Names leaked once at startup (`clap::Str` only converts from `&'static str`). Globals pulled from `ArgMatches` directly, not via `Cli::from_arg_matches`.
- [x] Argument forwarding: `disable_help_flag(true) + disable_version_flag(true) + trailing_var_arg(true) + allow_hyphen_values(true) + num_args(0..) + value_parser!(OsString)` so `--help`/`--version` and arbitrary args reach the script verbatim.
- [x] Basic dispatch in [`crates/qli-ext/src/dispatch.rs::run`](../../../crates/qli-ext/src/dispatch.rs): `Command::spawn`/`status` (not `exec`) so 1F can wrap with guards. Signal exits → `128 + signo`.
- [x] Discovery warnings print BEFORE `get_matches` so they fire on `--help` / `--version` / parse-error paths.
- [x] Verify: 7 unit tests in `discovery::tests` + manual smoke. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1F: Dispatcher with guardrails

- [x] 8-step guard sequence in [`crates/qli-ext/src/dispatch.rs::run`](../../../crates/qli-ext/src/dispatch.rs): banner → `requires_env` → confirm → secrets → audit-start → spawn → wait → audit-finish/interrupted. Each step gates the next; failure short-circuits. Helpers in [`guard.rs`](../../../crates/qli-ext/src/guard.rs), [`secrets.rs`](../../../crates/qli-ext/src/secrets.rs), [`audit.rs`](../../../crates/qli-ext/src/audit.rs).
- [x] `--yes` global flag short-circuits confirm; non-TTY with `confirm = true` and no `--yes` returns `NonInteractiveRefuse`.
- [x] Audit JSONL `Start`/`Finish`/`Interrupted` events with `env_var_names` (names only, never values).
- [x] Child PID registered in `DispatchSignals` shared with the binary's ctrlc handler; forwards SIGTERM via `nix::sys::signal::kill` (workspace `unsafe_code = "forbid"` rules out `libc::kill`).
- [x] Exit code propagates `0..=255` → `ExitCode`; signal exits → `128 + signo` on Unix.
- [x] 1F StubResolver in the binary returns `ProviderUnavailable` for any `[[secrets]]` until 1G; trait surface is frozen.
- [x] Color-routing decision: keep the env-mutation shim ([`apply_color_choice`](../../../crates/qli/src/cli.rs)). Recorded in [context.md](qli-foundation-context.md#color-routing-decision-resolved-2026-05-02).
- [x] **Fail-fast/fail-loud audit (post-1F polish).** Diagnostic policy doc'd at top of [`qli-ext::lib`](../../../crates/qli-ext/src/lib.rs) (four tiers: anyhow / typed `DispatchError` / `eprintln!` warning / `tracing`). Fixes:
    - NUL check on resolved secret values before `Command::env` → typed `DispatchError::SecretValueInvalid` (value omitted from message).
    - `audit::append` takes an exclusive `nix::fcntl::Flock` on Unix (macOS `PIPE_BUF = 512` could interleave under `O_APPEND` alone).
    - Signal-handler install failure: `tracing::warn!` → `eprintln!` (must-see).
    - XDG data-dir resolution failure: silent swallow → loud warning, fall back to empty discovery.
- [x] Regression test [`tests/secrets_never_leak.rs`](../../../crates/qli-ext/tests/secrets_never_leak.rs) — distinct sentinels across happy/env_fail/confirm_decline/child_fail; asserts none appear in stdout/stderr/audit.
- [x] Verify: unit + integration tests + manual smoke. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1G: Secrets providers

- [x] `OnePassword` provider in [`crates/qli-ext/src/secrets.rs::resolve_one_password`](../../../crates/qli-ext/src/secrets.rs): `op read <ref>` via `Command`. Output mapping split into `parse_op_output(spec, io::Result<Output>)` so it's unit-testable without a fake `op` binary. NotFound spawn → `ProviderUnavailable` with install + signin hint; non-zero exit → `Resolution` carrying `op`'s stderr; non-UTF-8 stdout → `Resolution` (drops bytes). Strips exactly one trailing `\n` (and preceding `\r` if present); preserves internal newlines.
- [x] `Env` provider in [`resolve_env`](../../../crates/qli-ext/src/secrets.rs): `std::env::var(spec.reference)` → bind under `spec.env`. `VarError::{NotPresent, NotUnicode}` → `Resolution { provider: "env", … }` echoing the reference name only.
- [x] `ProductionResolver::new()` dispatches per `SecretProvider`; replaces the 1F `StubResolver` in [`crates/qli/src/main.rs`](../../../crates/qli/src/main.rs). Trait surface untouched.
- [x] Diagnostics policy carries through: secrets never enter `tracing` output; failure variants only ever surface env-var names + `op`'s own stderr.
- [x] Verify: 11 new unit tests in `secrets::tests` (3 `Env` + 8 `OnePassword`, 7 unix-gated) + manual smoke. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1H: Default extension stubs (embedded via `include_dir!`)

- [x] Default fixtures: [`extensions/dev/`](../../../extensions/dev/) (no guards), [`extensions/prod/`](../../../extensions/prod/) (full guard set: banner, confirm, `QLI_ENV=prod`, `$XDG_STATE_HOME/qli/prod-audit.log`), [`extensions/org/`](../../../extensions/org/). All `hello` scripts bash + 0o755.
- [x] [`crates/qli-ext/src/defaults.rs`](../../../crates/qli-ext/src/defaults.rs): `DEFAULTS: Dir = include_dir!(...)` + `materialize_to(target, force) -> MaterializeStats`. Walks group subdirs only (skips top-level `extensions/README.md`); chmods scripts to 0o755 on Unix (`include_dir` doesn't preserve mode bits). **Crate-publish caveat**: 1.5C must address `cargo publish` stripping files outside the crate dir.
- [x] `qli ext install-defaults [--force]` writes embedded defaults to `$XDG_DATA_HOME/qli/extensions/`. Idempotent without `--force`; per-file skip granularity. `ext` reserved-name skip prevents user shadowing.
- [x] Dispatch-time merge: `discovery::discover` takes `&[(&Path, ExtensionOrigin)]` and walks sources in priority order — first source to claim a group keeps it **wholesale** (manifest + extensions list). Binary materializes `DEFAULTS` to `$XDG_CACHE_HOME/qli/embedded/<VERSION>/` at startup, then calls `discover([(xdg, Xdg), (cache, Embedded)])`. Materialize failure → warning, embedded layer disabled, XDG still works. Added `ExtensionOrigin::Embedded`.
- [x] Verify: 6 new tests in `defaults::tests` + 3 layered-discovery tests + manual smoke against release binary in ephemeral XDG. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1I: Meta commands

- [x] `qli ext list [--json]` ([`dispatch_ext_list`](../../../crates/qli/src/main.rs)): tab-separated `<group>\t<extension>\t<origin>\t<path>` on stdout; `--json` is a flat array. Origin labels (`xdg`/`embedded`/`path`) sourced from `ExtensionOrigin::as_str`.
- [x] `qli ext which <group> <name> [--json]` ([`dispatch_ext_which`](../../../crates/qli/src/main.rs)): just the path (Unix `which` semantics) or the JSON object. Unknown extension exits 1 with stderr error.
- [x] `qli ext install-defaults [--force]` — landed in 1H.
- [x] `qli self-update [--json]` ([`dispatch_self_update`](../../../crates/qli/src/main.rs)): stub; prints to **stderr**, exits **2** (USAGE). `--json` emits `{status, available_in, install_methods}`. 1.5E replaces with real impl.
- [x] Verify: smoke against release binary in ephemeral XDG. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1J: Error messages with suggestions

- [x] **Panic hook** in [`crates/qli/src/panic.rs`](../../../crates/qli/src/panic.rs), installed at top of `main()`. Replaces Rust's default panic UI with a terse bug-report message + panic location. `RUST_BACKTRACE=1` path: captures prior hook via `take_hook()` and delegates (move-capture) to keep stack traces. Decided not to add a wrapping `UserError` enum — every failure already routes through `main()`'s typed-error renderer.
- [x] **Closest-match suggestions** produced by clap 4 out of the box (`qli porod hello` → `tip: ... 'prod'`). Decided not to roll a parallel Levenshtein. No-close-match falls back to clap's `For more information, try '--help'`.
- [x] **Missing env var** already emits `GuardError::EnvMissing` from 1F: `... set it with: export QLI_ENV=prod`. No code change needed.
- [x] Verify: 3 unit tests in `panic::tests` (str / String / unknown payload) + manual smoke. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### 1K: CI

- [x] [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml) with 5 jobs: `fmt` (Linux), `clippy` (matrix), `test` (matrix), `build` (matrix), `audit` (Linux). Matrix = `ubuntu-latest` + `macos-14`. Triggers: `push` on `main`, all PRs, `merge_group`. Workflow-level `permissions: contents: read`.
- [x] `cargo audit` via `rustsec/audit-check@v2` — fails on **any** advisory (no severity-tier filter; the plan chose `cargo audit` over `cargo-deny`).
- [x] Toolchain + cache via `actions-rust-lang/setup-rust-toolchain@v1` (honours `rust-toolchain.toml`; bundles `Swatinem/rust-cache@v2`).
- [ ] **Block PR merge on CI green.** Deferred — branch protection on private repos requires GitHub Pro; "no paid services" constraint rules it out. Re-enable when repo goes public (likely Phase 1.5). Required checks: `rustfmt`, `clippy ×2`, `test ×2`, `build ×2`, `cargo-audit`.
- [ ] **Verify the gates fail when they should** — clean-tree green half is empirical (run [25282714217](https://github.com/QLangstaff/qli/actions/runs/25282714217)); deliberate red-build experiment deferred to user's first real PR.

### 1L: Tests

- [x] **Fixture root** at [`tests/fixtures/README.md`](../../../tests/fixtures/README.md). Per-language subdirs land with the phase that needs them (2H/2I/2.5/2.6).
- [x] **Hermetic test harness** ([`crates/qli-ext/tests/common/mod.rs::XdgSandbox`](../../../crates/qli-ext/tests/common/mod.rs)): RAII guard pointing `HOME` + four `XDG_*` vars at a `TempDir`, restoring on `Drop`. Smoke test [`xdg_sandbox_smoke.rs`](../../../crates/qli-ext/tests/xdg_sandbox_smoke.rs). Mirrored into [`crates/qli/tests/common/mod.rs`](../../../crates/qli/tests/common/mod.rs) for assert_cmd tests, with a `stage_extension` helper.
- [x] `#[serial_test::serial]` gates added to all 10 env-mutating unit tests across `audit`/`dispatch`/`guard`/`secrets`. Unique-name discipline kept as first defence; `#[serial]` is second.
- [x] **Engine-purity test** at [`crates/qli-core/tests/dependency_purity.rs`](../../../crates/qli-core/tests/dependency_purity.rs): parses `cargo metadata --no-deps` and asserts every `DependencyKind::Normal` dep is in a hardcoded `ALLOWED_DIRECT_DEPENDENCIES` (currently empty). `[dev-dependencies]` ungated.
- [x] **CLI contract snapshots** at [`crates/qli/tests/cli.rs`](../../../crates/qli/tests/cli.rs) — `trycmd` over 6 cases in [`crates/qli/tests/cmd/`](../../../crates/qli/tests/cmd/): version, completions-zsh (header only, body elided with `...`), completions-help, unknown-no-tip, unknown-with-tip, missing-env. Root `qli --help` deliberately not snapshotted (dynamic group synthesis bakes machine-specific paths into subcommand `about`). Hermeticity via per-invocation `TempDir` + `TestCases::env(...)` + `EnvUnset` RAII guard for `QLI_ENV`.
- [x] **Dispatcher integration tests** at [`crates/qli/tests/dispatcher.rs`](../../../crates/qli/tests/dispatcher.rs) — 6 `assert_cmd` tests under `XdgSandbox`: happy path + one failure per guard (`requires_env`, `confirm`, `secrets`, `audit_log`) + SIGINT integration. SIGINT test spawns qli into its own process group via `process_group(0)` (safe stdlib, stable 1.64+) and `killpg(SIGINT)`; asserts exit 130 + `event:interrupted` audit record.
- [x] Tooling: added `assert_cmd`, `predicates`, `serial_test`, `tempfile`, `trycmd` as dev-deps to `qli`; `serial_test` to `qli-ext`; `cargo_metadata` to `qli-core`. Unix-only `nix` (signal feature) on `qli` for the SIGINT test.
- [x] Verify: `cargo test --workspace` 73 tests green; clippy + fmt clean; `--test-threads=4` green; `XDG_*=/nonexistent` green. See SESSION PROGRESS in [context.md](qli-foundation-context.md).

### Phase 1 acceptance

- [x] `qli --help`, `qli dev hello`, `qli prod hello` (with env + confirm), `qli org hello` all work end-to-end on a clean machine.
- [x] Drop a new bash script in `~/.local/share/qli/extensions/dev/`, see it appear in `qli --help` immediately, run it.
- [x] `qli prod fake-cmd` without `QLI_ENV` errors clearly with a suggestion.
- [x] CI green.

Acceptance gate verified 2026-05-03 against a clean release binary in ephemeral XDG dirs. Verify details + the bullet-3 reasoning live in [context.md](qli-foundation-context.md) SESSION PROGRESS.

## Phase 1.5: Distribution & Claude Code Plugin

### 1.5A: cargo-dist

- [ ] Run `cargo dist init`, accept config, commit generated `.github/workflows/release.yml` and `dist-workspace.toml` (or whatever `cargo-dist` calls it now).
- [ ] Configure targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-pc-windows-msvc`.
- [ ] Configure installers: shell installer (`curl | sh`), Homebrew formula, msi for Windows (optional).
- [ ] **Verify C toolchain on every target**: tree-sitter grammars are C code that compiles per-target. cargo-dist's default GitHub-hosted runners include C toolchains, but Windows MSVC and Linux musl can be flaky. On the first test release, confirm every target binary actually builds (not just queues).
- [ ] Tag a `v0.1.0` test release; verify all target binaries build and a release is created.
- [ ] Verify: `curl -LsSf https://github.com/QLangstaff/qli/releases/latest/download/qli-installer.sh | sh` installs `qli` on macOS.

### 1.5B: Homebrew tap

- [ ] Create separate repo `QLangstaff/homebrew-qli`.
- [ ] Configure `cargo-dist` to publish to it on release.
- [ ] Verify: `brew install QLangstaff/qli/qli` on a clean machine installs the latest version.

### 1.5C: crates.io (publish all workspace crates)

- [ ] Set up crates.io account, add API token to GitHub repo secrets.
- [ ] Adopt `release-plz` (or write a custom script) that publishes workspace crates in topological order on tag: leaf crates first (`qli-core`, `qli-outputs`), then `qli-lang`, then language adapters, then `qli-ext`, finally `qli` (the binary).
- [ ] Configure each crate's `Cargo.toml` with `description`, `repository`, `license`, `keywords`, `categories` — required for crates.io.
- [ ] All workspace crates share `version` from `[workspace.package]`; bumps are atomic.
- [ ] Document the release procedure in `RELEASING.md`: tag → CI publishes all crates in order → `cargo install qli` works from registry.
- [ ] Test publish to crates.io with `0.1.0` for every crate; verify topological order succeeds.
- [ ] Verify: `cargo install qli` on a clean machine (no repo, no path deps) installs the binary; running `qli --version` matches the published version.

### 1.5D: Claude Code plugin

#### 1.5D.1: Plugin scaffolding (skill + slash commands)

- [ ] Create `claude-code-plugin/` directory with `skill.md` documenting when Claude should invoke `qli` and how to interpret its output.
- [ ] Create `commands/qli-analyze.md`, `commands/qli-index.md`, etc. — slash command wrappers that shell out to the installed `qli` binary.
- [ ] Verify each slash command works in Claude Code with the plugin installed locally.

#### 1.5D.2: MCP server skeleton (own subcommand, own protocol)

- [ ] Add `qli mcp` subcommand. MCP is JSON-RPC 2.0 over stdio (separate protocol from LSP); the `qli` binary speaks both via different subcommands.
- [ ] Use the official `rmcp` crate (or the closest current-best Rust MCP SDK) — do **not** roll your own JSON-RPC.
- [ ] Implement MCP server lifecycle: `initialize`, `initialized`, `shutdown`, `exit`. Long-running stdio process; logging goes to stderr or a file (never stdout — that's the MCP transport).

#### 1.5D.3: MCP tool schemas

- [ ] Declare `qli_analyze` MCP tool with input schema `{ paths: string[], lang?: string, analyzer?: string }` and output schema matching `qli analyze --format jsonl` records.
- [ ] Declare `qli_index` MCP tool with input schema `{ path: string, output?: string, lang?: string[] }` and output schema describing the resulting SCIP file (path, byte count, symbol/reference counts).
- [ ] Declare `qli_ext_list` MCP tool exposing the discovered extensions (Claude can introspect what's available).
- [ ] Tool implementations call the same `qli-core` engine the CLI uses — no shelling out to `qli` from inside `qli mcp`.

#### 1.5D.4: MCP integration test

- [ ] Add an integration test that spawns `qli mcp`, sends `initialize`, `tools/list`, then `tools/call` for `qli_analyze` over a fixture, asserts the response contains expected diagnostics.
- [ ] Use the MCP SDK's test client if available; otherwise hand-craft JSON-RPC frames.

#### 1.5D.5: `mcp.json` and install docs

- [ ] Create `claude-code-plugin/mcp.json` declaring the MCP server (`command: "qli", args: ["mcp"]`).
- [ ] Document plugin install path against the current Claude Code plugin spec (verify exact location at implementation time — likely `~/.claude/plugins/qli/`).
- [ ] Verify end-to-end: in Claude Code with the plugin installed, `/qli-analyze foo.py` works (slash command path); Claude can also call `qli_analyze` as an MCP tool with structured inputs/outputs (MCP path).

### 1.5E: Self-update implementation

- [ ] Detect install method: check binary's canonical path against `cargo install` (under `~/.cargo/bin`), Homebrew (under `/usr/local/Cellar` or `/opt/homebrew`), curl-installed (under `~/.local/bin` or wherever `cargo-dist` puts it).
- [ ] For curl-installed: use the `self_update` crate (or `cargo-dist`'s installer re-run path) to fetch latest GitHub release.
- [ ] For Homebrew: print `brew upgrade qli`, no fight.
- [ ] For cargo: print `cargo install qli --force`.
- [ ] For Claude Code plugin: print update-via-plugin-manager instructions.
- [ ] Verify: `qli self-update` produces the right behavior under all four install methods.

### Phase 1.5 acceptance

- [ ] All four install paths (cargo, brew, curl, Claude Code plugin) work on a clean machine.
- [ ] Tagged release produces all artifacts automatically.
- [ ] `qli self-update` works for the curl-installed path and prints correct guidance for the others.

## Phase 2: Polyglot Analysis Core

### 2A: `qli-core` engine traits

- [ ] Define core types as `serde`-serializable structs: `Position { line: u32, column: u32 }`, `Range { start: Position, end: Position }`, `Severity { Error, Warning, Info, Hint }`, `Diagnostic { range, severity, message, code }`, `Symbol`, `Reference`.
- [ ] Define `Analyzer` trait: `fn analyze(&self, source: &Source) -> AnalysisResult`.
- [ ] Define `Source { path: PathBuf, content: Vec<u8>, language: LanguageId }`.
- [ ] Strict purity: no I/O, no global state. The crate must build without depending on `clap`, `tokio`, `tracing`, etc.
- [ ] Verify: `cargo build -p qli-core` produces a tiny crate with minimal deps.

### 2B: `qli-lang` language adapter trait

- [ ] Define `Language` trait with: `fn id(&self) -> LanguageId`, `fn extensions(&self) -> &[&str]`, `fn parse(&self, source: &Source) -> ParseTree`, `fn analyze_with(&self, analyzer: &dyn Analyzer, tree: &ParseTree) -> AnalysisResult`.
- [ ] Define `LanguageRegistry` keyed on language id; supports lookup by file extension for `--lang auto`.
- [ ] Add `tree-sitter` as core dep; provide `TreeSitterLanguage` helper that wraps a grammar.
- [ ] Verify: registry can register/lookup languages; grammar wrapping compiles.

### 2C: Python adapter

- [ ] Create `qli-lang-python` crate; depend on `qli-lang`, `tree-sitter`, `tree-sitter-python`.
- [ ] Implement `Language` trait registering `.py` extension.
- [ ] Verify: parse a sample `.py` file end-to-end, produce a non-empty parse tree.

### 2D: TypeScript adapter

- [ ] Create `qli-lang-typescript` crate; depend on `qli-lang`, `tree-sitter`, `tree-sitter-typescript`.
- [ ] Handle both `.ts` and `.tsx` (two grammars in `tree-sitter-typescript`).
- [ ] Verify: parse `.ts` and `.tsx` samples; produce parse trees.

### 2E: Outputs

- [ ] `qli-outputs/human.rs` — pretty terminal output with file:line:col, color when TTY, severity icons.
- [ ] `qli-outputs/jsonl.rs` — one JSON object per diagnostic, one line each.
- [ ] Verify: same input, two formats; jsonl is parseable by `jq`.

### 2F: Cache

- [ ] In `qli-core`, define content-hashed cache keyed on `(language_id, blake3(content), analyzer_id, analyzer_version)` → `AnalysisResult`. The `analyzer_version` field invalidates the cache when an analyzer's behavior changes; bumping it is the analyzer's responsibility.
- [ ] Persist to `$XDG_CACHE_HOME/qli/<analyzer_id>/<analyzer_version>/<hash-prefix>/<hash>.json`. Directory layout makes orphaned versions trivially purgeable.
- [ ] **Eviction policy**: combine size cap + TTL.
  - Size cap: configurable, default 500 MB. Track total cache size in a sidecar `index.json` updated atomically.
  - TTL: configurable, default 30 days. Touched on hit (LRU-ish behavior).
  - Eviction runs lazily on cache write when size exceeds cap, or on demand via `qli ext cache clean`.
- [ ] Add `--no-cache` flag (skip both read and write for the current run).
- [ ] Add `qli ext cache clean [--all|--older-than <days>]` for manual eviction.
- [ ] Verify: second run on unchanged file is cache hit; modifying the file invalidates; bumping `analyzer_version` invalidates without touching disk; size cap triggers eviction of oldest entries.

### 2G: `qli analyze` command

- [ ] Add subcommand to `qli` binary.
- [ ] Args: `paths: Vec<PathBuf>` (positional), `--lang <id|auto>`, `--format <human|jsonl|auto>`, `--no-cache`, `-v`/`-q`.
- [ ] Auto-detect language from extension when `--lang auto`.
- [ ] Auto-detect format: `human` if stdout is a TTY, `jsonl` otherwise.
- [ ] Add `after_help` examples to `qli analyze` matching the 1B pattern (root + `completions`).
- [ ] Verify: `qli analyze foo.py` and `qli analyze foo.ts` both work; `qli analyze --help` shows examples; `| jq .` consumes jsonl output.

### 2H: Trivial seed analyzer (TODO/FIXME extractor)

- [ ] Create `qli-analyzers` crate (separate from `qli-core` so analyzers can be added without touching core types).
- [ ] Implement an `Analyzer` registry pattern (not a single hardcoded analyzer) — even though only two ship in Phase 2, the architecture must accommodate more.
- [ ] Implement `TodoFixme` analyzer: walks the parse tree, restricts matching to comment nodes (via tree-sitter — _not_ regex over raw bytes), regex-matches `TODO|FIXME|XXX|HACK` inside comment text, emits diagnostics.
- [ ] Each analyzer carries `analyzer_id: &'static str` and `analyzer_version: u32`. Cache key (Phase 2F) includes `analyzer_version` so cache invalidates when behavior changes.
- [ ] Same analyzer runs across both Python and TypeScript adapters — proves polyglot.
- [ ] Verify: a known fixture with mixed TODO/FIXME in Python and TypeScript files yields the expected count and locations. Bumping `analyzer_version` invalidates cache entries.

### 2I: Definition + reference extractor (Phase 4 prerequisite)

- [ ] Add a second analyzer `DefRefs` to `qli-analyzers` that emits `Symbol` (for definitions) and `Reference` (for usages) entries — the data SCIP and LSP go-to-def actually need.
- [ ] Per-language tree-sitter queries identify:
  - Function/method definitions and their names.
  - Class/struct definitions and their names.
  - Variable bindings at module scope.
  - Call sites referencing names defined elsewhere.
- [ ] For Phase 2, lexical resolution only — no cross-file resolution, no type inference. References resolve to a same-file definition if present; otherwise the reference is unresolved and recorded as such.
- [ ] Symbols carry stable IDs of the form `<scheme>:<package>:<file>:<symbol-path>` (loose precursor to SCIP symbol scheme — Phase 4 will formalize the scheme).
- [ ] Implement for Python and TypeScript adapters via tree-sitter queries committed to each adapter crate.
- [ ] Verify on a multi-file fixture: defining `foo` in `a.py` and calling `foo()` in `a.py` produces a `Symbol` and a resolved `Reference`. Calling `bar()` (undefined) produces an unresolved `Reference`.
- [ ] Output: works with `--format jsonl` to emit one symbol/reference per line; verify with `jq`.

### Phase 2 acceptance

- [ ] `qli analyze tests/fixtures/` produces expected TODO/FIXME diagnostics for both Python and TypeScript files.
- [ ] `qli analyze --analyzer defrefs tests/fixtures/` produces expected definitions and references.
- [ ] Same engine output, two output formats, both correct.
- [ ] Cache hit/miss observable via `-vv` logging; bumping analyzer version invalidates appropriately.
- [ ] Analyzer registry can dispatch to any registered analyzer by id.

## Phase 2.5: C# Adapter

- [ ] Create `qli-lang-csharp`; depend on `tree-sitter-c-sharp`.
- [ ] Implement `Language` for `.cs`.
- [ ] Add C# fixtures with TODO/FIXME comments.
- [ ] Verify: `qli analyze foo.cs` works with the same TODO/FIXME analyzer as Phase 2.

## Phase 2.6: Angular Template Adapter

- [ ] Research current best `tree-sitter-angular` grammar (compare options; Angular template syntax has multiple parsers in the ecosystem).
- [ ] Create `qli-lang-angular`; depends on the chosen grammar plus `qli-lang-typescript` for embedded expressions.
- [ ] Implement `Language` registering `.html` (component templates) — careful: not all `.html` files are Angular templates. Either require an opt-in marker or detect via project config (`angular.json`).
- [ ] Handle structural directives (`*ngIf`, `*ngFor`), bindings (`[prop]`, `(event)`, `[(ngModel)]`), interpolation (`{{ expr }}`).
- [ ] Bridge embedded TS expressions into the TS adapter for analysis.
- [ ] Add Angular fixtures with TODO/FIXME in templates.
- [ ] Verify: `qli analyze foo.component.html` works; embedded expressions are parsed.

## Phase 3: LSP Frontend

### 3A: `qli-lsp` crate

- [ ] Create `qli-lsp`; depend on `tower-lsp`, `tokio`, `qli-core`, `qli-analyzers`, language adapters.
- [ ] Implement basic server lifecycle: `initialize`, `initialized`, `shutdown`, `exit`.
- [ ] Implement document sync: `textDocument/didOpen`, `didChange`, `didSave`, `didClose`.
- [ ] On `didChange`, run `TodoFixme` and `DefRefs` (from Phase 2I) over the new document content; publish `Diagnostic` (from TodoFixme) via `textDocument/publishDiagnostics`.
- [ ] Implement `textDocument/definition` (go-to-def) using the `DefRefs` analyzer's symbol table — same-file references resolve to the local definition.
- [ ] **LSP cache strategy** (two-tier):
  - **In-memory per-document LRU** keyed by `DocumentUri`. Value is the most recent `(content_hash, AnalysisResult)`. Bounded size (default 200 documents). On `didChange`, hash content; if hash matches in-memory entry, skip re-analysis. This handles the per-keystroke load.
  - **Persisted hash cache** (the same one from Phase 2F) sits behind it for cold-start recovery. Same content-hash keys.
- [ ] Convert `qli-core` types to LSP types via `qli-outputs/lsp.rs` (`Position`, `Range`, `Diagnostic`, `Location`).

### 3B: `qli lsp` command

- [ ] Add `qli lsp` subcommand: `--stdio` (default), `--tcp <port>`.
- [ ] In `--stdio` mode, **all** logging goes to stderr or a file (never stdout — that's the LSP transport). In `--tcp` mode, stderr stays clean too (file logging only) to keep terminal usage sane.
- [ ] Verify: starting the server with `qli lsp --stdio` produces valid LSP handshake; `tower-lsp`'s test harness completes initialize→didOpen→publishDiagnostics roundtrip.

### 3C: VS Code extension + Helix config (real deliverables)

- [ ] Create `editors/vscode/` containing a minimal VS Code extension package: `package.json` declaring activation events for `.py`/`.ts`/`.tsx` files and `extension.js` that spawns `qli lsp --stdio` and wires it via `vscode-languageclient`. Build is `npm run package` → `qli-vscode-x.y.z.vsix`.
- [ ] Decide v1 distribution path for the VS Code extension: install from `.vsix` (simplest) or publish to the Marketplace (more setup, requires publisher account). v1 = `.vsix`; Marketplace publish is a backlog item.
- [ ] Create `editors/helix/languages.toml` snippet documenting how to register `qli lsp` as a language server for Python/TS in Helix.
- [ ] Commit a `editors/README.md` with install instructions for both editors.
- [ ] Manual smoke test: install the VS Code `.vsix` on a clean machine, open a Python file with TODO/FIXME, verify diagnostics appear in the Problems panel; trigger go-to-def on a same-file function reference, verify it jumps.

### Phase 3 acceptance

- [ ] LSP server starts, accepts edits, publishes diagnostics for known-bad fixtures.
- [ ] VS Code extension `.vsix` builds and works on a clean machine; Helix config works for at least Python.
- [ ] Go-to-definition works for same-file references in both Python and TypeScript fixtures.
- [ ] Per-keystroke editing on a 1k-line file feels responsive (subjective; document any latency hot spots in `editors/README.md`).

## Phase 4: SCIP Emission

**Prerequisite:** Phase 2I (definition + reference extractor) shipped. Phase 4 emits the symbols/references that 2I produces.

### 4A: SCIP symbol scheme design

- [ ] Define a per-language SCIP symbol scheme. Symbols follow SCIP's `<scheme> <package-manager> <package-name> <package-version> <descriptor>` format. Decide:
  - Scheme prefix (e.g., `scip-qli`).
  - How to map Python module paths and TypeScript file/module paths into SCIP descriptors.
  - How to handle local-only symbols (no public package).
- [ ] Document the scheme in `crates/qli-scip/SCHEME.md`.

### 4B: `qli-scip` crate

- [ ] Add `qli-scip`; depend on Sourcegraph's `scip` crate, `qli-core`, language adapters.
- [ ] Implement `qli-outputs/scip.rs`: convert `qli-core` Symbols and References (from Phase 2I) into SCIP `Document` and `Occurrence` protobufs using the scheme from 4A.

### 4C: `qli index` command

- [ ] Add `qli index` subcommand: walks a project root, runs the `DefRefs` analyzer over every supported file, emits a single `index.scip` file.
- [ ] Args: `path: PathBuf` (positional, default `.`), `--output <path>` (default `index.scip`), `--lang <ids>` (optional filter), `-v`/`-q`.
- [ ] Use the `ignore` crate (same one used by `ripgrep`) for `.gitignore`-respecting walks.
- [ ] Reuse the analysis cache from Phase 2F so re-indexing is incremental.

### 4D: Validation

- [ ] Install `scip` CLI in CI (binary release from Sourcegraph).
- [ ] Add an integration test: emit `index.scip` over a multi-file Python + TypeScript fixture, run `scip print index.scip`, assert expected symbols and references are present.
- [ ] Add a roundtrip test: write a fixture where `foo()` is defined in one file and called in another; verify the SCIP index records the reference resolving to the definition's symbol.
- [ ] (Optional) Document running a local Sourcegraph instance against the index for visual sanity check.

### Phase 4 acceptance

- [ ] `qli index path/to/project` produces a valid `index.scip` recognized by `scip print`.
- [ ] Symbols (function/class/variable definitions) and references (call sites resolved to defs) from Phase 2I's `DefRefs` analyzer appear correctly in the index for both Python and TypeScript fixtures.
- [ ] Cross-file references resolve where the analyzer can resolve them; unresolved references are still recorded.

## Cross-cutting / standing tasks

- [ ] Quarterly: bump `rust-toolchain.toml` to current latest stable, verify CI passes, commit. MSRV (`Cargo.toml` `rust-version`) is a separate decision — only bump it when a dependency forces it or you adopt a feature that requires it.
- [ ] Quarterly: review `Cargo.lock` for security advisories (`cargo audit`).
- [ ] Each phase: update README.md with installed-features state.
- [ ] Each phase: update this `tasks.md` with discovered tasks; check off as completed.
- [ ] Maintain `plans/backlog/` for ideas that surface mid-implementation but don't belong in the active plan.
- [ ] May adopt later when justified (do not promote to Phase 1): `cargo-nextest`, `cargo-llvm-cov`, `proptest` beyond the manifest parser, fuzzing, MCP error-path expansion.
