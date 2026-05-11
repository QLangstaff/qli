---
description: Install qli's embedded default extensions to $XDG_DATA_HOME/qli/extensions/
argument-hint: [--force]
allowed-tools: [Bash]
---

Run `qli ext install-defaults $ARGUMENTS` via the Bash tool.

Show the user the installation summary line that qli prints to stderr (`installed defaults to <path>: wrote N, skipped M`).

Then, if the user did not pass `--force` and the summary reports `skipped > 0`, add a brief note that they can re-run with `--force` to overwrite their existing extension files. Do not re-run with `--force` on the user's behalf — overwriting their edits requires explicit user intent.
