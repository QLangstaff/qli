# qli extensions

This directory holds the default extensions shipped with `qli`. Each
subdirectory (`dev/`, `prod/`, `org/`, ...) is a **group**: every script
inside it shares the same guardrails, declared in a `_manifest.toml`
file at the group root.

User-installed extensions live at
`$XDG_DATA_HOME/qli/extensions/<group>/<name>` (default
`~/.local/share/qli/extensions/`). The repo's `extensions/` tree is
embedded into the binary at build time and copied into the data dir by
`qli ext install-defaults`. User-installed files always override embedded
defaults at dispatch time.

## `_manifest.toml` schema (version 1)

```toml
# Required. Must be 1 — older qli builds reject newer values with a
# clear "upgrade qli or downgrade the manifest" error.
schema_version = 1

# Required. One-line summary shown in `qli --help` next to the group name.
description = "Production operations"

# Optional. Printed to stderr before any extension in this group runs.
banner = "PROD — irreversible; verify before proceeding"

# Optional. Map of env vars that must equal the listed value. Missing
# or wrong values fail closed with a "set X=Y to continue" hint.
[requires_env]
QLI_ENV = "prod"

# Optional, default false. If true, dispatcher prompts for confirmation
# before running. Non-TTY runs require `--yes` to proceed.
confirm = true

# Optional. Path receives a start + finish entry per invocation
# (timestamp, command, args, env var *names* — never values).
# `$XDG_STATE_HOME` and `~` are expanded by the dispatcher at use time.
audit_log = "$XDG_STATE_HOME/qli/prod-audit.log"

# Optional. Each entry resolves to an env var injected into the
# extension's process. Resolution happens before exec; any failure
# aborts the run.
[[secrets]]
env      = "OP_TOKEN"                  # var name in extension's env
ref      = "op://Personal/CI/token"    # provider-specific reference
provider = "one_password"              # one_password | env

[[secrets]]
env      = "GITHUB_TOKEN"
ref      = "GITHUB_TOKEN"              # for env: name to read from dispatcher's env
provider = "env"
```

### Field summary

| Field            | Type                          | Required | Default | Notes |
|------------------|-------------------------------|----------|---------|-------|
| `schema_version` | `u32`                         | yes      | —       | Must be `1`. |
| `description`    | `string`                      | yes      | —       | Shown in `qli --help`. |
| `banner`         | `string`                      | no       | none    | Printed to stderr before each run. |
| `requires_env`   | `table<string,string>`        | no       | `{}`    | All listed pairs must match. |
| `confirm`        | `bool`                        | no       | `false` | TTY-only prompt; `--yes` overrides. |
| `audit_log`      | `string` (path)               | no       | none    | `$XDG_STATE_HOME` / `~` expanded at use time. |
| `secrets`        | `array<table>`                | no       | `[]`    | See `SecretSpec` below. |

### `SecretSpec`

| Field      | Type                       | Notes |
|------------|----------------------------|-------|
| `env`      | `string`                   | Env var name set in extension's process. |
| `ref`      | `string`                   | Provider-specific reference. |
| `provider` | `"one_password"` \| `"env"` | Resolution backend. |

Unknown fields are rejected with a parser error naming the offending
key — typos like `audti_log` fail loudly rather than silently doing
nothing.

### Providers

- `one_password` — resolves by spawning `op read <ref>` (1Password CLI).
  The `ref` is a `op://Vault/Item/field` URI. Failure modes:
  - `op` not installed / not on `PATH` → fail closed with an "install
    the 1Password CLI and run `op signin`" hint.
  - `op` exits non-zero (e.g. not signed in, ref doesn't resolve) →
    fail closed surfacing `op`'s stderr and the "run `op signin`" hint.

- `env` — resolves by reading the environment variable named by `ref`
  from the dispatcher's own environment. Useful for plumbing values
  already in CI env (e.g. `GITHUB_TOKEN`) into an extension under a
  different name. Failure modes:
  - the named env var is unset → fail closed.
  - the value is not valid Unicode → fail closed.

Resolution happens up-front, before the child is spawned; any failure
aborts the run before `audit_log` records `start`. Resolved values
never appear in the audit log, in `tracing` output, or in error
messages — only env-var **names** do.

## Schema versioning

The manifest schema is versioned via `schema_version`. A `qli` build
only understands one value (currently `1`). When the schema changes,
the version bumps and older binaries reject newer manifests with
guidance to upgrade. Pre-1.0 the schema may change with minor version
bumps; post-1.0 it becomes API surface.
