# Comprehensive Plan: qli — Polyglot Code Analysis CLI + Extension Framework
**Last Updated:** 2026-04-30

## Executive Summary

`qli` is a Rust CLI that combines two capabilities behind one consistent UX:

1. **Built-in polyglot code analysis** — a Rust engine over tree-sitter that powers `qli analyze` (CLI), `qli lsp` (language server), and `qli index` (SCIP emitter). Architecturally pure: one engine, multiple frontends.
2. **External extension dispatch** — any-language scripts organized into groups (`dev`, `prod`, `org`, ...), discovered from `$XDG_DATA_HOME/qli/extensions/<group>/<name>` or `qli-<group>-<name>` on PATH. Pattern follows `git`/`gh`/`cargo`/`kubectl`. The dispatcher enforces per-group guardrails (env requirement, confirmation, audit log, secrets injection from 1Password) so a bash one-liner gets the same protection as a Python script.

The goal is one tool that replaces ad-hoc personal automation scripts AND cross-codebase analysis tooling, aligned with modern CLI best practices (clig.dev, XDG, NO_COLOR, GNU long flags), distributable via Homebrew, `cargo install`, GitHub release binaries, a curl installer, and a Claude Code plugin.

## Strategic Approach

**Architecture committed up front, delivery phased.** Getting the engine/frontend split right on day one is load-bearing — bolting `core/` purity on later is painful. Each phase ships a working product on its own:

- **Phase 1** ships a usable extension dispatcher: drop in scripts, get safety + consistency for free.
- **Phase 1.5** makes the binary installable via standard channels, adds Claude Code integration, lights up self-update.
- **Phase 2** introduces the analysis engine. Two analyzers ship: TODO/FIXME (polyglot diagnostics) and definitions + references (symbols/refs needed by LSP go-to-def and SCIP). Two seed languages: Python + TypeScript.
- **Phase 2.5 / 2.6** add C# and Angular template adapters (mechanical / structural respectively).
- **Phase 3** wires the engine into a tower-lsp server. With the def+ref analyzer in place, this is meaningfully useful (diagnostics + go-to-def).
- **Phase 4** adds SCIP whole-repo indexing. With Phase 2I in place, the SCIP output contains real symbols and references — the SCIP semantic delivers as designed.

**Key design principles:**

- **Engine purity.** `qli-core` has no I/O, no clap, no rich output. Pure library. Multiple frontends share it. This is the `ruff`/`biome`/`deno` pattern and the only shape that scales to LSP + CLI + SCIP without contortion.
- **Outputs are pluggable formatters** over engine output: `human` (TTY default), `jsonl` (machine), `scip` (indexer), `lsp` (editor). Adding a new output = one file, no engine changes.
- **Extension safety in the dispatcher, not in scripts.** Group `_manifest.toml` declares `requires_env`, `confirm`, `audit_log`, `secrets`. Dispatcher enforces them before exec'ing the script. Language-agnostic — bash, python, go all get the same guarantees.
- **Standards alignment from day one.** clig.dev rules baked in: stdout=data / stderr=chatter, standard exit codes, standard flag names, `NO_COLOR`, TTY-aware color, XDG paths, shell completion, `--help` examples, SIGTERM handling, error messages with suggestions.
- **Lazy expensive work.** Heavy imports (tree-sitter grammars, SCIP protobufs) are only loaded by the commands that need them — `qli dev hello` should not pay for the analysis machinery.

## Scope

### In Scope

- Rust workspace with `qli`, `qli-core`, `qli-lang`, `qli-lang-{python,typescript,csharp,angular}`, `qli-outputs`, `qli-lsp`, `qli-scip`, `qli-ext` crates — **all published to crates.io** under the `qli-*` prefix (the main binary as `qli`, libraries as `qli-core`, `qli-ext`, etc.).
- Extension dispatcher with discovery from XDG dir + PATH, group manifests, env/confirm/audit/secrets guardrails.
- 1Password CLI (`op`) and env-var secrets providers.
- Built-in commands: `qli analyze`, `qli lsp`, `qli index`, `qli ext list/install/which`, `qli self-update`, `qli completions`.
- Default extension stubs in `dev/`, `prod/`, `org/` — **embedded into the binary at compile time via `include_dir!`** so a fresh `cargo install qli` / `brew install qli` has working defaults without needing the repo present.
- Distribution via `cargo-dist`: cross-compiled GitHub releases (macOS arm64/x86_64, Linux x86_64/arm64, Windows x86_64), Homebrew tap (`QLangstaff/homebrew-qli`), `cargo install` via crates.io, `curl | sh` installer.
- Claude Code plugin: skill documenting qli usage, slash commands wrapping common ops, optional MCP server exposing `qli analyze` / `qli index` as MCP tools.
- CI: lint (clippy), format check (rustfmt), tests, release build matrix, `cargo audit` security gate.
- **Two analyzers in Phase 2**:
  - **TODO/FIXME extractor** (Phase 2H) — proves polyglot diagnostics across languages.
  - **Definition + reference extractor** (Phase 2I) — extracts function/class definitions and call sites; provides the symbol/reference data that Phase 3 (LSP go-to-def) and Phase 4 (SCIP) actually need to be meaningful. Required prerequisite for Phase 4's acceptance criteria.

### Out of Scope

- Real analysis logic beyond the seed extractor. Specific analyzers ship after Phase 2 is solid.
- Multi-user / team sharing of extensions beyond a Homebrew tap and GitHub releases.
- WASM-based plugin system (revisit post-Phase 4).
- Platform-specific package managers beyond Homebrew (e.g., apt, dnf, winget) — out of scope for v1.
- GUI / web frontend.
- Telemetry / analytics.

## Risks & Mitigations

- **Risk: Architecture-first plan over-engineers Phase 1.** _Mitigation: Phase 1 ships only the dispatcher + scaffolding; engine/lsp/scip are deferred. Each crate exists from Phase 1 only as an empty stub if needed for the workspace, and is filled in its own phase._
- **Risk: tree-sitter grammar inconsistency across languages.** Each grammar has its own quirks (TS template literals, C# preprocessor, Angular embedded TS). _Mitigation: `qli-lang` defines a strict adapter trait; per-language crates absorb grammar quirks behind it. Phase 2 proves the trait with two languages before C#/Angular pile on._
- **Risk: Angular template parsing is harder than C#.** Templates are HTML + embedded TS expressions + structural directives. _Mitigation: Phase 2.6 is its own milestone, layered on top of working TS support; not bundled with Phase 2 or 2.5._
- **Risk: `cargo-dist` cross-compile failures (macOS notarization, Windows toolchain).** _Mitigation: Phase 1.5 is its own phase; budget time for CI iteration. Homebrew tap and crates.io are fallbacks if release binaries lag._
- **Risk: 1Password CLI not installed on user's machine.** _Mitigation: Manifest declares secrets provider; dispatcher errors with a clear install hint if `op` is missing. Env-var fallback is always available for non-prod groups._
- **Risk: Self-update interacting badly with Homebrew/cargo installs.** _Mitigation: `self-update` detects install method via canonical-path heuristics; for Homebrew/cargo it prints the correct upgrade command rather than fighting the package manager._
- **Risk: Scope creep from "this would be cool too."** _Mitigation: This plan freezes scope at the listed phases. New ideas land in a `plans/backlog/` doc, not bolted onto active phases._
- **Risk: LSP integration assumes editor smoke test for validation.** _Mitigation: Phase 3 acceptance criterion is "diagnostics appear in VS Code or Helix on a known-bad file." Editor configs committed to repo._
- **Risk: Rust toolchain pin drifts from CI.** _Mitigation: `rust-toolchain.toml` is the single source of truth; CI uses it. Quarterly refresh tracked in `tasks.md` backlog._
- **Risk: Extension manifest schema becomes a versioning surface.** _Mitigation: Manifest carries `schema_version`; dispatcher rejects unknown versions with a clear message. Schema changes are explicit, not implicit._
- **Risk: `qli` crate name unavailable on crates.io.** Three-letter names are often squatted. _Mitigation: Phase 0 includes a name-availability check on crates.io for `qli` and every planned `qli-*` crate. If `qli` is taken, fall back to `qlictl` or similar; commit to the chosen name before any code._
- **Risk: Publishing N workspace crates multiplies release toil.** Bumping a shared dep means updating versions across all crates and publishing in dependency order. _Mitigation: Use `release-plz` or a custom release script that publishes crates in topological order; document the release procedure. Accept this cost as the price of the modular architecture._
- **Risk: Embedded extension defaults via `include_dir!` go stale relative to the repo.** Users updating via `cargo install` get the version baked into the binary, not the repo's `extensions/` HEAD. _Mitigation: Treat embedded defaults as part of the release artifact; document that user-installed extensions in `$XDG_DATA_HOME/qli/extensions/` always override embedded defaults. `qli ext install-defaults` writes embedded defaults to disk so users can edit them._
- **Risk: Phase 4 SCIP needs symbols and references, not just diagnostics.** _Mitigation: Phase 2I (definition + reference extractor) is now an explicit prerequisite for Phase 4. Without it, Phase 4 ships an empty index. Sequencing enforces this._
- **Risk: 1.5D MCP server is its own protocol surface (JSON-RPC over stdio, tool schemas, lifecycle).** Underscoping it as "wrap the CLI" leads to a broken plugin. _Mitigation: 1.5D is broken into sub-tasks (server skeleton, tool schemas, integration test, install docs) in `tasks.md`._
- **Risk: tree-sitter grammars are C code; cross-compilation needs a C toolchain on every release target.** _Mitigation: `cargo-dist` runners include C toolchains by default; verify on first 1.5A test release before declaring 1.5 done._
