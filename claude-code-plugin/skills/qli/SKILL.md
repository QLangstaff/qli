---
name: qli
description: "qli is a polyglot CLI: code analysis (`qli analyze` for TODO/FIXME diagnostics and definitions/references, `qli index` for a whole-project SCIP index, over Python & TypeScript) plus extension dispatch (run user-defined `dev`/`prod`/`org` scripts). Use when the user references `qli`, wants to analyze or index code with qli, or asks about discovering, locating, or setting up their qli extensions. Do NOT autonomously run user-defined extensions (`qli dev|prod|org <name>`) — the user invokes those directly."
---

# qli

`qli` is a single Rust binary that ships four capabilities behind one CLI:

1. **Extension dispatch.** Any-language scripts (bash, python, ...) organized into groups (`dev`, `prod`, `org`, ...) discovered from `$XDG_DATA_HOME/qli/extensions/<group>/<name>` and from `qli-<group>-<name>` binaries on `PATH`. Each group has a `_manifest.toml` declaring guardrails (env requirement, confirmation prompt, audit log, secrets injection). The dispatcher enforces guardrails before exec'ing the script.
2. **Polyglot code analysis.** `qli analyze` parses Python, TypeScript/TSX, C#, and Angular templates via tree-sitter and runs an analyzer over the parse tree: `todofixme` (TODO/FIXME/XXX/HACK markers in comments) or `defrefs` (definition + reference extraction). Results render as colored human output on a TTY or JSON Lines when piped, cached by content hash. `qli index` runs `defrefs` over a whole project, resolves references across files, and writes one [SCIP](https://github.com/scip-code/scip) index file.
3. **Editor integration (LSP).** `qli lsp --stdio` serves the same engine to editors over the Language Server Protocol: `todofixme` diagnostics as you type and same-file go-to-definition from `defrefs`, for Python, TypeScript/TSX, C#, and Angular templates. An editor spawns it (see [`editors/`](https://github.com/QLangstaff/qli/tree/main/editors) for VS Code and Helix setup); it is informational here — there is no slash command, and it is not something to run via Bash, because it blocks reading LSP frames from stdin.
4. **MCP server.** `qli mcp` serves the same engines to AI assistants over the Model Context Protocol, as three tools: `qli_analyze` (the records `qli analyze --format jsonl` emits), `qli_index` (writes a SCIP index; returns its path, size, and symbol and reference counts), and `qli_ext_list` (the entries `qli ext list --json` prints). This plugin's `.mcp.json` starts it for you — do not run `qli mcp` via Bash, because it blocks reading MCP frames from stdin.

## When to use the MCP tools

When the `qli_analyze`, `qli_index`, and `qli_ext_list` tools are available, prefer them over Bash whenever you need the result yourself — to reason about findings, count them, or act on them. They return structured JSON, so there is nothing to parse. Pass absolute paths — a relative path resolves against the server's working directory, which may not be yours.

- **Findings or definitions/references you will work with** → `qli_analyze` with `paths`, plus `analyzer: "defrefs"` for definitions and references (default `todofixme`) and `lang` to force a language.
- **Build a SCIP index** → `qli_index` with `path`, plus `output` and `lang` when the user names them. It overwrites the output file.
- **Which extensions exist** → `qli_ext_list`.

A tool error carries the message the CLI prints after `error: ` (unknown analyzer, unknown language, missing path): fix the argument and call again.

Use Bash (below) when the user wants qli's own output shown verbatim, wants the human format, or asks for something no tool covers (`ext which`, `ext install-defaults`, `ext cache clean`, `completions`).

## When to invoke `qli` via Bash

Run `qli` via the Bash tool when the user asks any of these:

- **"Analyze these files / find TODOs / list the definitions"** → `qli analyze <paths>`. Add `--analyzer defrefs` for definitions/references (default is `todofixme`). Auto-detects language and format; pass `--format jsonl` when you need to parse the output.
- **"Index this project / build a SCIP index"** → `qli index [path]` (default: current directory → `./index.scip`). Add `--output <file>` to choose the file, `--lang <id,id,...>` to limit languages. The index is binary; inspect it with `scip print --json <file>` if the SCIP CLI is installed.
- **"Clean qli's analysis cache"** → `qli ext cache clean` (or `--all` / `--older-than <days>`).
- **"What qli extensions do I have?"** → `qli ext list` (or `qli ext list --json` if downstream parsing is implied).
- **"Where is my `<group> <name>` extension?" / "Locate ..."** → `qli ext which <group> <name>`.
- **"Set up qli defaults" / "Install qli's default extensions"** → `qli ext install-defaults` (the user can re-run with `--force` to overwrite their edits).
- **"Generate qli shell completions for <shell>"** → `qli completions <shell>`. Pipe target depends on the user's shell.

These also have dedicated slash commands: `/qli:analyze`, `/qli:index`, `/qli:ext-cache-clean`, `/qli:ext-list`, `/qli:ext-which`, `/qli:ext-install-defaults`, `/qli:completions`.

## When NOT to invoke `qli`

- **Do not run user-defined extensions.** `qli dev <name>`, `qli prod <name>`, `qli org <name>` invoke the user's own scripts whose semantics you cannot predict. Even `dev` scripts may have side effects (writes to remote services, file deletions, etc.). The user invokes these directly.
- **Do not autonomously run `qli self-update`.** It upgrades the user's installed binary — re-execs the installer on the curl-installed path, prints the upgrade command for cargo/brew. Shell it via Bash only when the user explicitly asks to upgrade qli.

## Output discipline

- `qli analyze` writes findings to **stdout** (colored human lines on a TTY, one JSON object per finding when piped). Warnings about unreadable/unparseable files go to stderr. Exit 0 even when findings exist — findings are data, not a command failure.
- `qli index` writes the index to the `--output` **file** (default `./index.scip`), nothing to stdout, and one summary line to **stderr** ("indexed N files to ...: B bytes, S symbols, R references (...)"). A reference that matches no definition, or a name defined in several files, is counted as unresolved/ambiguous — that is data, not a failure. Exit 1 if the root is not a directory, a `--lang` id is unknown, or the file cannot be written.
- `qli ext list` writes data to **stdout** (tab-separated columns or JSON with `--json`).
- `qli ext which` writes the path to stdout, or JSON with `--json`. Unknown extension exits 1 with a stderr error.
- `qli ext install-defaults` writes status to **stderr** ("installed defaults to ...: wrote N, skipped M").
- `qli self-update` writes its status/guidance to **stderr** (JSON with `--json`); exits 0 when it upgraded or printed guidance, 1 if detection or the upgrade failed.
- **Discovery warnings** (`warning: ...`) can appear on stderr at startup for *any* `qli` invocation except `qli lsp` (which skips extension discovery) when discovery finds malformed extensions, reserved names, or non-executable `qli-*` binaries on `PATH`. If stderr contains warnings alongside the normal output, surface them to the user too.

Print `qli`'s output verbatim. Do not translate paths, summarize counts, or reformat JSON — the user is asking what `qli` says, not what you think it means.

## Where qli lives

- **Binary**: typically `~/.cargo/bin/qli` (cargo install), `/opt/homebrew/bin/qli` (brew), or `~/.local/bin/qli` (curl installer). `which qli` to confirm.
- **User extensions**: `$XDG_DATA_HOME/qli/extensions/<group>/` (default `~/.local/share/qli/extensions/<group>/`).
- **Audit log** (prod group default): `$XDG_STATE_HOME/qli/prod-audit.log` (default `~/.local/state/qli/prod-audit.log`).
- **Embedded defaults**: compiled into the binary; `qli ext install-defaults` materializes them.

## Version

This skill targets `qli v0.3.0`. If `qli --version` reports a substantially different version (e.g., `0.4.x`+), check `qli --help` for the actual subcommand surface before invoking commands described here.
