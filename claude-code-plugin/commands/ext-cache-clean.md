---
description: Evict entries from qli's on-disk analysis cache
argument-hint: [--all | --older-than <days>]
allowed-tools: [Bash]
---

Run `qli ext cache clean $ARGUMENTS` via the Bash tool.

`qli ext cache clean` evicts cached analysis results under `$XDG_CACHE_HOME/qli/analysis`:
- no flags — apply the default policy (drop entries older than 30 days, then enforce the size cap).
- `--all` — remove every cached entry.
- `--older-than <days>` — remove entries older than the given number of days.

Show the user the summary line qli prints to stderr (`cleaned analysis cache: removed N entries, reclaimed M bytes`).

Do not pass `--all` on the user's behalf unless they explicitly asked to clear the entire cache.
