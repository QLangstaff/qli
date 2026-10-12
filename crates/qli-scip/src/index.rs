//! Conversion of per-file `qli-core` analysis results into a SCIP [`Index`].
//!
//! [`build_index`] is the whole pipeline after per-file analysis: it runs the
//! project-wide [`resolve_project`] pass over the borrowed results, names every
//! definition with the [`scip-qli` scheme](crate::symbol), and lays the
//! definitions and references out as SCIP `Document`s and `Occurrence`s.

use std::collections::HashMap;

use qli_core::resolve::{resolve_project, ProjectFile, Resolution};
use qli_core::{AnalysisResult, LanguageId, Range, SymbolKind};
use scip::types::occurrence::Typed_range;
use scip::types::symbol_information::Kind;
use scip::types::{
    Document, Index, Metadata, MultiLineRange, Occurrence, PositionEncoding, SingleLineRange,
    SymbolInformation, SymbolRole, TextEncoding, ToolInfo,
};

use crate::symbol::{document_symbols, local_symbol, Package};

/// One analyzed file to index.
#[derive(Clone, Copy, Debug)]
pub struct IndexDocument<'a> {
    /// Path relative to the project root, `/`-separated, no leading `./`.
    pub relative_path: &'a str,
    pub language: &'a LanguageId,
    pub result: &'a AnalysisResult,
}

/// Index-wide metadata.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Project {
    /// `file://` URI of the project root (see [`file_uri`]).
    pub root_uri: String,
    /// Version of the tool writing the index.
    pub tool_version: String,
    /// Command-line arguments the tool was invoked with.
    pub tool_arguments: Vec<String>,
    /// Package identity for emitted symbols; the default is "no package".
    pub package: Package,
}

/// Counts over a built index.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct IndexStats {
    pub documents: usize,
    /// Definitions (one per `qli-core` symbol).
    pub symbols: usize,
    /// All references, resolved or not.
    pub references: usize,
    /// References resolved to a definition in the same file.
    pub same_file: usize,
    /// References resolved to a definition in another file.
    pub cross_file: usize,
    /// References left unresolved because several files define the name.
    pub ambiguous: usize,
    /// References no definition matches.
    pub unresolved: usize,
}

/// Build a SCIP index over `documents`.
///
/// Documents are emitted sorted by `relative_path`, so the same set of files
/// produces the same bytes whatever order they are passed in. Unresolved and
/// ambiguous references are still recorded: each becomes an occurrence of a
/// document-local symbol (see `SCHEME.md`).
#[must_use]
pub fn build_index(project: &Project, documents: &[IndexDocument<'_>]) -> (Index, IndexStats) {
    let files: Vec<ProjectFile<'_>> = documents
        .iter()
        .map(|document| ProjectFile {
            language: document.language,
            result: document.result,
        })
        .collect();
    let resolutions = resolve_project(&files);
    let symbols: Vec<Vec<String>> = documents
        .iter()
        .map(|document| {
            document_symbols(
                &project.package,
                document.language,
                document.relative_path,
                &document.result.symbols,
            )
        })
        .collect();

    let mut stats = IndexStats {
        documents: documents.len(),
        ..Default::default()
    };
    let mut order: Vec<usize> = (0..documents.len()).collect();
    order.sort_by_key(|&index| documents[index].relative_path);
    let scip_documents = order
        .into_iter()
        .map(|index| {
            build_document(
                &documents[index],
                &symbols[index],
                &resolutions[index],
                &symbols,
                &mut stats,
            )
        })
        .collect();

    let index = Index {
        metadata: Some(Metadata {
            tool_info: Some(ToolInfo {
                name: "qli".to_owned(),
                version: project.tool_version.clone(),
                arguments: project.tool_arguments.clone(),
                ..Default::default()
            })
            .into(),
            project_root: project.root_uri.clone(),
            text_document_encoding: TextEncoding::UTF8.into(),
            ..Default::default()
        })
        .into(),
        documents: scip_documents,
        ..Default::default()
    };
    (index, stats)
}

fn build_document(
    document: &IndexDocument<'_>,
    own_symbols: &[String],
    resolutions: &[Resolution],
    all_symbols: &[Vec<String>],
    stats: &mut IndexStats,
) -> Document {
    let result = document.result;
    let mut occurrences = Vec::with_capacity(result.symbols.len() + result.references.len());
    let mut information: Vec<SymbolInformation> = Vec::new();

    for (definition, symbol) in result.symbols.iter().zip(own_symbols) {
        stats.symbols += 1;
        occurrences.push(occurrence(
            definition.range,
            symbol.clone(),
            SymbolRole::Definition as i32,
        ));
        // Definitions sharing a symbol (a reassigned variable) are described
        // once, by the first.
        if !information.iter().any(|info| &info.symbol == symbol) {
            information.push(SymbolInformation {
                symbol: symbol.clone(),
                kind: kind(definition.kind).into(),
                display_name: definition.name.clone(),
                ..Default::default()
            });
        }
    }

    // name -> local symbol, numbered in order of first appearance.
    let mut unresolved: HashMap<&str, String> = HashMap::new();
    for (reference, resolution) in result.references.iter().zip(resolutions) {
        stats.references += 1;
        match resolution {
            Resolution::SameFile(_) => stats.same_file += 1,
            Resolution::CrossFile(_) => stats.cross_file += 1,
            Resolution::Ambiguous => stats.ambiguous += 1,
            Resolution::Unresolved => stats.unresolved += 1,
        }
        let symbol = if let Some(target) = resolution.target() {
            all_symbols[target.file][target.symbol].clone()
        } else {
            let next = unresolved.len();
            unresolved
                .entry(reference.name.as_str())
                .or_insert_with(|| {
                    let symbol = local_symbol(next);
                    information.push(SymbolInformation {
                        symbol: symbol.clone(),
                        display_name: reference.name.clone(),
                        ..Default::default()
                    });
                    symbol
                })
                .clone()
        };
        occurrences.push(occurrence(reference.range, symbol, 0));
    }

    occurrences.sort_by(|a, b| a.range.cmp(&b.range));

    Document {
        language: scip_language(document.language),
        relative_path: document.relative_path.to_owned(),
        occurrences,
        symbols: information,
        // `qli_core::Position::column` is a byte offset within the line (it is
        // tree-sitter's column), which is exactly this encoding — no
        // conversion happens anywhere.
        position_encoding: PositionEncoding::UTF8CodeUnitOffsetFromLineStart.into(),
        ..Default::default()
    }
}

/// An occurrence carrying its range in both SCIP encodings, kept equivalent:
/// the typed `single_line_range`/`multi_line_range` current consumers prefer,
/// and the packed `[line, start, end]` / `[line, start, line, end]` list that
/// is all consumers predating the typed fields read.
fn occurrence(range: Range, symbol: String, symbol_roles: i32) -> Occurrence {
    let (start_line, start_character) =
        (coordinate(range.start.line), coordinate(range.start.column));
    let (end_line, end_character) = (coordinate(range.end.line), coordinate(range.end.column));
    let (packed, typed) = if start_line == end_line {
        (
            vec![start_line, start_character, end_character],
            Typed_range::SingleLineRange(SingleLineRange {
                line: start_line,
                start_character,
                end_character,
                ..Default::default()
            }),
        )
    } else {
        (
            vec![start_line, start_character, end_line, end_character],
            Typed_range::MultiLineRange(MultiLineRange {
                start_line,
                start_character,
                end_line,
                end_character,
                ..Default::default()
            }),
        )
    };
    Occurrence {
        range: packed,
        typed_range: Some(typed),
        symbol,
        symbol_roles,
        ..Default::default()
    }
}

/// SCIP coordinates are `int32`; a position past that range saturates rather
/// than wrapping negative.
fn coordinate(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

fn kind(kind: SymbolKind) -> Kind {
    match kind {
        SymbolKind::Function => Kind::Function,
        SymbolKind::Method => Kind::Method,
        SymbolKind::Class => Kind::Class,
        SymbolKind::Variable => Kind::Variable,
    }
}

/// The `Document.language` string for a qli language id.
///
/// SCIP asks for the name of its own `Language` enum value where one exists.
/// The id is matched case-insensitively against those names (`python` →
/// `Python`, `csharp` → `CSharp`); ids SCIP names differently are listed
/// here; anything else is passed through unchanged, as SCIP permits.
fn scip_language(language: &LanguageId) -> String {
    use protobuf::Enum;

    let id = match language.as_str() {
        "tsx" => "typescriptreact",
        "jsx" => "javascriptreact",
        other => other,
    };
    scip::types::Language::VALUES
        .iter()
        .filter(|value| **value != scip::types::Language::UnspecifiedLanguage)
        .map(|value| format!("{value:?}"))
        .find(|name| name.eq_ignore_ascii_case(id))
        .unwrap_or_else(|| language.to_string())
}

/// The `file://` URI for an absolute path given as `/`-separated segments,
/// percent-encoding everything outside RFC 3986's unreserved set.
#[must_use]
pub fn file_uri(absolute_path: &str) -> String {
    use std::fmt::Write;

    let mut uri = String::from("file://");
    if !absolute_path.starts_with('/') {
        uri.push('/');
    }
    for byte in absolute_path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(char::from(byte));
            }
            other => write!(uri, "%{other:02X}").expect("writing to a String cannot fail"),
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use qli_core::{Position, Reference, Symbol};

    use super::*;

    fn span(line: u32, start: u32, end: u32) -> Range {
        Range::new(Position::new(line, start), Position::new(line, end))
    }

    fn function(name: &str, range: Range) -> Symbol {
        Symbol::new(format!("stale:{name}"), name, SymbolKind::Function, range)
    }

    fn result(symbols: Vec<Symbol>, references: Vec<Reference>) -> AnalysisResult {
        AnalysisResult {
            symbols,
            references,
            ..Default::default()
        }
    }

    fn index(documents: &[(&str, &str, &AnalysisResult)]) -> (Index, IndexStats) {
        let languages: Vec<LanguageId> = documents
            .iter()
            .map(|(_, language, _)| LanguageId::from(*language))
            .collect();
        let documents: Vec<IndexDocument<'_>> = documents
            .iter()
            .zip(&languages)
            .map(|((relative_path, _, result), language)| IndexDocument {
                relative_path,
                language,
                result,
            })
            .collect();
        build_index(&Project::default(), &documents)
    }

    #[test]
    fn cross_file_reference_carries_the_definitions_symbol() {
        let lib = result(vec![function("foo", span(0, 4, 7))], vec![]);
        let app = result(vec![], vec![Reference::new("foo", span(2, 0, 3))]);
        // Passed out of path order; emitted sorted.
        let (index, stats) = index(&[("lib.py", "python", &lib), ("app.py", "python", &app)]);

        let paths: Vec<&str> = index
            .documents
            .iter()
            .map(|d| d.relative_path.as_str())
            .collect();
        assert_eq!(paths, ["app.py", "lib.py"]);

        let definition = &index.documents[1].occurrences[0];
        assert_eq!(definition.symbol, "scip-qli . . . lib/foo().");
        assert_eq!(definition.symbol_roles, SymbolRole::Definition as i32);
        let reference = &index.documents[0].occurrences[0];
        assert_eq!(reference.symbol, definition.symbol);
        assert_eq!(reference.symbol_roles, 0);

        let info = &index.documents[1].symbols[0];
        assert_eq!(info.symbol, definition.symbol);
        assert_eq!(info.display_name, "foo");
        assert_eq!(info.kind.enum_value(), Ok(Kind::Function));

        assert_eq!(
            stats,
            IndexStats {
                documents: 2,
                symbols: 1,
                references: 1,
                cross_file: 1,
                ..Default::default()
            }
        );
    }

    #[test]
    fn unresolved_and_ambiguous_references_are_recorded_as_document_locals() {
        let a = result(vec![function("helper", span(0, 4, 10))], vec![]);
        let b = result(vec![function("helper", span(0, 4, 10))], vec![]);
        let app = result(
            vec![],
            vec![
                Reference::new("missing", span(0, 0, 7)),
                Reference::new("helper", span(1, 0, 6)),
                Reference::new("missing", span(2, 0, 7)),
            ],
        );
        let (index, stats) = index(&[
            ("a.py", "python", &a),
            ("app.py", "python", &app),
            ("b.py", "python", &b),
        ]);
        let app_document = &index.documents[1];
        let symbols: Vec<&str> = app_document
            .occurrences
            .iter()
            .map(|o| o.symbol.as_str())
            .collect();
        assert_eq!(symbols, ["local 0", "local 1", "local 0"]);
        let names: Vec<(&str, &str)> = app_document
            .symbols
            .iter()
            .map(|info| (info.symbol.as_str(), info.display_name.as_str()))
            .collect();
        assert_eq!(names, [("local 0", "missing"), ("local 1", "helper")]);
        assert_eq!((stats.ambiguous, stats.unresolved), (1, 2));
    }

    #[test]
    fn same_file_reference_ignores_the_stale_path_in_the_precursor_id() {
        let definition = function("foo", span(0, 4, 7));
        let call = Reference::new("foo", span(3, 0, 3)).resolved_to(definition.id.clone());
        let file = result(vec![definition], vec![call]);
        // Two paths, one cached result: each document resolves to its own symbol.
        let (index, stats) = index(&[("a/x.py", "python", &file), ("b/x.py", "python", &file)]);
        for (document, expected) in index
            .documents
            .iter()
            .zip(["scip-qli . . . a/x/foo().", "scip-qli . . . b/x/foo()."])
        {
            for occurrence in &document.occurrences {
                assert_eq!(occurrence.symbol, expected);
            }
        }
        assert_eq!(stats.same_file, 2);
    }

    #[test]
    fn ranges_use_both_encodings_and_utf8_byte_columns() {
        // Line 0 is `é = foo()`: `é` is two bytes, so `foo` spans bytes 5..8
        // (UTF-16/UTF-32 columns would be 4..7).
        let line = "é = foo()";
        let start = u32::try_from(line.find("foo").unwrap()).unwrap();
        assert_eq!(start, 5);
        let single = Reference::new("foo", span(0, start, start + 3));
        let multi = Reference::new("bar", Range::new(Position::new(1, 2), Position::new(3, 4)));
        let file = result(vec![], vec![single, multi]);
        let (index, _) = index(&[("m.py", "python", &file)]);
        let document = &index.documents[0];
        assert_eq!(
            document.position_encoding.enum_value(),
            Ok(PositionEncoding::UTF8CodeUnitOffsetFromLineStart)
        );

        let single = &document.occurrences[0];
        assert_eq!(single.range, [0, 5, 8]);
        assert_eq!(single.single_line_range().line, 0);
        assert_eq!(single.single_line_range().start_character, 5);
        assert_eq!(single.single_line_range().end_character, 8);

        let multi = &document.occurrences[1];
        assert_eq!(multi.range, [1, 2, 3, 4]);
        assert!(multi.has_multi_line_range());
        assert_eq!(multi.multi_line_range().end_line, 3);
        assert_eq!(multi.multi_line_range().end_character, 4);
    }

    #[test]
    fn metadata_names_the_tool_root_and_encoding() {
        let project = Project {
            root_uri: file_uri("/work/my project"),
            tool_version: "1.2.3".to_owned(),
            tool_arguments: vec!["index".to_owned()],
            package: Package::default(),
        };
        let (index, stats) = build_index(&project, &[]);
        assert_eq!(index.metadata.project_root, "file:///work/my%20project");
        assert_eq!(index.metadata.tool_info.name, "qli");
        assert_eq!(index.metadata.tool_info.version, "1.2.3");
        assert_eq!(index.metadata.tool_info.arguments, ["index"]);
        assert_eq!(
            index.metadata.text_document_encoding.enum_value(),
            Ok(TextEncoding::UTF8)
        );
        assert_eq!(stats, IndexStats::default());
    }

    #[test]
    fn language_ids_map_to_scip_language_names() {
        for (id, expected) in [
            ("python", "Python"),
            ("typescript", "TypeScript"),
            ("tsx", "TypeScriptReact"),
            ("csharp", "CSharp"),
            ("angular", "angular"),
        ] {
            assert_eq!(scip_language(&LanguageId::from(id)), expected);
        }
    }

    #[test]
    fn file_uri_percent_encodes_and_roots_the_path() {
        assert_eq!(file_uri("/a/b-c_d.e~f"), "file:///a/b-c_d.e~f");
        assert_eq!(file_uri("/a/é#?"), "file:///a/%C3%A9%23%3F");
        assert_eq!(file_uri("C:/x"), "file:///C%3A/x");
    }
}
