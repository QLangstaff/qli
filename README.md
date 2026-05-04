# qli

A polyglot code-analysis CLI and any-language extension framework, written in Rust.

> **Status:** pre-alpha. **Phase 1 complete** — extension framework end-to-end. Built-in: workspace scaffolding, CLI core (`--version`, `--help`, `completions <shell>`, global `--verbose`/`--quiet`/`--color`/`--yes`), strict-XDG paths, manifest schema, extension discovery (XDG + PATH + embedded defaults), dispatcher with guardrails (banner / `requires_env` / confirm / secrets injection via 1Password or env / JSONL audit log), default extensions (`dev`, `prod`, `org`) embedded via `include_dir!`, meta commands (`qli ext list/which/install-defaults`, `qli self-update` stub), typed error rendering with clap suggestions, CI matrix (Linux + macOS, fmt/clippy/test/build/audit). No code analysis yet — that's Phase 2. Crate names on crates.io are reserved under the `qli-*` prefix.

## What it is

`qli` combines two capabilities behind one consistent UX:

1. **Built-in polyglot code analysis.** A Rust engine over [tree-sitter](https://tree-sitter.github.io/) that powers `qli analyze` (CLI), `qli lsp` ([LSP](https://microsoft.github.io/language-server-protocol/) server), and `qli index` ([SCIP](https://github.com/sourcegraph/scip) emitter). One engine, multiple frontends — the [`ruff`](https://github.com/astral-sh/ruff) / [`biome`](https://biomejs.dev/) / [`deno`](https://deno.com/) pattern.
2. **External extension dispatch.** Any-language scripts organized into groups (`dev`, `prod`, `org`, ...), discovered from `$XDG_DATA_HOME/qli/extensions/<group>/<name>` or `qli-<group>-<name>` on `PATH`. The dispatcher enforces per-group guardrails — env requirements, confirmation prompts, audit logs, secrets injection from 1Password — so a bash one-liner gets the same protection as a Python script. Pattern follows `git` / `gh` / `cargo` / `kubectl`.

## License

See [LICENSE](LICENSE).
