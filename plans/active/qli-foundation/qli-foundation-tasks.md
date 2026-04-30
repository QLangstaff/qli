# Task Checklist: qli — Polyglot Code Analysis CLI + Extension Framework
**Last Updated:** 2026-04-30

Each phase ships a working artifact. Don't start phase N+1 until phase N's "verify" tasks pass.

## Resolved structural decisions

These three forks have been locked. Documented here for reference; the phases below assume them.

- **Crate publishing:** publish every workspace crate (`qli`, `qli-core`, `qli-ext`, `qli-lang`, `qli-lang-*`, `qli-outputs`, `qli-lsp`, `qli-scip`) to crates.io under the `qli-*` prefix. Use `release-plz` or a topological-order script for releases. Required for `cargo install qli` to work from the registry.
- **Extension defaults:** embed the repo's `extensions/` directory into the binary at compile time via `include_dir!`. `qli ext install-defaults` writes these embedded defaults to `$XDG_DATA_HOME/qli/extensions/`. User-installed extensions always override at dispatch time.
- **SCIP prerequisite:** Phase 4 (SCIP emission) requires a real symbol/reference analyzer. Added as **Phase 2I — definition + reference extractor** below; Phase 4 cannot ship without it.

## Phase 0: Repo bootstrap

- [ ] **Verify crate name availability on crates.io** for `qli`, `qli-core`, `qli-ext`, `qli-lang`, `qli-lang-python`, `qli-lang-typescript`, `qli-lang-csharp`, `qli-lang-angular`, `qli-outputs`, `qli-lsp`, `qli-scip`. If `qli` is squatted, fall back to `qlictl` (or similar) and rename downstream `qli-*` crates to match. Lock the chosen name before any code.
- [ ] Reserve crate names by publishing empty `0.0.0` placeholders if any are taken (or pivot the prefix). Document the chosen name prefix in `README.md`.
- [ ] Initialize Cargo workspace at repo root (`Cargo.toml` with `[workspace]`, `members = ["crates/*"]`, shared `[workspace.package]` for version/license/edition).
- [ ] Add `rust-toolchain.toml` pinning `channel = "1.83.0"`, `components = ["rustfmt", "clippy"]`.
- [ ] Add `.gitignore` for Rust (`/target`, `Cargo.lock` rules, `.DS_Store`).
- [ ] Add `.editorconfig` (4-space tabs for Rust, LF line endings, trim trailing whitespace).
- [ ] Add `rustfmt.toml` with project conventions (e.g., `max_width = 100`, `imports_granularity = "Crate"`).
- [ ] Add `clippy.toml` (or `[lints]` in `Cargo.toml`) enabling `clippy::all` + selectively `clippy::pedantic` lints, denying warnings in CI.
- [ ] Confirm existing `LICENSE` is the intended license; record it in workspace `[workspace.package].license`.
- [ ] Replace placeholder `README.md` with a stub that describes the project and links to `plans/active/qli-foundation/`.
- [ ] Verify: `cargo check` from a fresh clone succeeds (no crates yet, but the workspace parses).

## Phase 1: Skeleton + Extension Dispatch

### 1A: Workspace crates (stubs)

- [ ] Create empty crates: `qli`, `qli-core`, `qli-lang`, `qli-outputs`, `qli-ext`. Each gets `Cargo.toml` + `src/lib.rs` (or `src/main.rs` for `qli`).
- [ ] Wire dependencies: `qli` depends on `qli-ext`. `qli-ext` does **not** depend on `qli-outputs` (decoupling: Phase 1 dispatcher prints banners/errors directly via `anstream`; `qli-outputs` is for analysis output formatters in Phase 2+). `qli` will pull in `qli-core`, `qli-lsp`, `qli-scip`, language adapters in their respective phases.
- [ ] Verify: `cargo build` succeeds.

### 1B: Core CLI scaffolding (in `qli` crate)

- [ ] Add `clap` (derive) with workspace root command `qli`.
- [ ] Implement `--version` (auto from `CARGO_PKG_VERSION`).
- [ ] Implement `qli completions <shell>` using `clap_complete` (bash, zsh, fish, powershell).
- [ ] Wire `tracing-subscriber` to log to stderr; respect `-v`/`-vv`/`-q` for level. Document precedence with `RUST_LOG` (env var overrides flags).
- [ ] Implement standard exit code conventions (0 success, 1 error, 2 usage, 130 SIGINT, 143 SIGTERM).
- [ ] Use the `ctrlc` crate to install a unified Ctrl+C / SIGTERM handler that flips an `AtomicBool` and exits cleanly with the right code (cross-platform; SIGTERM differs on Windows).
- [ ] TTY detection: use `std::io::IsTerminal` from stdlib (stable since Rust 1.70) — no extra crate needed.
- [ ] Color output: depend on `anstream` + `anstyle`. Wire `--color={auto,always,never}` flag; respect `NO_COLOR` automatically (handled by `anstream`).
- [ ] Add `--help` examples to every subcommand using clap's `after_help` (or `after_long_help`) attribute. Examples should show the most common usage on stdin/stdout/file inputs and machine-readable output via `--json`.
- [ ] Verify: `qli --version`, `qli --help`, `qli completions zsh > _qli` all work; `NO_COLOR=1 qli --help` produces no ANSI codes; pressing Ctrl+C during a long-running extension exits with code 130; `kill <pid>` exits with 143.
- [ ] Verify: `qli analyze --help` (once Phase 2 lands) shows examples in its help output.

### 1C: XDG path resolution

- [ ] Add `directories` crate; expose helpers `config_dir()`, `cache_dir()`, `state_dir()`, `data_dir()` for `qli`.
- [ ] On first run, ensure these directories exist (mkdir -p semantics).
- [ ] Verify: directories created at expected XDG paths on macOS + Linux.

### 1D: Extension manifest schema

- [ ] Define `_manifest.toml` schema in `qli-ext` using `serde`. Fields:
  - `schema_version: u32` (start at 1).
  - `description: String`.
  - `banner: Option<String>` (printed to stderr before any extension in this group runs).
  - `requires_env: Option<HashMap<String, String>>` (e.g., `{ QLI_ENV = "prod" }`).
  - `confirm: bool` (default `false`).
  - `audit_log: Option<PathBuf>` (path supports `$XDG_STATE_HOME` expansion).
  - `secrets: Vec<SecretSpec>` where `SecretSpec` is `{ env: String, ref: String, provider: SecretProvider }`.
  - `SecretProvider`: `OnePassword | Env`.
- [ ] Reject manifests with unknown `schema_version` with a clear error.
- [ ] Document the schema in `extensions/README.md`.
- [ ] Verify: unit tests parse valid manifests and reject malformed ones with helpful messages.

### 1E: Extension discovery

- [ ] Discover groups (subdirs of `$XDG_DATA_HOME/qli/extensions/` and the embedded defaults from `include_dir!`). Flat structure only — no nested subgroups in v1.
- [ ] A group requires a `_manifest.toml` to exist (in XDG dir or embedded). PATH-only groups (i.e., `qli-foo-bar` exists on PATH but no `foo` manifest anywhere) are **rejected** with a warning: "PATH binary `qli-foo-bar` references unknown group `foo`; create `extensions/foo/_manifest.toml` to enable it." This prevents PATH from silently creating unguarded groups.
- [ ] Within each group, discover executable files (skip `_manifest.toml`, skip files starting with `_`).
- [ ] Also discover `qli-<group>-<name>` executables on `PATH`.
- [ ] **Collision rule**: if both `$XDG_DATA_HOME/qli/extensions/<group>/<name>` and PATH `qli-<group>-<name>` exist, the XDG dir wins. Warn the user once at discovery time: "extension `<group> <name>` exists in both XDG and PATH; using XDG. Use `qli ext which` to inspect."
- [ ] **Clap dynamic subcommand strategy**: pick one and document the choice in code comments:
  - Option A: `Command::allow_external_subcommands(true)` — simpler but loses help integration for groups.
  - Option B: enumerate discovered groups/extensions at startup and synthesize `Command::subcommand(...)` entries dynamically; full help integration but adds a dispatch shim.
  - Decision: Option B — full help integration is worth the extra code. Discovery runs once at startup; the synthesized clap tree includes every group/extension with its description from the manifest.
- [ ] Skip files without execute bit; warn on non-executables in extensions dir.
- [ ] Verify: drop a `chmod +x` script in `~/.local/share/qli/extensions/dev/foo`, run `qli dev foo` — it executes. `qli --help` lists the `dev` group with its description; `qli dev --help` lists `foo` with its description (from manifest if specified). PATH binary `qli-bogus-thing` produces a warning to stderr at startup.

### 1F: Dispatcher with guardrails

- [ ] Before running an extension, execute group-level guards in **this order** (each step gates the next):
  1. Print `banner` to stderr if set.
  2. Check `requires_env` — fail with clear error and "set X=Y" suggestion if not satisfied.
  3. **Confirm before secrets**: if `confirm` is true and stdin is a TTY, prompt the user; if not a TTY and `--yes` not passed, refuse with clear error. Confirming early avoids fetching secrets the user is going to abort on (and prevents unnecessary `op` audit entries).
  4. Inject `secrets` (resolve via 1Password CLI for `OnePassword`, env lookup for `Env`). All secrets resolved up-front; fail closed on any resolution error.
  5. Append start entry to `audit_log` (timestamp, user, command, args, env var **names** only — never values).
  6. Spawn the extension as a child process via `std::process::Command::spawn` (not `exec` — the dispatcher must remain alive to write the post-run audit entry, propagate exit codes, and forward signals).
  7. Wait on the child; forward stdout/stderr/stdin transparently.
  8. After child exits, append finish entry to audit log with exit code and duration.
- [ ] Propagate exit code from extension.
- [ ] On Ctrl+C / SIGTERM mid-extension: forward the signal to the child, wait briefly, write a partial audit entry indicating interrupted, exit with the right code (130 / 143).
- [ ] Verify: a `prod` group extension fails without `QLI_ENV=prod`; with env set, shows banner and confirm prompt; refuses non-interactively without `--yes`; with `--yes` runs and writes start+finish audit entries; secrets land in child env but never in audit log; Ctrl+C during the child writes an "interrupted" audit entry and exits 130.

### 1G: Secrets providers

- [ ] Implement `OnePassword` provider: spawn `op read <ref>`, capture stdout, surface stderr on failure with "is `op` installed and signed in?" hint.
- [ ] Implement `Env` provider: just `std::env::var(name)`.
- [ ] Resolve all secrets *before* `exec`-ing the extension; fail closed if any resolution errors.
- [ ] Never log secret values, never include them in audit log.
- [ ] Verify: missing `op` produces a clear error; resolved secrets land in extension's env; audit log contains the env var names but not values.

### 1H: Default extension stubs (embedded via `include_dir!`)

- [ ] Create `extensions/dev/_manifest.toml` (no guardrails) and `extensions/dev/hello` (a bash script printing "hello from dev").
- [ ] Create `extensions/prod/_manifest.toml` (`requires_env = { QLI_ENV = "prod" }`, `confirm = true`, `audit_log = "$XDG_STATE_HOME/qli/prod-audit.log"`, `banner = "PROD — irreversible; verify before proceeding"`) and `extensions/prod/hello`.
- [ ] Create `extensions/org/_manifest.toml` and `extensions/org/hello`.
- [ ] Add `include_dir!` macro invocation in `qli-ext` (or a new `qli-defaults` crate) that embeds the repo's `extensions/` directory into the binary at compile time.
- [ ] Implement `qli ext install-defaults` that walks the embedded directory, writes each file (including manifests) to `$XDG_DATA_HOME/qli/extensions/<group>/<name>` with execute bit preserved on scripts, and is idempotent (skips files that already exist unless `--force` is passed).
- [ ] At dispatch time, user-installed files in `$XDG_DATA_HOME` always override embedded defaults; the dispatcher merges the two sources.
- [ ] Verify: a freshly installed `qli` (no repo on disk) successfully runs `qli ext install-defaults` and then `qli dev hello`, `qli prod hello`, `qli org hello`. Build the binary, copy it alone to `/tmp/qli-clean/`, run it from there with empty `$XDG_DATA_HOME` to test.

### 1I: Meta commands

- [ ] `qli ext list` — list discovered extensions with origin (XDG dir vs PATH) and group.
- [ ] `qli ext which <group> <name>` — print resolved path.
- [ ] `qli ext install-defaults` — copy repo defaults to XDG.
- [ ] `qli self-update` — stub that prints "not yet implemented; install via brew/cargo/curl. Phase 1.5."
- [ ] Verify: each meta command works and produces machine-readable output with `--json`.

### 1J: Error messages with suggestions

- [ ] Wrap top-level error reporting: `UserError`-like enum vs unexpected panics. `UserError` prints message + suggestion, no traceback.
- [ ] For "command not found", suggest closest match using a small Levenshtein implementation against discovered extensions.
- [ ] For "missing env var" errors, suggest the exact `export` line.
- [ ] Verify: `qli porod hello` suggests `qli prod hello`. `qli prod hello` without env says "set QLI_ENV=prod to continue."

### 1K: CI

- [ ] Add `.github/workflows/ci.yml`: jobs for `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test`, build matrix on macOS arm64, Linux x86_64.
- [ ] Add `cargo audit` job as a security gate (uses `rustsec/audit-check` action or runs `cargo audit` directly). Fails CI on advisories with severity ≥ medium; warns otherwise.
- [ ] Cache `~/.cargo` and `target/` keyed on `Cargo.lock` for speed.
- [ ] Block PR merge on CI green.
- [ ] Verify: a deliberate clippy violation fails CI; reverting passes. A pinned crate with a known advisory fails the audit job.

### 1L: Tests

- [ ] Unit tests in `qli-ext` for manifest parsing, discovery, guard evaluation.
- [ ] Integration tests using `assert_cmd` exercising real subprocess dispatch with a temp HOME.
- [ ] Verify: `cargo test` is green; happy paths and at least one failure path per guard.

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
- [ ] Verify: `qli analyze foo.py` and `qli analyze foo.ts` both work; `| jq .` consumes jsonl output.

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

- [ ] Quarterly: bump `rust-toolchain.toml` to the latest stable, verify CI passes, commit.
- [ ] Quarterly: review `Cargo.lock` for security advisories (`cargo audit`).
- [ ] Each phase: update README.md with installed-features state.
- [ ] Each phase: update this `tasks.md` with discovered tasks; check off as completed.
- [ ] Maintain `plans/backlog/` for ideas that surface mid-implementation but don't belong in the active plan.
