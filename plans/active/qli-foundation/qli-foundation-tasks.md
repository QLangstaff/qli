# Task Checklist: qli — Polyglot Code Analysis CLI + Extension Framework
**Last Updated:** 2026-05-02 (Phase 1J shipped)

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

- [x] Defined `_manifest.toml` schema in `qli-ext` (`crates/qli-ext/src/manifest.rs`) using `serde` + `toml` 0.8 + `thiserror` 2. `Manifest` carries `schema_version: u32`, `description: String`, `banner: Option<String>`, `requires_env: HashMap<String,String>` (default empty), `confirm: bool` (default false), `audit_log: Option<String>`, `secrets: Vec<SecretSpec>` (default empty). `SecretSpec` carries `env`, `reference` (TOML key `ref` via `#[serde(rename = "ref")]` since `ref` is a Rust keyword), and `provider: SecretProvider`. `SecretProvider` enum has `OnePassword` and `Env` variants with `#[serde(rename_all = "snake_case")]` so TOML uses `provider = "one_password"` / `provider = "env"`, matching the surrounding key style. Every struct uses `#[serde(deny_unknown_fields)]` so typos like `audti_log` fail loudly. `audit_log` is stored as a `String` (not `PathBuf`) because `$XDG_STATE_HOME` / `~` are still literal until the dispatcher expands them in Phase 1F — `String → PathBuf` happens at that boundary. `Manifest` implements `FromStr` (not an inherent `from_str`, to satisfy `clippy::should_implement_trait`).
- [x] Reject schema-version mismatches via two typed variants: `ManifestError::SchemaVersionTooNew { found, supported }` (`found > supported`, message: `manifest schema_version {found} is newer than this qli build supports ({supported}); upgrade qli or downgrade the manifest`) and `ManifestError::SchemaVersionInvalid { found, supported }` (`found < supported`, e.g. `0`, message: `manifest schema_version {found} is invalid (this qli build supports {supported})`). `CURRENT_SCHEMA_VERSION` const = 1.
- [x] **Parse-time SecretSpec validation** (fail at the manifest boundary, not deep in dispatch): `Manifest::from_str` walks every `[[secrets]]` entry via `validate_secret_spec`. Rejects empty `env`, `env` containing `=`, `env` containing NUL, and empty `ref` — all conditions that would otherwise crash `Command::env` at exec time. New variants `ManifestError::InvalidSecretEnv { env, reason: &'static str }` and `ManifestError::InvalidSecretRef { env, reason: &'static str }`.
- [x] Documented schema in `extensions/README.md` with TOML example covering every field, a field-summary table, a `SecretSpec` table, and a "schema versioning" paragraph noting this is pre-1.0 mutable surface.
- [x] Verify: 13 unit tests in `manifest::tests` pass — minimal manifest (defaults applied), full manifest (both providers, snake_case values), `schema_version = 2` rejected as `SchemaVersionTooNew`, `schema_version = 0` rejected as `SchemaVersionInvalid`, missing `schema_version` (serde error mentions the field name), unknown field (`audti_log` typo, error names the offender), unknown provider value (`vault`), stale PascalCase value (`OnePassword`) rejected to lock in the casing decision, `ref`-keyword round-trip, and four secret-spec validation tests (empty env, `=` in env, NUL in env, empty ref). `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean; full workspace `cargo test` passes.

### 1E: Extension discovery

- [x] Discover groups: subdirs of `$XDG_DATA_HOME/qli/extensions/` containing a `_manifest.toml` (`crates/qli-ext/src/discovery.rs::scan_xdg_root`). Flat structure only. Embedded `include_dir!` defaults are deferred to Phase 1H — discovery is structured around an `extensions_root: &Path` so the second source slots in cleanly when 1H lands.
- [x] PATH-only groups rejected with warning: `merge_path_binaries` emits `PATH binary \`qli-<group>-<name>\` references unknown group \`<group>\`; create extensions/<group>/_manifest.toml to enable it`.
- [x] Within each group, discover executable files; skip `_manifest.toml` and any other `_*` file (`scan_group_executables`). Non-UTF-8 names skipped with a warning.
- [x] Discover `qli-<group>-<name>` on `PATH` via `scan_path_for_qli_binaries`: `std::env::split_paths` walk + `strip_prefix("qli-").split_once('-')`. Per-advisor blind spot #2: malformed names (`qli-`, `qli-foo` no separator, `qli-foo-` trailing empty) warn and skip rather than producing empty group/extension entries.
- [x] **Collision rule**: XDG wins at `merge_path_binaries`; warning text matches the plan verbatim (`extension \`<group> <name>\` exists in both XDG (...) and PATH (...); using XDG. Use \`qli ext which\` to inspect.`).
- [x] **Clap dynamic subcommand strategy = Option B.** `crates/qli/src/main.rs` calls `Cli::command()` for the static derive tree, then loops over `discovery.groups` adding `ext::build_group_command(group)` synthesized in `crates/qli/src/ext.rs`. Names are leaked once at startup since `clap::Command::new` requires `Into<Str>` and `Str` only converts from `&'static str`. Globals are pulled from `ArgMatches` via `get_count`/`get_flag`/`get_one` rather than `Cli::from_arg_matches` (which would fail on synthesized subcommands).
- [x] Reserved-name guard (advisor blind spot #1): `RESERVED_GROUP_NAMES` const in `discovery.rs` blocks `analyze`, `completions`, `ext`, `help`, `index`, `lsp`, `mcp`, `self-update`. A user group named `completions` is skipped with a warning instead of panicking clap at `--help` time on duplicate subcommand. Forward-looking — only `completions` is registered today; the others are reserved for future phases.
- [x] Skip files without execute bit; warn on non-executables (`is_executable` checks `mode & 0o111` on Unix; non-Unix accepts any regular file pending a Windows port).
- [x] Per-extension descriptions: the Phase 1D manifest schema has no per-extension description field, so each extension's `about` shows `<origin>: <path>` where origin is `xdg`, `embedded`, or `path` (canonical label from `ExtensionOrigin::as_str`). The 1E verify said "from manifest if specified"; deferring a `[extensions.<name>]` table to a future schema bump if and when needed (out of scope for 1E).
- [x] Argument forwarding: extension subcommands set `disable_help_flag(true) + disable_version_flag(true)` so `--help`/`--version` reach the script, plus a `trailing_var_arg(true) + allow_hyphen_values(true) + num_args(0..) + value_parser!(OsString)` positional. Args round-trip via `matches.get_many::<OsString>("args")`.
- [x] Basic dispatch in `crates/qli-ext/src/dispatch.rs::run` — `Command::spawn`/`status` (not `exec`) so Phase 1F can wrap the spawn with the guard sequence and write a post-run audit entry. Exit code maps signal exits to `128 + signo` on Unix.
- [x] Discovery warnings print BEFORE `get_matches` so they fire on `--help`, `--version`, and parse-error paths (clap exits before our post-`get_matches` code runs). Direct `eprintln!` — not gated on logging level.
- [x] Verify: 7 new unit tests in `discovery::tests` (missing root, happy path, `_*` skip, non-executable warning, malformed manifest, no-manifest skip, reserved name) + workspace `cargo build`/`clippy --all-targets -D warnings`/`fmt --check`/`test` all green. Manual smoke test against the documented acceptance:
    - `qli --help` lists the `dev` group with description (✓).
    - `qli dev --help` lists each extension with its resolved path (✓).
    - `qli dev foo arg1 --flag arg2` runs the XDG script and forwards `arg1 --flag arg2` (✓).
    - Exit code 7 from the child propagates to `qli`'s exit code 7 (✓).
    - PATH `qli-bogus-thing` warns about unknown group at startup (✓).
    - PATH `qli-dev-foo` colliding with XDG `dev/foo` warns and XDG wins (✓).
    - Reserved-name group `completions` skipped with warning (✓).
    - Malformed PATH binaries (`qli-orphan`, `qli-only-`) warn and skip (✓).
    - Non-executable file in extensions dir warns with `chmod +x` hint (✓).

### 1F: Dispatcher with guardrails

- [x] Group-level guards run in this order (each gates the next), implemented in [`crates/qli-ext/src/dispatch.rs::run`](../../../crates/qli-ext/src/dispatch.rs):
  1. Banner to stderr if set ([`guard::print_banner`](../../../crates/qli-ext/src/guard.rs)).
  2. `requires_env` checked — `EnvMissing` error includes `export X=Y` suggestion (`guard::check_requires_env`).
  3. Confirm gated *before* secret resolution. `--yes` short-circuits; non-TTY with `confirm = true` and no `--yes` returns `NonInteractiveRefuse` (`guard::run_confirm` + `guard::TtyConfirm` backed by `dialoguer::Confirm`).
  4. Secrets resolved up-front via [`SecretsResolver`](../../../crates/qli-ext/src/secrets.rs) trait; fail closed on the first error. Phase 1F freezes the trait surface; Phase 1G fills in `OnePassword`/`Env` providers. Production `qli` binary uses a `StubResolver` that returns `ProviderUnavailable` for any `[[secrets]]` until 1G ships; manifests without secrets are unaffected.
  5. `Start` audit event appended (`audit::append`, JSONL). Fields: `timestamp`, `user`, `group`, `extension`, `args`, `env_var_names` (names only).
  6. `std::process::Command::spawn` (not `exec`); child PID registered in shared `DispatchSignals` so the binary's ctrlc handler can forward SIGTERM via `nix::sys::signal::kill` (workspace-level `unsafe_code = "forbid"` rules out `libc::kill` directly).
  7. `child.wait()` blocks; stdin/stdout/stderr inherited transparently.
  8. `Finish` (or `Interrupted`, if signals flagged the run) audit event with `exit_code` + `duration_ms`.
- [x] Exit code propagates: `0..=255` → `ExitCode`, signal exits map to `128 + signo` on Unix.
- [x] Ctrl+C / SIGTERM forwarded to child (SIGTERM); after child exits, `Interrupted` audit entry written and parent returns `128 + signo` (verified by `dispatch::tests::signal_forwarding_writes_interrupted_audit_and_exits_with_signal_code` — exits 143 with a `SIGTERM` audit record).
- [x] Verify (manual smoke against `~/.local/share/qli/extensions/prod/`):
    - No `QLI_ENV` → fails with `missing required env var QLI_ENV ... set it with: export QLI_ENV=prod` (✓).
    - `QLI_ENV=prod` non-TTY without `--yes` → fails with `prod requires confirmation but stdin is not a TTY; pass --yes` (✓).
    - `QLI_ENV=prod --yes` → banner prints, child runs, audit log has `start` + `finish` lines with `env_var_names` and no values (✓).
    - `dev hello` (no guards) still runs normally (✓).
    - Manifest with `[[secrets]]` fails with `secret providers ship in Phase 1G` until 1G lands (✓).
- [x] Regression test (`crates/qli-ext/tests/secrets_never_leak.rs`): drives every guard path (happy, env_fail, confirm_decline, child_fail) under a helper subprocess with a distinct sentinel per scenario, asserts no sentinel appears in stdout / stderr / audit log. Status: green.
- [x] Color-routing decision: keep the env-mutation shim ([`apply_color_choice`](../../../crates/qli/src/cli.rs)). Recorded in [context.md](qli-foundation-context.md#color-routing-decision-resolved-2026-05-02). Banner + confirm prompts in 1F print plain text + dialoguer's defaults, both already honour `NO_COLOR` / `CLICOLOR_FORCE` via `anstream`/`console`.
- [x] **Fail-fast/fail-loud audit (post-1F polish, 2026-05-02).** Codified the diagnostic policy as a doc comment at the top of [`qli-ext::lib`](../../../crates/qli-ext/src/lib.rs) — four tiers: process-fatal (anyhow), dispatch-fatal (typed `DispatchError`), must-see warning (`eprintln!`, never `tracing::warn!` since `-q` would silence it), trace (`tracing`). Fixes applied to align the existing code with the policy:
    - Resolved-value NUL check before `Command::env`: `dispatch::apply_secret_env` returns `DispatchError::SecretValueInvalid { env, reason }` (value omitted from message) instead of letting stdlib panic. Test: `dispatch::tests::nul_in_resolved_secret_value_is_rejected_before_spawn`.
    - macOS `PIPE_BUF = 512` audit interleave: `audit::append` now takes an exclusive `nix::fcntl::Flock` for the write on Unix; the kernel releases the lock when the fd closes. Required adding `fs` to `nix`'s feature list in `qli-ext/Cargo.toml`.
    - Signal-handler install failure ([`crates/qli/src/signal.rs::install`](../../../crates/qli/src/signal.rs)): switched from `tracing::warn!` to `eprintln!("warning: ... Ctrl+C will not forward to running extensions")` so `-q` can't hide the degraded behaviour.
    - XDG data-dir resolution failure in [`crates/qli/src/main.rs`](../../../crates/qli/src/main.rs): replaced `paths::data_dir().map_or_else(|_| PathBuf::new(), …)` (silent swallow) with an explicit match that prints `warning: could not resolve XDG data dir (…); extensions are disabled. Set $XDG_DATA_HOME or $HOME and retry.` and proceeds with an empty discovery so built-in subcommands (`--version`, `completions`) still work.
    - The Phase 1D parse-time `SecretSpec` validation entry above is the manifest-side half of this same fail-fast pass.

### 1G: Secrets providers

- [x] Implement `OnePassword` provider in [`crates/qli-ext/src/secrets.rs::resolve_one_password`](../../../crates/qli-ext/src/secrets.rs): `Command::new("op").arg("read").arg(<ref>).output()`. Output mapping is split into a separate `parse_op_output(spec, io::Result<Output>)` helper so the parser is unit-testable without a fake `op` binary on `PATH`. Spawn `ErrorKind::NotFound` → `SecretsError::ProviderUnavailable` with "install the 1Password CLI and run `op signin`, then retry" hint; other spawn errors → `ProviderUnavailable` with the os error context. Non-zero exit → `SecretsError::Resolution` carrying `op`'s trimmed stderr + "is `op` signed in? run `op signin` and retry"; empty stderr falls back to "(status: <code>); …". Non-UTF-8 stdout → `Resolution { message: "secret value returned by `op read` is not valid UTF-8" }`. Strips exactly one trailing `\n` (then a preceding `\r` if present) from the value — preserves any other whitespace and any internal newlines a secret happens to carry.
- [x] Implement `Env` provider in [`resolve_env`](../../../crates/qli-ext/src/secrets.rs): `std::env::var(spec.reference)` — reads the env var named by `ref`, binds it under `spec.env`. `VarError::NotPresent` and `VarError::NotUnicode(_)` both surface as `SecretsError::Resolution { provider: "env", message: "env var `<ref>` is not set" / "is not valid Unicode" }`.
- [x] Already enforced by 1F: resolution happens up-front (`SecretsResolver::resolve_all` is called before `Command::spawn` in `dispatch::run`); first error short-circuits via `Result::collect` on the per-spec map. The dispatcher writes audit-start *after* `resolve_all` succeeds, so a resolution failure leaves the audit log empty (verified by smoke).
- [x] Already enforced by 1F + parse-time validation: secrets never enter `tracing` output; the `Start` audit event records `env_var_names` (names only). The new error variants from 1G never include a resolved secret value — the non-UTF-8 path drops the bytes, the `Env` failure paths only echo the *reference* name, and the `op read` non-zero-exit path echoes `op`'s stderr (which carries `op`'s own diagnostic, not the secret value).
- [x] `crates/qli/src/main.rs` now constructs `ProductionResolver::new()` (re-exported from `qli_ext`) instead of the 1F `StubResolver`; the stub struct is removed. `dispatch::run` continues to drive the `&dyn SecretsResolver` it received from 1F — the trait surface is untouched.
- [x] Verify (manual smoke under transient `XDG_DATA_HOME` / `XDG_STATE_HOME`):
    - `dev hello` (no secrets) — runs, exit 0 (control).
    - `envprov hello` with `[[secrets]] env = "TARGET", ref = "QLI_TEST_PAT", provider = "env"` and `QLI_TEST_PAT` set — child sees `TARGET=<value>`, audit log records `"env_var_names":["TARGET"]` and never the value.
    - `envprov hello` with `QLI_TEST_PAT` unset — fails closed, message: `could not resolve secret for env `TARGET` via env: env var `QLI_TEST_PAT` is not set`.
    - `opprov hello` with `provider = "one_password"` and `op` not on `PATH` — fails closed: `provider tool not available for env `OP_TOKEN`: `op` not found on PATH; install the 1Password CLI and run `op signin`, then retry`. Audit log empty (resolution failed before audit-start).
- [x] Tests: 11 new unit tests in `secrets::tests` (3 `Env` provider + 8 `OnePassword` provider — 7 unix-gated). `parse_op_output` exercised against `Err(NotFound)`, `Err(PermissionDenied)`, non-zero exit with stderr, non-zero exit with empty stderr, success with `\n` terminator, success with `\r\n` terminator, success with no terminator + internal `\n`, and non-UTF-8 stdout. Workspace `cargo test` 51 unit tests + 1 integration green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.

### 1H: Default extension stubs (embedded via `include_dir!`)

- [x] Created [`extensions/dev/_manifest.toml`](../../../extensions/dev/_manifest.toml) (no guardrails) + `dev/hello` (bash script printing `hello from dev`).
- [x] Created [`extensions/prod/_manifest.toml`](../../../extensions/prod/_manifest.toml) with `schema_version=1`, `description`, `banner = "PROD — irreversible; verify before proceeding"`, `confirm = true`, `audit_log = "$XDG_STATE_HOME/qli/prod-audit.log"`, and `[requires_env] QLI_ENV = "prod"`. Plus `prod/hello`.
- [x] Created [`extensions/org/_manifest.toml`](../../../extensions/org/_manifest.toml) + `org/hello`. All scripts chmod'd to 0o755 in the source tree.
- [x] [`crates/qli-ext/src/defaults.rs`](../../../crates/qli-ext/src/defaults.rs): `static DEFAULTS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../extensions")` plus `materialize_to(target, force) -> MaterializeStats { written, skipped }` and `MaterializeError { CreateDir, Write, Chmod }`. `include_dir` does not preserve mode bits, so `materialize_to` chmods every non-`_manifest.toml` file to 0o755 on Unix after writing — without that, discovery's `is_executable` filter would warn-and-skip every shipped script. Top-level files (the repo's `extensions/README.md`) are skipped entirely; only group subdirectories are walked. **Crate-publish caveat noted in module docs**: `include_dir!("$CARGO_MANIFEST_DIR/../../extensions")` works for workspace builds but `cargo publish` strips files outside the crate dir, so 1.5C will need to copy/include `extensions/` into `crates/qli-ext/` at publish time or move the canonical location into the crate.
- [x] Implemented `qli ext install-defaults [--force]` ([`crates/qli/src/cli.rs`](../../../crates/qli/src/cli.rs) `Command::Ext { action: ExtAction::InstallDefaults { force } }` → [`main::dispatch_ext`](../../../crates/qli/src/main.rs)). Writes to `$XDG_DATA_HOME/qli/extensions/`, prints `installed defaults to <path>: wrote N, skipped M (use --force to overwrite)` to stderr. Idempotent without `--force`; per-file skip granularity. `ext` was already in `RESERVED_GROUP_NAMES` (Phase 1E forward-looking); now it's a real subcommand and the reserved-name skip prevents user shadowing.
- [x] Dispatch-time merge implemented via two refactors: (1) `crates/qli-ext/src/discovery.rs::discover` now takes `&[(&Path, ExtensionOrigin)]` and walks each source in priority order — first source to claim a group keeps it **wholesale** (manifest *and* extensions list), so a user who deletes `dev/hello` from XDG does not see it re-appear from embedded. (2) `crates/qli/src/main.rs::materialize_embedded_layer()` extracts `DEFAULTS` to a version-keyed cache at `$XDG_CACHE_HOME/qli/embedded/<CARGO_PKG_VERSION>/` on every startup (idempotent, skips existing files), then `discover` is called with `[(xdg, Xdg), (cache, Embedded)]`. Failure to resolve `cache_dir` or extract files prints a warning and disables the embedded layer for the run; XDG-installed extensions still work. Added `ExtensionOrigin::Embedded` (existing `Xdg` and `Path` unchanged).
- [x] Verify: `cargo build --release -p qli`, copy binary alone to `/tmp/qli-clean/qli`, fresh ephemeral XDG dirs:
    - **Empty XDG** (no `install-defaults` run): `qli --help` lists dev/org/prod from embedded layer; `qli dev hello`, `qli org hello`, `qli --yes prod hello` (with `QLI_ENV=prod`) all run end-to-end including the prod banner + audit log.
    - **`qli ext install-defaults`** writes 6 files (3 manifests + 3 scripts), skips 0; the repo's top-level `extensions/README.md` is NOT installed (verified by `find` listing under XDG).
    - **XDG-shadows-embedded**: editing `<xdg>/extensions/dev/hello` to print a distinctive marker and re-running `qli dev hello` shows the user-edited output, not the embedded one.
    - **Idempotent + `--force`**: a second `install-defaults` run with no flag writes 0 / skips 6; with `--force` writes 6 / skips 0 and the user-edited file is overwritten back to the upstream content.
- [x] Tests: 6 new unit tests in `defaults::tests` (DEFAULTS contains expected groups, materialize writes manifests + scripts, exec bit on scripts only, idempotent without force, force overwrites, top-level files skipped) + 3 new layered-discovery tests (`embedded_visible_when_xdg_missing_group`, `xdg_shadows_embedded_per_group` including a per-extension shadowing assertion, `distinct_groups_layer_across_sources`). Workspace `cargo test` 60 unit + 1 integration green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.

### 1I: Meta commands

- [x] `qli ext list [--json]` ([`crates/qli/src/main.rs::dispatch_ext_list`](../../../crates/qli/src/main.rs)). Plain output is tab-separated `<group>\t<extension>\t<origin>\t<path>` rows on stdout (tabs preserve spaces in paths; `column -t` for visual alignment). `--json` is a flat array of `{group, extension, origin, path}` objects, pretty-printed for interactive readability and round-trip-safe through `jq -c .`. Origin labels: `xdg` | `embedded` | `path` (lowercase, sourced from `ExtensionOrigin::as_str` so `--help` / `qli ext list` / `qli ext which` agree).
- [x] `qli ext which <group> <name> [--json]` ([`dispatch_ext_which`](../../../crates/qli/src/main.rs)). Plain output is just the resolved path (Unix `which` semantics — easy to pipe into `cat`, `bat`, an editor). `--json` returns a single `{group, extension, origin, path}` object. Unknown group or extension exits 1 with a stderr error suggesting `qli ext list`.
- [x] `qli ext install-defaults [--force]` — landed in Phase 1H. See [1H entry](#1h-default-extension-stubs-embedded-via-include_dir).
- [x] `qli self-update [--json]` ([`dispatch_self_update`](../../../crates/qli/src/main.rs)). Stub: prints to **stderr** (it's a status message, not data), and exits **2 (USAGE)** so `cmd && qli self-update && cmd2` halts the chain instead of treating "no-op stub" as success. Plain form lists the three install methods (`cargo install qli --force`, `brew upgrade qli`, the curl installer); `--json` form emits `{"status": "not_implemented", "available_in": "1.5E", "install_methods": [...]}`. Phase 1.5E will replace it with the real install-method-detecting implementation.
- [x] Verify (smoke against the release binary, ephemeral XDG dirs):
    - **`qli ext list`** — tab-separated rows, exit 0; with `install-defaults` already run shows `xdg` origin, without shows `embedded` from the cache layer.
    - **`qli ext list --json | jq -r '.[].path'`** — round-trips cleanly; returns just the paths.
    - **`qli ext which dev hello`** — prints the resolved path on stdout, nothing else.
    - **`qli ext which dev hello --json`** — prints the JSON object on stdout.
    - **`qli ext which dev nonexistent`** — exits 1 with `error: unknown extension `dev nonexistent`; run `qli ext list` to see what's available` on stderr; stdout is empty.
    - **`qli self-update`** — prints stub message to stderr, exits 2; `--json` emits the structured payload to stderr, also exit 2.
- [x] No new unit tests added in `qli-ext` (the meta commands live in the binary; their formatting is mechanical `serde_json::json!` + tab-separated `writeln!`). Smoke verifies every output path. Workspace `cargo test` 60 unit + 1 integration green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.

### 1J: Error messages with suggestions

- [x] **UserError-vs-panic separation** via a panic hook in [`crates/qli/src/panic.rs`](../../../crates/qli/src/panic.rs), installed at the top of `main()`. Replaces Rust's default panic UI ("thread 'main' panicked at file:line: msg" + backtrace prompt) with one terse message: `error: qli encountered an internal bug. Please report it at https://github.com/QLangstaff/qli/issues` followed by `panic at <location>: <message>`. `RUST_BACKTRACE=1` path: `set_hook` replaces the default hook entirely (the std-library runtime does not print backtraces independently of the hook), so `install` captures the prior hook via `take_hook()` and the installed closure delegates to it (via move-capture) when `RUST_BACKTRACE` is set; the `re-run with RUST_BACKTRACE=1 for a backtrace` hint is suppressed in that branch. The delegated default hook re-emits its own `thread 'main' panicked at ...` line above the backtrace (duplicate prefix, unavoidable without reimplementing backtrace capture). Decided **not** to add a parallel `UserError` enum: every expected failure already routes through `main()`'s `Err(err) => eprintln!("error: {err:#}")` renderer with typed underlying errors (`GuardError`, `SecretsError`, `DispatchError`, `MaterializeError`); a wrapping enum would just delegate. Audited the workspace for stray `eprintln!.*error:` (`grep -rn 'eprintln!.*error:' crates/`) — only the central renderer in `main.rs` and the panic hook produce `error:` lines.
- [x] **Closest-match suggestions for unknown subcommands** are produced by clap 4 out of the box — `qli porod hello` → `tip: a similar subcommand exists: 'prod'`; `qli dev hellp` → `tip: some similar subcommands exist: 'hello', 'help'`. Decided **not** to roll a parallel Levenshtein implementation: two suggestion sources users see inconsistently is worse than relying on clap's. Far-from-anything typos (`qli foo`, `qli xyz`) get clap's no-tip fallback `For more information, try '--help'`, which is acceptable Unix-style guidance. The dynamic group/extension subcommands synthesized in `crates/qli/src/main.rs` participate in clap's suggestion ranker the same way the static ones do.
- [x] **Missing env var → exact `export` line** is already emitted by `GuardError::EnvMissing` in [`crates/qli-ext/src/guard.rs`](../../../crates/qli-ext/src/guard.rs): `missing required env var `QLI_ENV` (manifest expects `prod`); set it with: export QLI_ENV=prod`. Verified live in the smoke gate below; no code change needed.
- [x] Verify (release binary, ephemeral XDG dirs):
    - **`qli porod hello`** → `error: unrecognized subcommand 'porod'` + `tip: a similar subcommand exists: 'prod'`. Exit 2.
    - **`qli prod hello`** without `QLI_ENV` → `error: failed to run 'prod hello': missing required env var 'QLI_ENV' (manifest expects 'prod'); set it with: export QLI_ENV=prod`. Exit 1.
    - **`qli dev hellp`** → `tip: some similar subcommands exist: 'hello', 'help'`. Exit 2.
    - **`qli foo`** (no close match) → no tip; falls back to `For more information, try '--help'`. Exit 2.
    - **Panic hook** (verified by a standalone repro mirroring the `take_hook` + move-capture pattern with an indexed-out-of-bounds `Vec<i32>`): without `RUST_BACKTRACE` → 3 lines (bug-report URL, panic location + message, "re-run with RUST_BACKTRACE=1" hint), no Rust default UI. With `RUST_BACKTRACE=1` → bug-report message + the delegated default hook's `thread 'main' panicked at ...` line + full stack frames. The earlier in-`main()` panic-trigger smoke ran before the `take_hook` chaining was wired and erroneously claimed a backtrace was produced; that misobservation is what flagged the bug.
- [x] Tests: 3 new unit tests in `panic::tests` for `panic_message` decoding (`&str` payload, `String` payload, unknown payload). Workspace `cargo test` 60 unit (qli-ext) + 3 unit (qli) + 1 integration green; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.

### 1K: CI

- [ ] Add `.github/workflows/ci.yml`: jobs for `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, build matrix on macOS arm64, Linux x86_64.
- [ ] Add `cargo audit` job as a security gate (uses `rustsec/audit-check` action or runs `cargo audit` directly). Fails CI on advisories with severity ≥ medium; warns otherwise.
- [ ] Cache `~/.cargo` and `target/` keyed on `Cargo.lock` for speed.
- [ ] Block PR merge on CI green.
- [ ] Verify: a deliberate clippy violation fails CI; reverting passes. A pinned crate with a known advisory fails the audit job.

### 1L: Tests

- [ ] **Fixture root.** Create `tests/fixtures/README.md` documenting the workspace-root `tests/fixtures/<lang>/` convention; per-language subdirs are created by Phases 2H/2I as fixtures land. Verify: README exists; Phase 2H/2I/3/4 reference this path.
- [ ] **Hermetic test harness.** Establish the convention: each crate that runs hermetic tests carries a `tests/common/mod.rs` that builds a `tempfile::TempDir` and overrides `XDG_CONFIG_HOME` / `XDG_DATA_HOME` / `XDG_STATE_HOME` per test; gate `Env`-provider tests with `serial_test`; define `OnePassword` as a trait that unit tests stub. First instance lands in `crates/qli-ext/tests/common/mod.rs`; copy into other crates as Phase 1L items 4 and 5 land. Verify: `XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent XDG_STATE_HOME=/nonexistent cargo test -p qli -p qli-ext` is green; tests that mutate process env are gated with `#[serial]` and pass under `cargo test -- --test-threads=4`.
- [ ] **Engine-purity test.** Add `crates/qli-core/tests/dependency_purity.rs` parsing `cargo metadata` and asserting `qli-core`'s direct dependencies match a hardcoded allowlist constant (initially empty); runs under the existing `cargo test` CI job. Lands before Phase 2A merges. Verify: adding `tracing` to `qli-core/Cargo.toml` fails the test with a message naming the offender; reverting passes.
- [ ] **CLI contract snapshots.** Add `trycmd` dev-dep and a harness at `crates/qli/tests/cli.rs` driving case files under `crates/qli/tests/cmd/`; back-fill against shipped 1A–1C. Verify: `cargo test -p qli` green; `TRYCMD=overwrite cargo test -p qli` regenerates cleanly with no spurious diff.
- [ ] **Dispatcher unit + integration tests.** Unit tests in `qli-ext` for manifest parsing, discovery, and guard evaluation; integration tests in `crates/qli/tests/` using `assert_cmd` under the hermetic harness, plus one test that spawns the dispatcher with a slow child, sends SIGINT, asserts exit code 130 and that the audit log contains an "interrupted" entry. Verify: `cargo test` is green; happy paths and at least one failure path per guard (`requires_env`, `confirm`, `secrets`, `audit_log`).

### Phase 1 acceptance

- [ ] `qli --help`, `qli dev hello`, `qli prod hello` (with env + confirm), `qli org hello` all work end-to-end on a clean machine.
- [ ] Drop a new bash script in `~/.local/share/qli/extensions/dev/`, see it appear in `qli --help` immediately, run it.
- [ ] `qli prod fake-cmd` without `QLI_ENV` errors clearly with a suggestion.
- [ ] CI green.

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
