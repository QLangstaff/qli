# qli

A polyglot code-analysis CLI and any-language extension framework, written in Rust.

> **Status:** pre-alpha. The extension framework, the analysis engine, and all four frontends work end to end: `qli analyze`, `qli lsp`, `qli index`, and `qli mcp`. Analysis is limited to two analyzers and same-name resolution; see [Commands](#commands).

## What it is

`qli` combines two capabilities behind one consistent UX:

1. **Built-in polyglot code analysis.** A Rust engine over [tree-sitter](https://tree-sitter.github.io/) that powers `qli analyze` (CLI), `qli lsp` ([LSP](https://microsoft.github.io/language-server-protocol/) server), and `qli index` ([SCIP](https://github.com/sourcegraph/scip) emitter). One engine, multiple frontends — the [`ruff`](https://github.com/astral-sh/ruff) / [`biome`](https://biomejs.dev/) / [`deno`](https://deno.com/) pattern.
2. **External extension dispatch.** Any-language scripts organized into groups (`dev`, `prod`, `org`, ...), discovered from `$XDG_DATA_HOME/qli/extensions/<group>/<name>` or `qli-<group>-<name>` on `PATH`. The dispatcher enforces per-group guardrails — env requirements, confirmation prompts, audit logs, secrets injection from 1Password — so a bash one-liner gets the same protection as a Python script. Pattern follows `git` / `gh` / `cargo` / `kubectl`.

## Install

```sh
cargo install qli --locked
brew install QLangstaff/qli/qli
curl -LsSf https://github.com/QLangstaff/qli/releases/latest/download/qli-installer.sh | sh
```

Building from source needs Rust 1.88 or later.

## Commands

| Command | What it does |
| --- | --- |
| `qli analyze <paths>` | Runs an analyzer over files and prints diagnostics, or definitions and references with `--analyzer defrefs`. Output is human-readable on a terminal and JSON Lines when piped. |
| `qli lsp` | Serves the same analysis to editors over LSP: diagnostics as you type and same-file go-to-definition. Setup for VS Code and Helix is in [`editors/`](editors/). |
| `qli index [path]` | Writes one [SCIP](https://github.com/scip-code/scip) index for a project, resolving references across files by name. |
| `qli mcp` | Serves `qli_analyze`, `qli_index`, and `qli_ext_list` as [MCP](https://modelcontextprotocol.io) tools over stdio. |
| `qli <group> <name>` | Runs an extension script with its group's guardrails. |
| `qli ext list`, `which`, `install-defaults`, `cache clean` | Inspect extensions and manage the analysis cache. |
| `qli self-update` | Upgrades a curl-installed binary, or prints the upgrade command for cargo and Homebrew installs. |
| `qli completions <shell>` | Prints a shell completion script. |

**Languages:** Python, TypeScript and TSX, C#, and Angular templates (`.html` files inside an `angular.json` workspace).

**Analyzers:** `todofixme` reports `TODO`, `FIXME`, `XXX`, and `HACK` markers in comments. `defrefs` extracts function, class, and variable definitions and call sites.

A [Claude Code plugin](claude-code-plugin/) wraps these commands as slash commands and registers the MCP server.

## License

See [LICENSE](LICENSE).
