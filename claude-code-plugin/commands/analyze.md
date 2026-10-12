---
description: Analyze source files for TODO/FIXME diagnostics or definitions/references
argument-hint: <paths...> [--analyzer todofixme|defrefs] [--lang <id|auto>] [--format human|jsonl|auto]
allowed-tools: [Bash]
---

Run `qli analyze $ARGUMENTS` via the Bash tool. Print stdout verbatim. Do not interpret, summarize, or reformat the output.

`qli analyze` parses Python, TypeScript/TSX, C#, and Angular template files and runs an analyzer over them:
- `--analyzer todofixme` (default) — flags `TODO`/`FIXME`/`XXX`/`HACK` markers found in comments.
- `--analyzer defrefs` — extracts function/class/variable definitions and call-site references (same-file lexical resolution).

Language is auto-detected from the file extension unless `--lang` is given; format defaults to human on a TTY and jsonl when piped.

If `$ARGUMENTS` is empty, do NOT run `qli analyze` with no paths (it errors). Instead, show `qli analyze --help` output or ask the user which files or directory to analyze.

If stderr contains `warning: ...` lines (e.g. a file that could not be read or parsed), surface them to the user alongside the results.
