//! Project-wide reference resolution — the pass that runs *after* per-file
//! analysis and links references to definitions in other files.
//!
//! A definition/reference analyzer resolves same-file only and leaves every
//! other [`Reference`](crate::Reference) at `resolved: None`. [`resolve_project`]
//! takes the per-file [`AnalysisResult`]s of a whole project and decides, for
//! each reference, which definition it points at. It is pure data-in/data-out:
//! the inputs are borrowed and never mutated, so cached per-file results stay
//! valid and every frontend (SCIP emitter, LSP, MCP) can run it over them.
//!
//! # Resolution rule
//!
//! For each reference, in order:
//!
//! 1. **Same-file.** `resolved: Some(id)` is looked up among the symbols of the
//!    *same* result (first symbol carrying that id) and reported as
//!    [`Resolution::SameFile`]. An id that names no symbol in its own result is
//!    [`Resolution::Unresolved`] — it is never matched by name instead.
//!    `Symbol::id` is only ever compared within one result: it is not unique
//!    across files (a cached result carries the path it was first analyzed
//!    under), which is why targets are reported as indices, not ids.
//! 2. **Cross-file.** `resolved: None` is matched by exact, case-sensitive name
//!    against the *eligible* definitions of every file in the same
//!    [link family](link_family):
//!    - defined in exactly **one** file → [`Resolution::CrossFile`] to that
//!      file's first such definition in source order (the same
//!      first-definition-wins rule the same-file analyzer uses);
//!    - defined in **two or more** files → [`Resolution::Ambiguous`]. There is
//!      no tie-break: without import information any pick would be a guess;
//!    - defined nowhere → [`Resolution::Unresolved`].
//!
//! A definition is *eligible* as a cross-file target unless its kind is
//! [`SymbolKind::Method`]: a reference is a bare name, and a method is not
//! addressable by bare name from another file.
//!
//! The outcome depends only on the *set* of files, never on their order.
//!
//! # Limits
//!
//! Matching is by name, not by import graph or scope: a unique name match is
//! reported as resolved even if the referencing file never imports the
//! defining one. Names are not qualified, so `a.foo()` and `b.foo()` are the
//! same name `foo`.

use std::collections::HashMap;

use crate::diagnostic::Position;
use crate::result::AnalysisResult;
use crate::source::LanguageId;
use crate::symbol::SymbolKind;

/// One file's contribution to a project-wide resolution pass.
#[derive(Clone, Copy, Debug)]
pub struct ProjectFile<'a> {
    pub language: &'a LanguageId,
    pub result: &'a AnalysisResult,
}

/// A definition, addressed by position in the input: `files[file]`'s
/// `result.symbols[symbol]`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct SymbolLocation {
    pub file: usize,
    pub symbol: usize,
}

/// What one reference resolved to. See the [module docs](self) for the rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Resolution {
    /// Resolved by the per-file analyzer to a definition in the same file.
    SameFile(SymbolLocation),
    /// Resolved by this pass to the only file that defines the name.
    CrossFile(SymbolLocation),
    /// Two or more files define the name; deliberately left unresolved.
    Ambiguous,
    /// No eligible definition carries the name.
    Unresolved,
}

impl Resolution {
    /// The definition this reference points at, if it resolved.
    #[must_use]
    pub fn target(self) -> Option<SymbolLocation> {
        match self {
            Resolution::SameFile(location) | Resolution::CrossFile(location) => Some(location),
            Resolution::Ambiguous | Resolution::Unresolved => None,
        }
    }
}

/// The group of languages whose files can reference each other's definitions.
///
/// Default: a language links only with itself, so its family is its own id.
/// Languages that compile into one module graph share a family:
///
/// | language id | family       |
/// |-------------|--------------|
/// | `tsx`       | `typescript` |
///
/// A new adapter needs a row here only if its files call into another
/// adapter's files.
#[must_use]
pub fn link_family(language: &LanguageId) -> &str {
    match language.as_str() {
        "tsx" => "typescript",
        other => other,
    }
}

/// Resolve every reference of every file. The result is indexed like the
/// input: `resolutions[file][reference]` belongs to
/// `files[file].result.references[reference]`.
#[must_use]
pub fn resolve_project(files: &[ProjectFile<'_>]) -> Vec<Vec<Resolution>> {
    let definitions = Definitions::collect(files);
    files
        .iter()
        .enumerate()
        .map(|(file, project_file)| {
            let symbols = &project_file.result.symbols;
            let family = link_family(project_file.language);
            project_file
                .result
                .references
                .iter()
                .map(|reference| match &reference.resolved {
                    Some(id) => symbols
                        .iter()
                        .position(|symbol| &symbol.id == id)
                        .map_or(Resolution::Unresolved, |symbol| {
                            Resolution::SameFile(SymbolLocation { file, symbol })
                        }),
                    None => definitions.lookup(family, &reference.name),
                })
                .collect()
        })
        .collect()
}

/// Every eligible definition of a project, keyed by link family then name.
struct Definitions<'a> {
    by_family: HashMap<&'a str, HashMap<&'a str, Candidate>>,
}

/// The definitions sharing one name within one link family.
enum Candidate {
    /// Defined in a single file; the location is its first definition there.
    Unique(SymbolLocation, Position),
    /// Defined in more than one file.
    Ambiguous,
}

impl<'a> Definitions<'a> {
    fn collect(files: &[ProjectFile<'a>]) -> Self {
        let mut by_family: HashMap<&str, HashMap<&str, Candidate>> = HashMap::new();
        for (file, project_file) in files.iter().enumerate() {
            let by_name = by_family
                .entry(link_family(project_file.language))
                .or_default();
            for (symbol, definition) in project_file.result.symbols.iter().enumerate() {
                if definition.kind == SymbolKind::Method {
                    continue;
                }
                let location = SymbolLocation { file, symbol };
                let start = definition.range.start;
                by_name
                    .entry(definition.name.as_str())
                    .and_modify(|candidate| match candidate {
                        Candidate::Unique(first, first_start) if first.file == file => {
                            if start < *first_start {
                                *candidate = Candidate::Unique(location, start);
                            }
                        }
                        _ => *candidate = Candidate::Ambiguous,
                    })
                    .or_insert(Candidate::Unique(location, start));
            }
        }
        Self { by_family }
    }

    fn lookup(&self, family: &str, name: &str) -> Resolution {
        match self
            .by_family
            .get(family)
            .and_then(|by_name| by_name.get(name))
        {
            Some(Candidate::Unique(location, _)) => Resolution::CrossFile(*location),
            Some(Candidate::Ambiguous) => Resolution::Ambiguous,
            None => Resolution::Unresolved,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::Range;
    use crate::symbol::{Reference, Symbol};

    fn range(line: u32) -> Range {
        Range::new(Position::new(line, 0), Position::new(line, 3))
    }

    fn definition(file: &str, name: &str, kind: SymbolKind, line: u32) -> Symbol {
        Symbol::new(format!("{file}:{name}#{line}"), name, kind, range(line))
    }

    fn function(file: &str, name: &str, line: u32) -> Symbol {
        definition(file, name, SymbolKind::Function, line)
    }

    fn result(symbols: Vec<Symbol>, references: Vec<Reference>) -> AnalysisResult {
        AnalysisResult {
            symbols,
            references,
            ..Default::default()
        }
    }

    fn call(name: &str) -> Reference {
        Reference::new(name, range(9))
    }

    fn resolve(files: &[(&str, &AnalysisResult)]) -> Vec<Vec<Resolution>> {
        let languages: Vec<LanguageId> = files
            .iter()
            .map(|(language, _)| LanguageId::from(*language))
            .collect();
        let project: Vec<ProjectFile<'_>> = files
            .iter()
            .zip(&languages)
            .map(|((_, result), language)| ProjectFile { language, result })
            .collect();
        resolve_project(&project)
    }

    fn at(file: usize, symbol: usize) -> SymbolLocation {
        SymbolLocation { file, symbol }
    }

    #[test]
    fn unresolved_reference_resolves_to_the_only_defining_file() {
        let lib = result(vec![function("lib", "foo", 0)], vec![]);
        let app = result(vec![], vec![call("foo")]);
        let resolutions = resolve(&[("python", &lib), ("python", &app)]);
        assert_eq!(resolutions[1], [Resolution::CrossFile(at(0, 0))]);
        assert!(resolutions[0].is_empty());
    }

    #[test]
    fn name_defined_in_two_files_is_ambiguous_in_either_order() {
        let a = result(vec![function("a", "helper", 0)], vec![]);
        let b = result(vec![function("b", "helper", 0)], vec![]);
        let app = result(vec![], vec![call("helper")]);
        for files in [
            [("python", &a), ("python", &b), ("python", &app)],
            [("python", &app), ("python", &b), ("python", &a)],
        ] {
            let resolutions = resolve(&files);
            let app_index = files
                .iter()
                .position(|(_, r)| !r.references.is_empty())
                .unwrap();
            assert_eq!(resolutions[app_index], [Resolution::Ambiguous]);
        }
    }

    #[test]
    fn name_defined_nowhere_stays_unresolved() {
        let lib = result(vec![function("lib", "foo", 0)], vec![]);
        let app = result(vec![], vec![call("missing")]);
        let resolutions = resolve(&[("python", &lib), ("python", &app)]);
        assert_eq!(resolutions[1], [Resolution::Unresolved]);
    }

    #[test]
    fn same_file_resolution_is_kept_even_when_another_file_defines_the_name() {
        let local = function("app", "foo", 0);
        let app = result(
            vec![local.clone()],
            vec![call("foo").resolved_to(local.id.clone())],
        );
        let other = result(vec![function("other", "foo", 0)], vec![]);
        let resolutions = resolve(&[("python", &app), ("python", &other)]);
        assert_eq!(resolutions[0], [Resolution::SameFile(at(0, 0))]);
    }

    #[test]
    fn same_file_id_is_never_matched_against_another_files_symbols() {
        // Two results with byte-identical ids: what a content-keyed cache hands
        // back for two copies of one file.
        let copy = || {
            let definition = function("first-path", "foo", 0);
            let id = definition.id.clone();
            result(vec![definition], vec![call("foo").resolved_to(id)])
        };
        let (a, b) = (copy(), copy());
        let resolutions = resolve(&[("python", &a), ("python", &b)]);
        assert_eq!(resolutions[0], [Resolution::SameFile(at(0, 0))]);
        assert_eq!(resolutions[1], [Resolution::SameFile(at(1, 0))]);
    }

    #[test]
    fn dangling_same_file_id_is_unresolved_not_matched_by_name() {
        let lib = result(vec![function("lib", "foo", 0)], vec![]);
        let app = result(vec![], vec![call("foo").resolved_to("gone")]);
        let resolutions = resolve(&[("python", &lib), ("python", &app)]);
        assert_eq!(resolutions[1], [Resolution::Unresolved]);
    }

    #[test]
    fn definitions_in_another_language_are_not_candidates() {
        let py = result(vec![function("lib.py", "foo", 0)], vec![]);
        let ts = result(vec![function("lib.ts", "foo", 0)], vec![]);
        let py_app = result(vec![], vec![call("foo")]);
        let ts_app = result(vec![], vec![call("foo")]);
        let resolutions = resolve(&[
            ("python", &py),
            ("typescript", &ts),
            ("python", &py_app),
            ("typescript", &ts_app),
        ]);
        assert_eq!(resolutions[2], [Resolution::CrossFile(at(0, 0))]);
        assert_eq!(resolutions[3], [Resolution::CrossFile(at(1, 0))]);
    }

    #[test]
    fn languages_in_one_link_family_resolve_across_each_other() {
        let lib = result(vec![function("lib.ts", "foo", 0)], vec![]);
        let view = result(vec![], vec![call("foo")]);
        let resolutions = resolve(&[("typescript", &lib), ("tsx", &view)]);
        assert_eq!(resolutions[1], [Resolution::CrossFile(at(0, 0))]);
        assert_eq!(link_family(&LanguageId::from("csharp")), "csharp");
    }

    #[test]
    fn methods_are_not_cross_file_targets() {
        let lib = result(
            vec![definition("lib", "render", SymbolKind::Method, 0)],
            vec![],
        );
        let app = result(vec![], vec![call("render")]);
        let resolutions = resolve(&[("typescript", &lib), ("typescript", &app)]);
        assert_eq!(resolutions[1], [Resolution::Unresolved]);
    }

    #[test]
    fn several_definitions_in_the_one_defining_file_pick_the_first_in_source_order() {
        // Listed out of source order on purpose.
        let lib = result(
            vec![function("lib", "foo", 7), function("lib", "foo", 2)],
            vec![],
        );
        let app = result(vec![], vec![call("foo")]);
        let resolutions = resolve(&[("python", &lib), ("python", &app)]);
        assert_eq!(resolutions[1], [Resolution::CrossFile(at(0, 1))]);
    }
}
