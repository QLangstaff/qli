# The `scip-qli` symbol scheme

How `qli index` names things in a [SCIP](https://github.com/scip-code/scip)
index. The grammar is SCIP's own
([`scip.proto`](https://github.com/scip-code/scip/blob/main/scip.proto),
"Symbol"); this document fixes the choices SCIP leaves to the indexer. The
implementation is [`src/symbol.rs`](src/symbol.rs) and
[`src/index.rs`](src/index.rs).

## Global symbols

Every definition gets a global symbol:

```
scip-qli <manager> <package-name> <version> <namespace>/…<namespace>/<definition>
```

| Field | Value |
|---|---|
| `<scheme>` | `scip-qli`, always. |
| `<manager> <package-name> <version>` | The project's package identity (`qli_scip::Package`). An empty field is SCIP's placeholder `.`. `qli index` does not read package manifests, so today every symbol carries `. . .` — the "no public package" form. A space inside a field is doubled, per the grammar. |
| `<namespace>/…` | The file, from its path relative to the project root. See [Files](#files). |
| `<definition>` | The definition's name with a suffix chosen by its kind. See [Definitions](#definitions). |

Names that are not a SCIP simple identifier (`[A-Za-z0-9_+$-]+`) are
backtick-escaped, with a literal backtick doubled. Formatting and escaping are
done by the `scip` crate's `format_symbol`, and every symbol qli emits parses
back with its `parse_symbol`.

### Files

The mapping is chosen by the file's **language id** (the id its `qli-lang`
adapter reports) and uses nothing but the project-relative path, so a new
language adapter is covered without any change here.

| Language id | Rule | Example |
|---|---|---|
| *any language not listed below* (default) | One namespace per path segment; the file name is kept whole, extension included. | `src/app.ts` → ``src/`app.ts`/`` |
| `python` | As the default, but a trailing `.py` is dropped from the file name, so the namespaces spell the dotted module path. `__init__.py` is not special-cased. | `pkg/util.py` → `pkg/util/` (module `pkg.util`); `pkg/__init__.py` → `pkg/__init__/` |

TypeScript, TSX, C#, and Angular files all take the default rule: their module
identity *is* the file path. A language gets its own row (a `ModuleRule` in
`symbol.rs`) only when its module name differs from its file name.

Both rules are injective — two different files never produce the same
namespace chain — because the Python rule strips exactly the lowercase `.py`
suffix that selects the Python adapter and nothing else (`A.PY` stays
`` `A.PY`/ ``).

### Definitions

`qli_core::Symbol` carries a name, a kind, and the range of the name. It does
not carry its enclosing scope, so every definition hangs directly off its
file's namespaces.

| `SymbolKind` | Descriptor | SCIP `SymbolInformation.kind` |
|---|---|---|
| `Function` | method: `name().` | `Function` |
| `Method` | method: `name().` | `Method` |
| `Class` | type: `Name#` | `Class` |
| `Variable` | term: `name.` | `Variable` |

Several definitions in one file can share a name and descriptor:

- **Callables** (`Function`, `Method`) are distinct things — two classes each
  with a `render` method — so they are told apart with SCIP's method
  disambiguator, numbered in source order: `render().`, `render(+1).`,
  `render(+2).`. Functions and methods share one numbering because they share
  the descriptor form.
- **Classes and variables** repeating a name are treated as one symbol with
  several definition occurrences (the usual cause is reassignment:
  `x = 1` … `x = 2`).

## Unresolved references: document-local symbols

A reference that resolves to no definition — the name is defined nowhere, or it
is [ambiguous](#which-symbol-a-reference-gets) — is still recorded. SCIP has no
"unresolved" marker, so each distinct unresolved *name* in a document gets a
document-local symbol, numbered in order of first appearance:

```
local 0
local 1
```

Every unresolved reference to that name in that document is an occurrence of
it, and the document carries a `SymbolInformation` for it whose `display_name`
is the referenced name. Local symbols are scoped to one document, so two
files' unresolved `bar` are never linked to each other.

This is the only use of `local` symbols. Definitions are never local: nothing
in a `Symbol` proves a definition is invisible outside its file, and a local
symbol cannot be referenced from another document.

## Which symbol a reference gets

Decided by `qli_core::resolve::resolve_project`, which runs over the per-file
results of the whole project on every `qli index` (it is never stored in the
analysis cache):

1. A reference the per-file analyzer resolved (`resolved: Some(id)`) gets the
   symbol of that same-file definition.
2. Otherwise the reference's name is matched, exactly and case-sensitively,
   against the non-method definitions of all indexed files in the same *link
   family* — the file's own language, plus languages that share a module graph
   (`tsx` links with `typescript`; every other language links only with
   itself):
   - defined in exactly one file → that file's first definition of the name;
   - defined in two or more files → **ambiguous**: unresolved, no winner is
     picked;
   - defined in no file → unresolved.

Methods are excluded in step 2 because a reference is a bare name and a method
is not reachable by bare name from another file. Matching is by name only;
imports are not consulted.

## From the precursor ids

Before this scheme, `DefRefs` gave each `Symbol` a loose id of the form
`<scheme>:<package>:<file>:<symbol-path>`, concretely
`qli-local:<path>:<name>#<line>:<column>`. It still does, and that id is
**not** parsed to build a SCIP symbol:

| Precursor part | Formal scheme | Source |
|---|---|---|
| `<scheme>` (`qli-local`) | `scip-qli` | constant |
| `<package>` (absent) | `<manager> <package-name> <version>`, `. . .` when unknown | `qli_scip::Package` |
| `<file>` (the path as analyzed) | namespace descriptors | the document's project-relative path at index time — **not** the path inside the id |
| `<symbol-path>` (`<name>#<line>:<column>`) | one descriptor: name + kind suffix (+ disambiguator) | `Symbol::name`, `Symbol::kind`, source order |

The path inside a precursor id is whatever path the file had when its result
was first computed. Analysis results are cached by content, so the same result
is served for the same bytes under another spelling of the path (`./src/a.py`
vs an absolute path) and for a byte-identical copy elsewhere in the tree; the
embedded path is then wrong for the file being indexed. A precursor id is
therefore used for exactly one thing: as a key *within its own result*, to
find which of that file's definitions a same-file reference points at.

## Positions

Not part of the symbol grammar, but fixed by the same contract:

- Ranges are zero-based and half-open, as in `qli_core::Range`.
- `Document.position_encoding` is `UTF8CodeUnitOffsetFromLineStart`.
  `qli_core::Position::column` is a byte offset within the line (it is
  tree-sitter's column), which is that encoding exactly; no conversion is
  applied. On a line with non-ASCII text before a symbol, the column is larger
  than the character count.
- Each occurrence carries its range twice, kept equal: the typed
  `single_line_range` / `multi_line_range` that `scip.proto` now prefers, and
  the deprecated packed `range` (`[line, start, end]` on one line, otherwise
  `[startLine, start, endLine, end]`) that is all consumers predating the typed fields
  read.
