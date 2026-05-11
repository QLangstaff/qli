# qli Claude Code plugin

A Claude Code plugin that bundles the qli skill (telling Claude when to invoke qli) and slash commands that wrap qli's user-facing surface.

Targets qli v0.1.1.

## What's in this plugin

- **Skill** ([`skills/qli/SKILL.md`](skills/qli/SKILL.md)) — tells Claude when to invoke qli via Bash, when not to (don't autonomously run user-defined `dev`/`prod`/`org` extensions or the `self-update` stub), and how to interpret qli's output.
- **Slash commands** ([`commands/`](commands/)):
    - `/qli:ext-list` — list discovered extensions
    - `/qli:ext-which <group> <name>` — locate an extension
    - `/qli:ext-install-defaults [--force]` — install qli's embedded defaults to `$XDG_DATA_HOME/qli/extensions/`
    - `/qli:completions <bash|zsh|fish|powershell|elvish>` — generate a shell completion script

Code-analysis slash commands (`/qli:analyze`, `/qli:index`) land alongside the `qli analyze` and `qli index` subcommands in Phase 2G and 4C. MCP server (`qli mcp`) is planned for Phase 5.

## Install

### Local development

```
claude --plugin-dir /path/to/qli/claude-code-plugin
```

The flag loads the plugin from this directory. Edits to `commands/` and `skills/` are picked up via `/reload-plugins` within the session.

### Regular use

Install via Claude Code's `/plugin install` command. Refer to the [Claude Code plugin docs](https://code.claude.com/docs/en/plugins.md) for the current install syntax.

## Prerequisites

The slash commands shell out to a `qli` binary on `PATH`. Install qli via any of:

- `cargo install qli`
- `brew install QLangstaff/qli/qli`
- `curl -LsSf https://github.com/QLangstaff/qli/releases/latest/download/qli-installer.sh | sh`

## Issues

File at <https://github.com/QLangstaff/qli/issues>.

## License

MIT (matches the parent qli repo).
