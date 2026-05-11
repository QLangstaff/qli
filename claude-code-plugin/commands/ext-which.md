---
description: Locate where a specific qli extension lives on disk
argument-hint: <group> <name> [--json]
allowed-tools: [Bash]
---

Run `qli ext which $ARGUMENTS` via the Bash tool. Print stdout verbatim.

If `$ARGUMENTS` is empty or has fewer than two tokens (group + name), tell the user the command requires both a group and an extension name (e.g., `/qli:ext-which dev hello`) and do not invoke qli.

If qli exits non-zero (unknown extension), print qli's stderr message to the user as-is.
