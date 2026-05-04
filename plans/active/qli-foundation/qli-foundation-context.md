# Context: qli — Polyglot Code Analysis CLI + Extension Framework
**Last Updated:** 2026-05-03 (Phase 1 complete; acceptance gate green; CI green post-1L)

## SESSION PROGRESS

- **2026-05-03 — Phase 1 acceptance gate green.** Built `target/release/qli` and copied it alone into a tempdir under ephemeral `XDG_CONFIG_HOME`/`XDG_DATA_HOME`/`XDG_STATE_HOME`/`XDG_CACHE_HOME` + `HOME`; ran every acceptance bullet against that binary. CI also green post-push (runs [25292961221](https://github.com/QLangstaff/qli/actions/runs/25292961221) and [25292968387](https://github.com/QLangstaff/qli/actions/runs/25292968387) both pass all 8 jobs).
    - **Bullet 1.** `qli --help` lists dev/org/prod from the embedded layer; `qli dev hello`, `qli org hello`, `QLI_ENV=prod qli --yes prod hello` all run end-to-end with the prod banner + a clean two-line `start`/`finish` audit JSONL at `$XDG_STATE_HOME/qli/prod-audit.log`.
    - **Bullet 2.** `qli ext install-defaults` wrote 6 files (3 manifests + 3 scripts), skipped 0; dropping `dev/greet` (chmod +x) made it appear in `qli dev --help` next to `hello` and `qli dev greet world` echoed the forwarded arg.
    - **Bullet 3.** `qli prod fake-cmd` produces clap's generic-error template: `error: unrecognized subcommand 'fake-cmd'` + `Usage: qli prod [OPTIONS] [COMMAND]` + `For more information, try '--help'.` — same shape as `git foo` / `cargo foo`. Brief detour during evaluation: I initially flagged this as ambiguous because the literal `without QLI_ENV` clause is a no-op (parse failure short-circuits the `requires_env` guard) and clap renders no tip-line for far-from-anything typos. After re-reading, the bullet's "errors clearly with a suggestion" intent is satisfied by the existing template — it scopes usage to `qli prod` and points at `--help`. Adjacent paths verified for completeness: `qli prod hellp` → clap tip `'hello', 'help'`; `qli prod hello` (valid extension, `QLI_ENV` unset) → exit 1 with `set it with: export QLI_ENV=prod`. **Process lesson**: a single ambiguous bullet doesn't need a planning detour — if the implementation matches industry-standard behaviour (git / cargo / clap defaults), accept and move on rather than re-litigating.
    - **Bullet 4.** CI green on the post-1L push (link above).
    - **Tooling note carried over from 1L Push B**: `cargo build --release -p qli` finished in 2.71s incremental on the cached tree.

- **2026-05-03 — Phase 1L Push B complete (consumer tests: 2 of 5 tasks).** Tasks 4 (trycmd CLI snapshots) and 5 (assert_cmd dispatcher integration tests) shipped on top of the Push A harness. All five 1L task items now done; Phase 1 acceptance can be evaluated next.
    - **Dispatcher integration tests in [`crates/qli/tests/dispatcher.rs`](../../../crates/qli/tests/dispatcher.rs)** (6 tests) covering happy path + one failure per guard + SIGINT integration:
        - `dev_hello_runs_clean` (happy, embedded `dev` group)
        - `prod_without_env_fails_with_export_hint` (`requires_env` failure, exit 1, `export QLI_ENV=prod` hint)
        - `prod_non_tty_without_yes_fails_confirm` (`confirm` failure, exit 1, `--yes` hint — assert_cmd's piped stdin trips IsTerminal correctly)
        - `env_secret_failure_leaves_audit_empty` (`secrets` failure via env-provider with unset reference; exit 1; audit log stays empty because resolution short-circuits before audit-start, per Phase 1F)
        - `successful_run_writes_audit_start_and_finish` (`audit_log` success path — exactly two JSONL records written)
        - `sigint_during_slow_extension_writes_interrupted_audit` — exits **130** as the plan task specified. First-pass implementation asserted 143 (programmatic `kill -INT <qli_pid>` only reaches the parent; ctrlc handler forwards SIGTERM; `sleep 30` dies of SIGTERM → 143) and rationalised the deviation as an `unsafe_code = "forbid"` constraint blocking `pre_exec`+`setpgid`. **That was wrong** — `std::os::unix::process::CommandExt::process_group(0)` is a *safe* stdlib API (stable since Rust 1.64) that does the same `setpgid(0,0)` after fork. Fix: spawn qli with `.process_group(0)` so it's its own pgrp leader, then `killpg(qli.pid, SIGINT)` reaches both qli and the child simultaneously (terminal Ctrl+C model). Child dies of SIGINT (130), qli propagates `128 + 2 = 130`, audit records `interrupted`. The audit `signal` field still reads `SIGTERM` per the Phase 1F simplification (handler always labels SIGTERM regardless of originating signal); test asserts event presence, not label. **Process lesson**: when an apparent constraint blocks the plan's spec, search the safe-API surface harder before declaring deviation — `process_group` was the right answer all along, and surfacing the deviation only after work was done forced a redo plus an awkward conversation.
    - **Mirrored harness in [`crates/qli/tests/common/mod.rs`](../../../crates/qli/tests/common/mod.rs).** Same shape as `qli-ext/tests/common/mod.rs::XdgSandbox` (with the post-Push-A partial-construction fix carried over), plus a `stage_extension` helper that writes `<sandbox>/data/qli/extensions/<group>/_manifest.toml` + a chmod-755'd script. Cross-crate test pattern is now consistent.
    - **trycmd CLI snapshots in [`crates/qli/tests/cli.rs`](../../../crates/qli/tests/cli.rs) + [`crates/qli/tests/cmd/`](../../../crates/qli/tests/cmd/)** (6 case files):
        - `version.toml` — `qli --version` exact match
        - `completions-zsh.toml` — `#compdef qli` header + `...` multi-line elision (clap regenerates the script body on every minor bump; snapshotting the body would be high-churn for zero contract value)
        - `completions-help.toml` — `qli completions --help`. Locks the static `COMPLETIONS_AFTER_HELP` examples block from Phase 1B exactly; no path noise because help output is path-free for static built-in subcommands. Same lesson as the SIGINT exit-code one: an earlier draft skipped this (and dropped the count from six to five) on the premise that all `--help` output had path noise. Wrong — only the *root* `--help` does, because of dynamic group synthesis. `qli completions --help` is the static counter-example.
        - `unknown-no-tip.toml` — `qli foo` → exit 2, no tip, fallback `For more information, try '--help'`
        - `unknown-with-tip.toml` — `qli porod hello` → exit 2, `tip: a similar subcommand exists: 'prod'`
        - `missing-env.toml` — `qli prod hello` (no `QLI_ENV`) → exit 1, banner + `export QLI_ENV=prod` hint
    - **Root `qli --help` deliberately not snapshotted.** The dynamic group/extension synthesis from Phase 1E renders `xdg`/`embedded` paths in subcommand `about` text, which differ between dev machine and CI runner. `qli completions --help` covers the after_help-examples surface cleanly without the path noise.
    - **trycmd `env_remove` not in API.** `TestCases::env(...)` only sets, not unsets. The harness wraps the test body in an `EnvUnset` RAII guard that removes `QLI_ENV` in the test process before invoking `TestCases` and restores prior value on `Drop`. Otherwise a developer with `QLI_ENV=prod` exported in their shell would see the missing-env case behave differently from CI.
    - **trycmd snapshot syntax recap (caught two issues during Push B drafting):**
        - `[..]` matches characters within a single line.
        - `...` (own line) matches any number of lines. Used in `completions-zsh.toml` to elide the script body.
        - TOML `"""..."""` preserves the trailing blank line, so a literal close-quote on its own line introduces an extra `\n` — caught when `unknown-no-tip` and `unknown-with-tip` failed with one trailing-newline diff.
    - **Tooling**: added `assert_cmd = "2"`, `predicates = "3"`, `serial_test = "3"`, `tempfile = "3"`, `trycmd = "0.15"` as dev-deps to `qli`, plus unix-only `nix = "0.29"` (signal feature) for the SIGINT test's `killpg(2)` call. None affect production binary; all are dev-only.
    - **Verify (full suite, after both fix-ups)**:
        - `cargo test --workspace` — 73 tests pass; SIGINT test now asserts exit 130 (matches plan), trycmd test now runs 6 cases.
        - `cargo clippy --workspace --all-targets -- -D warnings` clean.
        - `cargo fmt --all -- --check` clean (auto-applied once: rustfmt wrapped four `.env(K, V)` calls onto multiple lines because the V exceeded 100 chars).
        - `XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent XDG_STATE_HOME=/nonexistent XDG_CACHE_HOME=/nonexistent cargo test -p qli -p qli-ext` — green; nothing in either crate reaches the host XDG dirs during `cargo test`.
        - `cargo test --workspace -- --test-threads=4` — green; `#[serial]` gates correctly serialize env-mutating tests under higher concurrency.
        - `TRYCMD=overwrite cargo test -p qli --test cli` regenerates with zero `git diff` output (snapshots are byte-stable).
    - **Test count by binary**: qli unit 3 / qli integration 7 (dispatcher 6 + cli 1) / qli-core integration 1 (dependency_purity) / qli-ext unit 60 / qli-ext integration 2 (secrets_never_leak + xdg_sandbox_smoke) = 73.
    - **What's NOT in Push B (defer to Phase 1 acceptance / future work):**
        - Snapshotting the *root* `qli --help` output. See "Root `qli --help` deliberately not snapshotted" above; would need machine-specific path redactions.
        - Engine-purity allowlist for non-qli-core crates (qli-ext, qli-outputs, etc.). The plan only specified qli-core; other crates legitimately need third-party deps and would just have a long allowlist.

- **2026-05-03 — Phase 1L Push A complete (foundation: 3 of 5 tasks).** Tasks 1 (fixture root), 2 (hermetic harness + `serial_test` refactor), and 3 (engine-purity test) shipped. Push B will land tasks 4 (trycmd CLI snapshots) and 5 (`assert_cmd` dispatcher integration tests).
    - **Plan-vs-code tension flagged + resolved early.** The 1L task says "gate `Env`-provider tests with `serial_test`" but `crates/qli-ext/src/secrets.rs:263–268` carried a deliberate "we do *not* depend on `serial_test`" comment from 1G. Surfaced via advisor before refactoring. User chose **option 1 (plan supersedes) + two-push split**: keep unique-name discipline as first line of defence, add `#[serial]` as hard second line. The deliberate comment was rewritten to explain the two-layer approach. 10 env-mutating unit tests across `audit.rs`/`dispatch.rs`/`guard.rs`/`secrets.rs` got `#[serial_test::serial]` annotations.
    - **Hermetic harness in [`crates/qli-ext/tests/common/mod.rs`](../../../crates/qli-ext/tests/common/mod.rs).** `XdgSandbox` is an RAII guard that creates a `TempDir`, points `HOME` + the four `XDG_*` vars at subdirs, and restores prior values on Drop. Deliberately minimal per the advisor's "don't build a fixture framework" guidance — no helpers beyond `path()`, `data_dir()`, `extensions_dir()`, `state_dir()`. `OnePassword`-as-trait is already satisfied by the `SecretsResolver` trait + `TestResolver` from 1F/1G; no new code needed for that part of the task. Smoke test [`xdg_sandbox_smoke.rs`](../../../crates/qli-ext/tests/xdg_sandbox_smoke.rs) asserts the override + restore cycle so a typo in the harness fails here, not from a confusing failure deep in a real test.
    - **Engine-purity test in [`crates/qli-core/tests/dependency_purity.rs`](../../../crates/qli-core/tests/dependency_purity.rs).** Uses `cargo_metadata = "0.18"` (dev-dep) to load qli-core's manifest, filters to `DependencyKind::Normal`, and asserts every dep appears in a hardcoded `ALLOWED_DIRECT_DEPENDENCIES = &[]`. `[dev-dependencies]` deliberately ungated — `cargo_metadata` itself is the obvious dev-only dep. Adding a permitted dep means extending the constant + writing a justifying note in the PR; the bar is high.
    - **Both negative-path verifies passed pre-push** (per advisor's pre-flight checklist):
        - `XDG_CONFIG_HOME=/nonexistent XDG_DATA_HOME=/nonexistent XDG_STATE_HOME=/nonexistent XDG_CACHE_HOME=/nonexistent cargo test -p qli -p qli-ext` — green. Nothing in qli-ext or qli reaches out to the host's `~/.config/qli` / `~/.local/share/qli` during `cargo test`.
        - Added `tracing = "0.1"` to qli-core's `[dependencies]`, ran the engine-purity test, confirmed failure: `Disallowed direct dependencies: ["tracing"]. Either remove them from qli-core/Cargo.toml or add them to ALLOWED_DIRECT_DEPENDENCIES in this test...`. Reverted; positive path passes. The test bites as designed.
        - `cargo test -p qli-ext -- --test-threads=4` — green; `#[serial]` correctly serializes env-mutating tests under higher concurrency.
        - `cargo clippy --workspace --all-targets -- -D warnings` clean (after fixing two `clippy::pedantic doc_markdown` lints in the harness/smoke doc comments — `assert_cmd` and `trycmd` need backticks); `cargo fmt --all -- --check` clean.
    - **Test counts (workspace `cargo test`):** qli unit 3 + qli-ext unit 60 + qli-ext integration 2 (secrets_never_leak + xdg_sandbox_smoke) + qli-core integration 1 (dependency_purity) = 66 total. Was 64 pre-1L (1 integration + 60 unit qli-ext, 3 unit qli, 0 in qli-core).
    - **Tests/fixtures README** at [`tests/fixtures/README.md`](../../../tests/fixtures/README.md). Per-language subdirs (`python/`, `typescript/`, etc.) deliberately not created — they land with the phase that needs them (2H Python+TypeScript, 2.5 C#, 2.6 Angular). Spells out workspace-root path discipline + what does NOT belong here (per-test scratch, generated artifacts, gitignored content).
    - **Open follow-ups for Push B:**
        - **Task 4 (trycmd CLI snapshots):** add `trycmd` dev-dep + harness at `crates/qli/tests/cli.rs` driving case files in `crates/qli/tests/cmd/`. Per advisor, narrow scope to 6 cases (`--version`, `--help`, `completions zsh` head only, unknown subcommand no-tip, typo with tip, missing-env export-hint). Don't snapshot the full `completions zsh` body — clap upgrades regenerate it and the diff drowns the actual contract.
        - **Task 5 (assert_cmd dispatcher integration tests):** mirror the `XdgSandbox` harness into `crates/qli/tests/common/mod.rs`, add `assert_cmd` + `predicates` dev-deps, write tests covering happy path + at least one failure path per guard (`requires_env`, `confirm`, `secrets`, `audit_log`). Plus the SIGINT integration test (additive over the existing `dispatch::tests::signal_forwarding...` unit test — different layer, both worth keeping per advisor).
    - **Post-Push-A review fix.** Code review flagged a partial-construction env leak in `XdgSandbox::new()`: the original built `saved` as a bare `Vec` and only wrapped it in `Self` after the loop, so a panic in `create_dir_all` or `set_var` mid-loop would skip `Drop` and leave the process env mutated. Restructured to construct `Self` first (with `saved: Vec::with_capacity(...)`) and push into `sandbox.saved` as the loop progresses, so the value is stack-resident and Drop unwinds through any panic. `cargo test -p qli-ext --test xdg_sandbox_smoke` still green.

- **2026-05-02 — Phase 1K complete (pending first-push verify).** [`.github/workflows/ci.yml`](../../../.github/workflows/ci.yml) added with five jobs:
    - `fmt` (Linux only — `cargo fmt --all -- --check`)
    - `clippy` (matrix Linux + macOS — `cargo clippy --workspace --all-targets -- -D warnings`)
    - `test` (matrix Linux + macOS — `cargo test --workspace`)
    - `build` (matrix Linux + macOS — `cargo build --release --workspace`)
    - `audit` (Linux only — `rustsec/audit-check@v2`)
    - **Triggers**: `push` on `main`, all `pull_request`s, plus `merge_group` (no-op until the user enables a merge queue; cheap to include now). `permissions: contents: read` keeps the default token narrow.
    - **Toolchain**: `actions-rust-lang/setup-rust-toolchain@v1` reads `rust-toolchain.toml` automatically — Rust version is single-sourced from the pin file (currently 1.95.0). The action also bundles `Swatinem/rust-cache@v2`, so caching is one less moving part. Each job runs `rustup show active-toolchain` as a sanity check so a misconfigured runner fails loudly naming the version it's using.
    - **Matrix**: `ubuntu-latest` (Linux x86_64) + `macos-14` (macOS arm64). Windows excluded — cargo-dist handles cross-compile in 1.5A. Clippy runs on both OSes because `cfg(unix)` code paths exist (`audit::write_locked`'s `nix::fcntl::Flock`, `discovery::is_executable`'s `mode & 0o111`); a Linux-only lint would miss platform-gated regressions.
    - **`cargo audit` simplification (deliberate)**: the action fails CI on **any** RustSec advisory, not the "≥ medium" tier the task originally specified. RustSec advisories don't carry consistent CVSS severity; tier filtering would require either custom JSON parsing of `cargo audit --json` or adopting `cargo-deny` for `[advisories] severity-threshold`. The plan ([qli-foundation-plan.md:64](qli-foundation-plan.md)) explicitly chose standalone `cargo audit` over `cargo-deny`; this matches that stance. If advisory noise becomes a real problem, switch to `cargo-deny` rather than building parallel JSON-filter logic.
    - **Audit job needed `token:` and `checks: write`.** First-push attempt omitted `token:` on the assumption the action would fall back to running `cargo audit` directly without it — wrong: `rustsec/audit-check@v2` made `token:` a required input, and the job failed with `Input required and not supplied: token` (run [25282206006](https://github.com/QLangstaff/qli/actions/runs/25282206006)). Fixed by passing `${{ secrets.GITHUB_TOKEN }}` and adding a job-level `permissions: { contents: read, checks: write }` block (workflow-level `contents: read` is preserved for every other job; only the audit job widens). Re-verified green on run [25282714217](https://github.com/QLangstaff/qli/actions/runs/25282714217) — all 8 jobs pass.
    - **All 8 jobs empirically green on push:** `rustfmt`, `clippy (ubuntu-latest)`, `clippy (macos-14)`, `test (ubuntu-latest)`, `test (macos-14)`, `build (ubuntu-latest)`, `build (macos-14)`, `cargo-audit`. The deliberate-clippy-violation and known-bad-advisory experiments from the original verify list remain unchecked — they're a "prove the gates fail when they should" experiment, distinct from the "prove the gates succeed on a clean tree" verify that this run satisfied. Reasonable to leave for the user's first real PR.
    - **Branch protection deferred to repo-public.** Tried `gh api repos/QLangstaff/qli/branches/main/protection --method PUT` with the 8 status-check contexts on 2026-05-03 → 403 `Upgrade to GitHub Pro or make this repository public`. Branch protection on private repos is a paid-plan feature; the project's "no paid services" constraint ([context.md:233](#constraints)) rules out the upgrade. Considered the public→protect→private workaround, but unverified suspicion was that the rules would go inactive when the repo flipped back to private (toothless protection is worse than no protection — invisible to the user). User chose to skip rather than verify-and-flip-and-flip-back. Re-enable when the repo goes public for 1.5A/1.5B distribution; the `gh api` call is documented in [tasks.md → 1K branch-protection item](../qli-foundation/qli-foundation-tasks.md#1k-ci) so it's a one-shot at that point.
    - **Open follow-up (low-priority, not 1K-blocking):** `actions/checkout@v4` and `rustsec/audit-check@v2` both run on Node.js 20, which GitHub deprecates on 2026-06-02 (forced cutover) and removes on 2026-09-16. `actions/checkout@v5` ships Node.js 24; `rustsec/audit-check` has not yet released a Node 24 version. Bump checkout when convenient; audit-check waits on upstream.
    - **Local pre-flight on the same commands the workflow runs**: `cargo fmt --all -- --check` clean; `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo test --workspace` 60 (qli-ext) + 3 (qli) unit + 1 integration pass; `cargo build --release --workspace` succeeds; `cargo audit` exits 0 (advisory db loaded 1067 advisories; 158 dep crates scanned; no matches). High confidence the workflow will be green on first push.
    - **What's NOT done in 1K**:
        - **Branch protection** (Settings → Branches → main → require `fmt`, `clippy (ubuntu-latest)`, `clippy (macos-14)`, `test (ubuntu-latest)`, `test (macos-14)`, `build (ubuntu-latest)`, `build (macos-14)`, `cargo-audit`). UI-only setting, can't be done from YAML; the user has to enable it after the first run confirms the job names render as expected.
        - **Empirical verify** ("a deliberate clippy violation fails CI; revert passes; pinned crate with a known advisory fails audit"). Requires a push to GitHub. Local equivalents pass; the workflow has not yet been seen running.
- **2026-05-02 — Origin label unification (post-1J review).** Code review flagged that `crates/qli/src/ext.rs::describe` rendered origins as `XDG`/`embedded`/`PATH` for the `--help` blurb while `qli ext list` / `qli ext which` rendered them as `xdg`/`embedded`/`path`. Consolidated to a single source of truth: added `ExtensionOrigin::as_str(self) -> &'static str` in [`crates/qli-ext/src/discovery.rs`](../../../crates/qli-ext/src/discovery.rs) returning the canonical lowercase label. `ext::describe` and the formerly local `origin_label` helper in `main.rs` both call it. User-visible change: `--help` extension blurbs now read `xdg: <path>` (was `XDG: <path>`); `qli ext list` / `--json` output unchanged. Workspace `cargo test` 60 + 3 + 1 still green; clippy + fmt clean.
- **2026-05-02 — Phase 1J complete.** UserError-vs-panic separation lands as a panic hook; the existing typed-error chain already covered everything else.
    - **Discriminating fact found early**: clap 4 already produces did-you-mean suggestions for unknown subcommands (`qli porod hello` → `tip: a similar subcommand exists: 'prod'`), and `GuardError::EnvMissing` already emits the exact `export X=Y` line. Two of the three task items in the plan were already satisfied by shipped behavior; rolling a parallel Levenshtein would have created two suggestion sources users see inconsistently.
    - **Panic hook** ([`crates/qli/src/panic.rs`](../../../crates/qli/src/panic.rs)) installed at the top of `main()`. Replaces Rust's default panic UI ("thread 'main' panicked at file:line: msg" + auto-backtrace prompt) with a 2- or 3-line message naming the bug as a bug, pointing at the issue tracker, and including the location + message a maintainer needs to triage. `RUST_BACKTRACE` handling: `set_hook` replaces the default hook entirely (the standard-library runtime does not print backtraces independently of the hook), so `install` captures the prior hook via `take_hook()` and the installed closure delegates to it (via move-capture) when `RUST_BACKTRACE` is set. The "re-run with RUST_BACKTRACE=1" hint is suppressed in that branch. Side effect: the delegated default hook re-emits its own `thread 'main' panicked at ...` line above the backtrace — duplicate prefix is unavoidable without reimplementing backtrace capture from scratch. `panic_message` was factored to take `&dyn Any + Send` so unit tests can exercise the `&str` / `String` / unknown-payload decode paths without constructing a `PanicHookInfo` (which has no public constructor).
    - **No new `UserError` enum**: every expected failure already routes through the central `main()` renderer (`Err(err) => eprintln!("error: {err:#}")`) with typed underlying errors (`GuardError`, `SecretsError`, `DispatchError`, `MaterializeError`). Audited the workspace with `grep -rn 'eprintln!.*error:'`: only the central renderer and the panic hook produce `error:` lines (plus a test helper in `secrets_never_leak.rs` that's not user-facing). Adding a wrapping enum would have been a parallel structure to `anyhow` that earns nothing.
    - **Verify** (release binary, ephemeral XDG dirs):
        - `qli porod hello` → tip suggesting `prod` (clap), exit 2.
        - `qli dev hellp` → tip suggesting `hello, help` (clap), exit 2.
        - `qli foo` (no close match) → no tip; clap's `For more information, try '--help'` fallback, exit 2.
        - `qli prod hello` (no `QLI_ENV`) → `set it with: export QLI_ENV=prod`, exit 1.
        - **Panic hook** verified by a standalone repro mirroring the `take_hook` + move-capture pattern (an indexed-out-of-bounds `Vec<i32>`): without `RUST_BACKTRACE`, the 3-line bug-report message + hint; with `RUST_BACKTRACE=1`, the bug-report message followed by the delegated default hook's `thread 'main' panicked at ...` line + full stack frames. The earlier in-`main()` panic-trigger smoke ran before the `take_hook` chaining was wired and erroneously claimed a backtrace was produced; that misobservation is what flagged the bug.
    - **Tests:** 3 new unit tests in `panic::tests` (str payload, String payload, unknown payload). `qli` crate now has 3 unit tests (was 0); workspace total `cargo test` 60 unit (qli-ext) + 3 unit (qli) + 1 integration green. Clippy + fmt clean.
    - **Open follow-ups:** none for 1J. Phase 1K (CI) and Phase 1L (test scaffolding) are next; the test-binary harness Phase 1L plans (`assert_cmd`, `serial_test`, `tests/common/mod.rs`) would be the natural place to add a subprocess-style end-to-end test for the panic hook if the manual one-shot smoke ever feels insufficient.
- **2026-05-02 — Phase 1I complete.** Meta commands wired; binary now exposes `qli ext list`, `qli ext which`, and a `qli self-update` stub.
    - **`Cli::Command` additions** ([`crates/qli/src/cli.rs`](../../../crates/qli/src/cli.rs)): `Ext { action: ExtAction }` gained `List { json }` and `Which { group, name, json }` alongside the existing `InstallDefaults { force }`. New top-level `SelfUpdate { json }`. Both `--json` flags are local to their subcommand (not global) — keeps the existing global flag surface unchanged.
    - **Output discipline** (per `Constraints` → "Unix-style discipline"):
        - `qli ext list` and `qli ext which` write data to **stdout**.
        - `qli ext install-defaults` writes its summary to **stderr** (status, not data — already correct from 1H).
        - `qli self-update` writes its message + JSON payload to **stderr** and exits **2 (`USAGE`)**, so a script chained on `&&` halts at the stub instead of treating it as success.
    - **Output shapes**:
        - `list` plain: tab-separated `<group>\t<extension>\t<origin>\t<path>` rows. Tabs (not whitespace alignment) so paths with spaces don't break parsing; `column -t` for visual alignment.
        - `list --json`: `[{group, extension, origin, path}, ...]`, pretty-printed for interactive use; `jq -c .` re-collapses.
        - `which` plain: just the path on stdout (Unix `which` semantics).
        - `which --json`: `{group, extension, origin, path}`.
        - Origin labels: `xdg` | `embedded` | `path` (lowercase, matches JSON convention).
        - `self-update --json`: `{"status": "not_implemented", "available_in": "1.5E", "install_methods": [...]}`.
    - **Error handling**: `qli ext which <group> <name>` on unknown extension exits 1 with `error: unknown extension `<group> <name>`; run `qli ext list` to see what's available` on stderr; stdout stays empty so a pipe-and-fail script doesn't see partial output.
    - **No new tests in `qli-ext`** — the meta commands live in the binary and their formatting is mechanical (`serde_json::json!` + `writeln!`). Smoke covered all 10 paths (plain + `--json` for each command, error path, `xdg` vs `embedded` origin labelling, jq round-trip). Workspace `cargo test` 60 unit + 1 integration green; clippy clean; fmt clean.
    - **New dep:** `serde_json = "1"` added to `crates/qli/Cargo.toml` (was already in `qli-ext`'s deps so no lockfile change).
    - **Open follow-ups** (1J / 1.5E):
        - 1J will wrap top-level error rendering with closest-match suggestions; the `which` "did you mean" text is currently inline.
        - 1.5E replaces the `self-update` stub with the real install-method-detecting implementation; the `--json` payload shape (`status` / `available_in` / `install_methods`) is intentionally minimal so 1.5E can extend it without breaking parsers.
- **2026-05-02 — Phase 1H complete.** Default extensions ship embedded; dispatch-time XDG↔embedded merge works.
    - **Default extension fixtures** at the repo root: [`extensions/dev/`](../../../extensions/dev/), [`extensions/prod/`](../../../extensions/prod/), [`extensions/org/`](../../../extensions/org/). `prod` carries the full guard set (`requires_env QLI_ENV=prod`, `confirm = true`, `audit_log = "$XDG_STATE_HOME/qli/prod-audit.log"`, banner). All `hello` scripts are bash + `0o755`.
    - **`crates/qli-ext/src/defaults.rs`** (new module): `static DEFAULTS: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../extensions")`, plus `materialize_to(target, force) -> MaterializeStats` with typed `MaterializeError`. Top-level files are skipped (so the repo's `extensions/README.md` does not pollute the user's XDG dir); only group subdirectories are walked. Non-manifest files get `0o755` on Unix after writing — `include_dir` does not preserve mode bits, so without this discovery's `is_executable` filter would silently drop every shipped script. **Crate-publish caveat** documented in the module header: `cargo publish` strips files outside the crate dir, so 1.5C needs to either copy `extensions/` into `crates/qli-ext/`, configure `Cargo.toml`'s `include` field, or move the canonical location into the crate.
    - **Discovery refactor**: `crates/qli-ext/src/discovery.rs::discover` now takes `&[(&Path, ExtensionOrigin)]` (was a single `&Path`). Sources scanned in priority order; the first source to claim a group keeps it **wholesale** — manifest + extensions list. Per-group (not per-extension) shadowing means a user who deletes `<xdg>/dev/hello` does not see it bleed back from embedded. Added `ExtensionOrigin::Embedded` (existing `Xdg` / `Path` unchanged); `ext::describe` in the binary picks up the new variant.
    - **Binary wiring** (`crates/qli/src/main.rs`):
        - `materialize_embedded_layer()` runs at startup, materializing `DEFAULTS` to `$XDG_CACHE_HOME/qli/embedded/<CARGO_PKG_VERSION>/`. Idempotent; failure prints a `warning:` and disables the embedded layer for that run (XDG still works).
        - `discover` is called with `[(xdg_extensions, Xdg), (embedded_cache, Embedded)]`.
        - New `Cli::Command::Ext { action: ExtAction::InstallDefaults { force } }` routes to `dispatch_ext`, which calls `materialize_to(<xdg>/extensions, force)` and prints `installed defaults to <path>: wrote N, skipped M (use --force to overwrite)`.
    - **Smoke verified end-to-end** (binary alone in `/tmp/qli-clean/`, ephemeral XDG dirs, no repo on disk):
        - **Empty XDG**: `qli --help` lists dev/org/prod from embedded; `dev hello`, `org hello`, `--yes prod hello` (with `QLI_ENV=prod`) all run.
        - **`install-defaults`** writes 6 files (3 manifests + 3 scripts) and does NOT install the top-level `README.md`.
        - **XDG override**: editing `<xdg>/dev/hello` to print a distinctive marker → `qli dev hello` runs the edited version (XDG shadows embedded).
        - **Idempotent + `--force`**: a no-flag second run writes 0 / skips 6; `--force` rewrites 6 and overwrites user edits.
    - **Tests:** 60 unit tests in `qli-ext` (was 51): +6 in `defaults::tests` (DEFAULTS contents, materialize writes manifests + scripts, exec-bit-on-scripts-only, idempotent-without-force, force-overwrites, top-level-files-skipped), +3 in `discovery::tests` (`embedded_visible_when_xdg_missing_group`, `xdg_shadows_embedded_per_group` — asserts a per-extension `only-embedded` does NOT bleed through when XDG defines the group, `distinct_groups_layer_across_sources`). The seven existing discovery tests were updated for the new `discover(&[(&Path, ExtensionOrigin)])` signature. Integration test `secrets_never_leak` unchanged (it constructs `Group`/`Extension` directly, doesn't call `discover`). `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.
    - **Open follow-up for 1I:** the `qli ext` subcommand is now real (not just reserved). Phase 1I will hang `list` / `which` off the existing `ExtAction` enum — same pattern as `InstallDefaults`. Phase 1.5C must address the `include_dir!` publish caveat before publishing `qli-ext` to crates.io.
- **2026-05-02 — Phase 1G complete.** Real secret providers wired in; the 1F `StubResolver` is gone.
    - **`OnePassword` provider** ([`crates/qli-ext/src/secrets.rs::resolve_one_password`](../../../crates/qli-ext/src/secrets.rs)): spawns `op read <ref>`. Output mapping split into a `parse_op_output(spec, io::Result<Output>)` helper so unit tests construct fake `io::Result<Output>` values directly — every error branch (NotFound spawn, PermissionDenied spawn, non-zero exit with stderr, non-zero exit empty stderr, non-UTF-8 stdout, LF/CRLF/no terminator) is covered without a fake `op` binary on PATH. Strips exactly one trailing `\n` (and the preceding `\r` if present); preserves internal whitespace.
    - **`Env` provider** ([`resolve_env`](../../../crates/qli-ext/src/secrets.rs)): `std::env::var(spec.reference)` → bind under `spec.env`. Both `VarError::NotPresent` and `VarError::NotUnicode(_)` map to `SecretsError::Resolution { provider: "env", … }`. Tests use `env != reference` (e.g., `env = "TARGET"`, `ref = "QLI_TEST_PAT"`) so a future swap of the two would fail loudly.
    - **`ProductionResolver`** dispatches per-spec on `SecretProvider`. Re-exported from `qli_ext`; `crates/qli/src/main.rs` constructs `ProductionResolver::new()` and the 1F `StubResolver` is removed. The `SecretsResolver` trait surface is untouched.
    - **Diagnostics policy** carries through unchanged: failure variants only ever surface env-var names + `op`'s own stderr, never resolved values; resolution happens before audit-start so a failed run leaves the audit log untouched (verified during smoke).
    - **Tests:** 11 new unit tests (3 `Env` + 8 `OnePassword`) for a total of 51 in `qli-ext` (was 40). `secrets_never_leak` integration test still green. `cargo clippy --workspace --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.
    - **Manual smoke** (transient `XDG_DATA_HOME` / `XDG_STATE_HOME`): `dev hello` runs (control); `envprov hello` with `QLI_TEST_PAT` set echoes the value into the child's `TARGET` and the audit log records only `"env_var_names":["TARGET"]`; same with `QLI_TEST_PAT` unset fails closed naming the reference; `opprov hello` (where `provider = "one_password"`) on a machine without `op` fails closed with the install + `op signin` hint and the audit log stays empty.
    - **`extensions/README.md`** gained a `### Providers` section documenting the `op read` and `std::env::var` semantics + the failure modes for each. No "ships in 1G" stub language was present in the README to remove.
    - **Trailing-newline assumption note:** the `op` CLI was not installed on the smoke machine, so the "exactly one trailing `\n`" assumption could not be live-verified. Test fixtures cover both LF and CRLF terminators and the no-terminator case; if a real `op` ever returns multiple terminators, only the last one is stripped (the rest stay in the value). Revisit when the user first wires a real 1Password ref through.
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
