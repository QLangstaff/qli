# qli Claude Code plugin

A Claude Code plugin that bundles the qli skill (telling Claude when to invoke qli), slash commands that wrap qli's user-facing surface, and qli's MCP server (structured tools Claude calls directly).

Targets qli v0.2.0.

## What's in this plugin

- **Skill** ([`skills/qli/SKILL.md`](skills/qli/SKILL.md)) — tells Claude when to invoke qli via Bash, when not to (don't autonomously run user-defined `dev`/`prod`/`org` extensions or the `self-update` stub), and how to interpret qli's output.
- **Slash commands** ([`commands/`](commands/)):
    - `/qli:analyze <paths...> [--analyzer todofixme|defrefs] [--lang <id|auto>] [--format human|jsonl|auto]` — analyze Python, TypeScript, C#, and Angular template files for TODO/FIXME diagnostics or definitions/references
    - `/qli:index [path] [--output <file>] [--lang <id,id,...>]` — index a project's Python, TypeScript, C#, and Angular template definitions and references into a SCIP index file
    - `/qli:ext-cache-clean [--all | --older-than <days>]` — evict entries from qli's analysis cache
    - `/qli:ext-list` — list discovered extensions
    - `/qli:ext-which <group> <name>` — locate an extension
    - `/qli:ext-install-defaults [--force]` — install qli's embedded defaults to `$XDG_DATA_HOME/qli/extensions/`
    - `/qli:completions <bash|zsh|fish|powershell|elvish>` — generate a shell completion script
- **LSP (no slash command)** — qli also speaks the Language Server Protocol via `qli lsp --stdio` for editor integration (diagnostics and go-to-definition). The skill mentions it so Claude can point users at the editor setup in [`editors/`](https://github.com/QLangstaff/qli/tree/main/editors); nothing in this plugin runs it.

- **MCP server** ([`.mcp.json`](.mcp.json)) — starts `qli mcp` when the plugin is enabled and gives Claude three tools. See [MCP tools](#mcp-tools).

## MCP tools

[`.mcp.json`](.mcp.json) registers one stdio server, `qli`, whose command is `qli mcp`. Claude Code starts it when the plugin is enabled; nothing else needs configuring. Each tool calls the same engine as the CLI command it mirrors and returns the result as structured JSON. Pass absolute paths: a relative path resolves against the server's working directory, which the MCP client chooses.

| Tool | What it does | Parameters | Returns |
| :- | :- | :- | :- |
| `qli_analyze` | Runs an analyzer over Python, TypeScript/TSX, C#, and Angular template files. Same engine as `qli analyze`. | `paths` (array of files or directories; required), `lang` (language id or `auto`; default `auto`), `analyzer` (`todofixme` or `defrefs`; default `todofixme`) | `records`: one object per finding, identical to the lines of `qli analyze --format jsonl` |
| `qli_index` | Indexes a project's definitions and references into one SCIP index file, overwriting an existing one. Same engine as `qli index`. | `path` (project root; required), `output` (index file; default `index.scip`), `lang` (array of language ids; default every supported language) | `path` (absolute path of the written file), `bytes`, `files`, `symbols`, `references`, `same_file`, `cross_file`, `ambiguous`, `unresolved` — the numbers in the `qli index` summary line |
| `qli_ext_list` | Lists the discovered extensions. It never runs one. Same data as `qli ext list --json`. | none | `extensions`: objects with `group`, `extension`, `origin`, `path` |

A request qli rejects — an unknown analyzer, an unknown language, a path that does not exist — comes back as a tool error carrying the message the CLI prints after `error: `; the server keeps running.

### MCP tools versus slash commands

- **Claude uses the MCP tools on its own initiative**, when a task needs what they return: TODO/FIXME markers or definitions and references to reason about, a SCIP index to build, the list of extensions. The result is structured JSON that Claude reads directly, with no Bash call and no output parsing.
- **You run the slash commands**, when you want qli's own output shown verbatim. `/qli:analyze`, `/qli:index`, and `/qli:ext-list` shell out to the CLI and print what it prints (including the human format and `--format`).
- **Only the CLI covers the rest**: `ext which`, `ext install-defaults`, `ext cache clean`, `completions`, and `self-update` have no MCP tool. Use their slash commands or Bash.

To use the server without this plugin: `claude mcp add qli -- qli mcp`.

## Install

### Regular use

From any Claude Code session:

```
/plugin marketplace add QLangstaff/qli
/plugin install qli@qli-plugins
```

The first command registers this repo's marketplace (`qli-plugins`); the second installs the `qli` plugin from it. The marketplace name (`qli-plugins`) is the install suffix, not the GitHub repo slug.

### Local development

```
claude --plugin-dir /path/to/qli/claude-code-plugin
```

The flag loads the plugin from this directory. Edits to `commands/` and `skills/` are picked up via `/reload-plugins` within the session.

## Prerequisites

The slash commands shell out to a `qli` binary on `PATH`, and the MCP server is that same binary run as `qli mcp`. Install qli via any of:

- `cargo install qli`
- `brew install QLangstaff/qli/qli`
- `curl -LsSf https://github.com/QLangstaff/qli/releases/latest/download/qli-installer.sh | sh`

## Issues

File at <https://github.com/QLangstaff/qli/issues>.

## License

MIT (matches the parent qli repo).
