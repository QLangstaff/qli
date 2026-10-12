---
description: Index a project's definitions and references into a SCIP index file
argument-hint: [path] [--output <file>] [--lang <id,id,...>]
allowed-tools: [Bash]
---

Run `qli index $ARGUMENTS` via the Bash tool. Print stderr verbatim. Do not interpret, summarize, or reformat the output.

`qli index` walks a project root (default: the current directory, respecting `.gitignore`), extracts definitions and call-site references from every Python, TypeScript/TSX, C#, and Angular template file, resolves references across files, and writes one SCIP index:
- `--output <file>` — where to write the index (default `index.scip` in the current directory).
- `--lang <id,id,...>` — only index the named languages (e.g. `python`, `typescript,tsx`); default is every supported language.

The index itself is a binary protobuf file — do not print it. On success `qli index` writes nothing to stdout and one summary line to stderr: the output path, its size in bytes, and the symbol and reference counts (same-file, cross-file, ambiguous, unresolved).

If `$ARGUMENTS` is empty, run `qli index` as is: it indexes the current directory into `./index.scip`.

If stderr contains `warning: ...` lines (e.g. a file that could not be read or parsed), surface them to the user alongside the summary.
