# Editor integration

`qli lsp` is a Language Server Protocol server over the same engine as `qli analyze`. An editor spawns it and gets, for Python (`.py`), TypeScript (`.ts`, `.tsx`), C# (`.cs`), and Angular template (`.html` inside an `angular.json` workspace) files:

- **Diagnostics** — `TODO` / `FIXME` / `XXX` / `HACK` markers in comments, published as you type.
- **Go to definition** — a call resolves to its definition in the same file. Cross-file resolution is not implemented.

Files with any other extension are ignored: the server publishes nothing for them and reports no error.

## Prerequisite

A `qli` binary on `PATH`, at a version that has the `lsp` subcommand (`qli lsp --help`). Install qli by any method in the [repository README](https://github.com/QLangstaff/qli#readme).

## VS Code

Distribution is install-from-`.vsix`; the extension is not on the Marketplace.

Build the package (needs Node.js and npm):

```sh
cd editors/vscode
npm install
npm run package          # writes qli-vscode-<version>.vsix
```

Install it:

```sh
code --install-extension qli-vscode-<version>.vsix
```

Or, in VS Code: **Extensions** view → **…** menu → **Install from VSIX…**.

The extension activates on Python, TypeScript, and TypeScript React files and runs `qli lsp --stdio`.

| Setting | Default | Purpose |
| --- | --- | --- |
| `qli.path` | `qli` | Path to the qli binary, if it is not on the `PATH` VS Code sees. |
| `qli.trace.server` | `off` | `messages` or `verbose` logs the LSP traffic to the **qli** output channel. |

Server logs (stderr) appear in **Output** → **qli**.

## Helix

Merge [`helix/languages.toml`](helix/languages.toml) into `~/.config/helix/languages.toml`, or into `.helix/languages.toml` at a project root.

A `language-servers` list **replaces** Helix's default list for that language. The snippet enables only qli; to keep the servers you already use, list them first and put `"qli"` last:

```toml
[[language]]
name = "python"
language-servers = ["pylsp", "qli"]
```

`hx --health python` (or `typescript`, `tsx`) prints the servers configured for a language and whether each binary was found. Helix sends go-to-definition to the first listed server that supports it and merges diagnostics from all of them.

## Other editors

Any LSP client works. Point it at one of:

| Command | Transport | Logs |
| --- | --- | --- |
| `qli lsp --stdio` (the default) | stdin/stdout | stderr |
| `qli lsp --tcp <port>` | one client on `127.0.0.1:<port>` | `$XDG_STATE_HOME/qli/lsp.log` (default `~/.local/state/qli/lsp.log`) |

In stdio mode stdout carries LSP frames and nothing else. In tcp mode stdout and stderr both stay silent. `-v` / `-vv` raise the log level in either mode. Columns are UTF-16 code units (the LSP default); document sync is full-text.

## Latency notes

Measured, not estimated. The measurement is the ignored test [`crates/qli-lsp/tests/latency.rs`](../crates/qli-lsp/tests/latency.rs):

```sh
cargo test -p qli-lsp --release --test latency -- --ignored --nocapture
```

It sends `textDocument/didChange` through the real `tower-lsp` service (JSON-RPC decode → handler → analysis → `publishDiagnostics`) and stops the clock when the diagnostics arrive, then times each engine stage on the same document.

**Setup:** generated Python (one function and one marker comment per 5 lines); release build; Apple M4 Max; 200 `didChange` samples per case, 50 runs per stage. Recorded 2026-10-12.

**Not included:** transport I/O (the test is in-process), and the on-disk cache — the test's persisted tier is in memory, whereas `qli lsp` does one cache-file lookup per analyzer on every changed `didChange`.

### 1,001 lines (21,791 bytes, 200 markers)

| `didChange` case | mean | median | p95 | max |
| --- | --- | --- | --- | --- |
| Content changed — parse + both analyzers run | 4.92 ms | 4.84 ms | 5.31 ms | 5.95 ms |
| Content unchanged — answered by the in-memory cache | 0.17 ms | 0.17 ms | 0.18 ms | 0.19 ms |

| Stage | mean |
| --- | --- |
| tree-sitter parse (full, not incremental) | 1.97 ms |
| `todofixme` analyzer | 0.49 ms |
| `defrefs` analyzer | 2.25 ms |

### 5,001 lines (110,591 bytes, 1,000 markers)

| `didChange` case | mean | median | p95 | max |
| --- | --- | --- | --- | --- |
| Content changed — parse + both analyzers run | 17.58 ms | 17.57 ms | 17.99 ms | 20.98 ms |
| Content unchanged — answered by the in-memory cache | 0.79 ms | 0.79 ms | 0.82 ms | 0.96 ms |

| Stage | mean |
| --- | --- |
| tree-sitter parse (full, not incremental) | 9.77 ms |
| `todofixme` analyzer | 2.20 ms |
| `defrefs` analyzer | 5.09 ms |

### What the numbers say

- **1k lines: ~5 ms per edit.** The server analyzes on every `didChange` with no debounce; at this size that is well inside a keystroke interval.
- **5k lines: ~18 ms per edit, and the cost is linear.** For 5× the input every stage costs about 5× or less; the full tree-sitter re-parse is over half of it.
- **Unchanged content skips analysis.** The remaining 0.2–0.8 ms is hashing the document and re-encoding the diagnostics, both proportional to size.
- **Disk is touched on open and save only.** The content at `didOpen` and `didSave` is written to the cache shared with `qli analyze`; intermediate edits stay in memory. A reopened, unmodified file in a fresh server is answered from disk without running an analyzer.
