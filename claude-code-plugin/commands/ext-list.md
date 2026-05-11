---
description: List qli extensions discovered from $XDG_DATA_HOME and PATH
argument-hint: [--json]
allowed-tools: [Bash]
---

Run `qli ext list $ARGUMENTS` via the Bash tool. Print stdout verbatim. Do not interpret, summarize, or reformat the output.

If `$ARGUMENTS` is empty, run `qli ext list` (no flags).

If stderr contains any `warning: ...` lines (discovery warnings — reserved-name skips, malformed PATH binaries), surface them to the user alongside the listing.
