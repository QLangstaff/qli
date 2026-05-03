# Test fixtures

This directory holds language-keyed fixtures used by integration tests across
the workspace. The convention is:

```
tests/fixtures/
  python/
  typescript/
  csharp/
  angular/
  ...
```

Each per-language subdirectory ships small, deliberately-shaped sources used
by analyzer / parser / SCIP-emitter tests. Per-language subdirs are created
**by the phase that introduces the language adapter or analyzer**, not
preemptively here:

- `tests/fixtures/python/` and `tests/fixtures/typescript/` land in
  Phase 2H (TODO/FIXME analyzer fixtures) and Phase 2I (def/ref extractor
  fixtures, including a multi-file case for cross-file references).
- `tests/fixtures/csharp/` lands in Phase 2.5.
- `tests/fixtures/angular/` lands in Phase 2.6.
- Phase 3 (LSP) and Phase 4 (SCIP) reuse the Phase 2H/2I fixtures rather
  than carrying their own.

## Path discipline

Tests reference fixtures relative to the **workspace root**, not relative to
their crate dir, so the layout doesn't change when a fixture is shared
across crates. Use `env!("CARGO_MANIFEST_DIR")` plus a known number of
`..` segments, or compute the workspace root via `cargo metadata`, rather
than guessing the relative path.

## What does NOT go here

- **Per-test-file scratch directories.** Use `tempfile::TempDir` from inside
  the test (see `crates/qli-ext/tests/common/mod.rs`) — those vanish when
  the test ends and never collide between runs.
- **Generated artifacts** (SCIP indexes, cache files, audit logs). Tests
  should produce these into a tempdir and assert against the produced
  artifact, never check it into the repo.
- **Anything ignored by `.gitignore`.** Fixtures are committed source.
