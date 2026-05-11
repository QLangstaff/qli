---
description: Generate a qli shell completion script for the given shell
argument-hint: <bash|zsh|fish|powershell|elvish>
allowed-tools: [Bash]
---

Run `qli completions $ARGUMENTS` via the Bash tool. Print stdout verbatim (it is the generated completion script).

If `$ARGUMENTS` is empty, tell the user the command needs a shell name (one of `bash`, `zsh`, `fish`, `powershell`, `elvish`) and do not invoke qli.

Do not write the script to a file on the user's behalf — installation paths vary by shell. Print the script and let the user redirect it themselves (e.g., `qli completions zsh > ~/.zsh/_qli`).
