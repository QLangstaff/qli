# Test fixtures

This directory holds language-keyed fixtures used by integration tests across
the workspace. The convention is:

```
tests/fixtures/
  python/
  typescript/
  csharp/
  angular/
  multifile/
  ...
```

Each per-language subdirectory ships small, deliberately-shaped sources used
by analyzer / parser / SCIP-emitter tests. Per-language subdirs are created
**by the phase that introduces the language adapter or analyzer**, not
preemptively here:

- `tests/fixtures/python/` and `tests/fixtures/typescript/` landed in
  Phase 2H/2I. Each ships a `sample.{py,ts}` exercising both analyzers: the
  four TODO/FIXME markers (in line and block comments, plus a string-literal
  `TODO` that must NOT be flagged) and a set of definitions/references
  (`foo` defined and called → resolved; `bar` called but undefined →
  unresolved). Cross-file resolution is out of scope until Phase 4.
- `tests/fixtures/csharp/` landed in Phase 2.5. Its `sample.cs` has the same
  shape as the Phase 2H/2I samples (the `FIXME` in a block comment, the `XXX`
  in a `///` doc comment), with `Foo`/`Bar` called as methods of `Widget` and
  a member-access call (`new Widget().Render()`) that is not a reference.
- `tests/fixtures/angular/` landed in Phase 2.6. It is a minimal Angular
  workspace: the `angular.json` at its root is the project marker that makes
  `--lang auto` treat `.html` files beneath it as Angular templates.
  `src/app/sample.component.html` exercises both analyzers: the four markers
  in `<!-- -->` comments (plus an element-text `TODO` that must NOT be
  flagged), every template construct (`*ngIf`/`*ngFor`, `[prop]`, `(event)`,
  `[(ngModel)]`, `{{ }}`, `@let`/`@if`/`@for`), and `format` bound by `@let`
  and called → resolved, while component-class calls such as `retry` stay
  unresolved.
- `tests/fixtures/html/plain.html` is the counter-case: a `.html` file with
  no `angular.json` above it, which `--lang auto` must skip and
  `--lang angular` must still analyze.
- `tests/fixtures/multifile/` landed in Phase 4 (SCIP). Unlike the
  single-file samples it is one small *project* — `python/` and `typescript/`
  halves — shaped for cross-file resolution: `foo` and `Gadget` defined in
  `lib.*` and called from `app.*` (and from `view.tsx`, across the
  `.ts`/`.tsx` boundary); `helper` defined in two Python files (ambiguous →
  unresolved); `missing` defined nowhere; and a call after non-ASCII text on
  one line of `python/app.py` (byte columns). `foo` exists in both halves on
  purpose: a call must resolve within its own language.
- Phase 3 (LSP) reuses the Phase 2H/2I fixtures rather than carrying its own.

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
