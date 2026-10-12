//! The `scip-qli` symbol scheme: how a `qli-core` [`Symbol`] in a file becomes
//! a SCIP symbol string. [`SCHEME.md`](../SCHEME.md) is the specification; this
//! module is its implementation.

use std::collections::HashMap;

use qli_core::{LanguageId, Symbol, SymbolKind};
use scip::types::descriptor::Suffix;
use scip::types::Descriptor;

/// The `<scheme>` field of every global symbol qli emits.
pub const SCHEME: &str = "scip-qli";

/// The `<package-manager> <package-name> <package-version>` triple.
///
/// An empty field is written as SCIP's `.` placeholder, so the default value
/// is the "no public package" form `. . .`.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Package {
    pub manager: String,
    pub name: String,
    pub version: String,
}

/// How a file's project-relative path becomes namespace descriptors.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModuleRule {
    /// One namespace per path segment, file name kept whole (`src/a.ts` →
    /// ``src/`a.ts`/``). The default for every language without its own rule.
    FilePath,
    /// As [`FilePath`](ModuleRule::FilePath), but the file name loses this
    /// exact extension, for languages whose module name is the file stem
    /// (`pkg/util.py` → `pkg/util/`).
    FileStem(&'static str),
}

impl ModuleRule {
    /// The rule for a language id. Languages not listed get
    /// [`ModuleRule::FilePath`].
    #[must_use]
    pub fn for_language(language: &LanguageId) -> Self {
        match language.as_str() {
            "python" => ModuleRule::FileStem("py"),
            _ => ModuleRule::FilePath,
        }
    }

    /// Split a `/`-separated project-relative path into namespace names.
    fn namespaces(self, relative_path: &str) -> Vec<&str> {
        let mut names: Vec<&str> = relative_path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .collect();
        if let (ModuleRule::FileStem(extension), Some(file_name)) = (self, names.last_mut()) {
            let stem = file_name
                .strip_suffix(extension)
                .and_then(|rest| rest.strip_suffix('.'));
            if let Some(stem) = stem.filter(|stem| !stem.is_empty()) {
                *file_name = stem;
            }
        }
        names
    }
}

/// The SCIP symbol string for each of one file's definitions, index-aligned
/// with `symbols`.
///
/// Definitions that share a descriptor share a symbol, except callables:
/// functions and methods repeating a name within the file get SCIP method
/// disambiguators (`+1`, `+2`, …) in source order, the first one bare.
#[must_use]
pub fn document_symbols(
    package: &Package,
    language: &LanguageId,
    relative_path: &str,
    symbols: &[Symbol],
) -> Vec<String> {
    let namespaces: Vec<Descriptor> = ModuleRule::for_language(language)
        .namespaces(relative_path)
        .into_iter()
        .map(|name| descriptor(name, Suffix::Namespace, String::new()))
        .collect();

    // Number same-named callables by source position, not by input order, so
    // the disambiguator is a property of the file rather than of the analyzer.
    let mut by_position: Vec<usize> = (0..symbols.len()).collect();
    by_position.sort_by_key(|&index| (symbols[index].range.start, index));
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut disambiguators = vec![String::new(); symbols.len()];
    for index in by_position {
        let symbol = &symbols[index];
        if suffix(symbol.kind) == Suffix::Method {
            let count = seen.entry(symbol.name.as_str()).or_insert(0);
            if *count > 0 {
                disambiguators[index] = format!("+{count}");
            }
            *count += 1;
        }
    }

    symbols
        .iter()
        .zip(disambiguators)
        .map(|(symbol, disambiguator)| {
            let mut descriptors = namespaces.clone();
            descriptors.push(descriptor(&symbol.name, suffix(symbol.kind), disambiguator));
            scip::symbol::format_symbol(scip::types::Symbol {
                scheme: SCHEME.to_owned(),
                package: Some(scip::types::Package {
                    manager: escape_spaces(&package.manager),
                    name: escape_spaces(&package.name),
                    version: escape_spaces(&package.version),
                    ..Default::default()
                })
                .into(),
                descriptors,
                ..Default::default()
            })
        })
        .collect()
}

/// The document-local symbol for the `ordinal`-th distinct unresolved name of
/// a document (`local 0`, `local 1`, …).
#[must_use]
pub fn local_symbol(ordinal: usize) -> String {
    scip::symbol::format_symbol(scip::types::Symbol::new_local(ordinal))
}

/// The descriptor suffix a definition kind maps to.
fn suffix(kind: SymbolKind) -> Suffix {
    match kind {
        SymbolKind::Function | SymbolKind::Method => Suffix::Method,
        SymbolKind::Class => Suffix::Type,
        SymbolKind::Variable => Suffix::Term,
    }
}

fn descriptor(name: &str, suffix: Suffix, disambiguator: String) -> Descriptor {
    Descriptor {
        name: name.to_owned(),
        disambiguator,
        suffix: suffix.into(),
        ..Default::default()
    }
}

/// SCIP separates the scheme and package fields with single spaces; a literal
/// space inside one is written doubled.
fn escape_spaces(field: &str) -> String {
    field.replace(' ', "  ")
}

#[cfg(test)]
mod tests {
    use qli_core::{Position, Range};

    use super::*;

    fn definition(name: &str, kind: SymbolKind, line: u32) -> Symbol {
        let range = Range::new(Position::new(line, 0), Position::new(line, 1));
        // The precursor id deliberately names another path: the scheme must
        // never read it.
        Symbol::new(
            format!("qli-local:/elsewhere/x:{name}#{line}:0"),
            name,
            kind,
            range,
        )
    }

    fn symbols_for(language: &str, path: &str, symbols: &[Symbol]) -> Vec<String> {
        document_symbols(
            &Package::default(),
            &LanguageId::from(language),
            path,
            symbols,
        )
    }

    #[test]
    fn python_path_maps_to_module_namespaces() {
        let symbols = [
            definition("foo", SymbolKind::Function, 0),
            definition("Widget", SymbolKind::Class, 1),
            definition("RETRIES", SymbolKind::Variable, 2),
        ];
        assert_eq!(
            symbols_for("python", "pkg/util.py", &symbols),
            [
                "scip-qli . . . pkg/util/foo().",
                "scip-qli . . . pkg/util/Widget#",
                "scip-qli . . . pkg/util/RETRIES.",
            ]
        );
    }

    #[test]
    fn default_rule_keeps_the_file_name_and_escapes_it() {
        let symbols = [definition("foo", SymbolKind::Function, 0)];
        for language in ["typescript", "tsx", "csharp", "some-future-language"] {
            assert_eq!(
                symbols_for(language, "src/app.ts", &symbols),
                ["scip-qli . . . src/`app.ts`/foo()."],
                "{language}"
            );
        }
    }

    #[test]
    fn file_stem_rule_only_strips_its_own_exact_extension() {
        let symbols = [definition("foo", SymbolKind::Function, 0)];
        assert_eq!(
            symbols_for("python", "a/B.PY", &symbols),
            ["scip-qli . . . a/`B.PY`/foo()."]
        );
        assert_eq!(
            symbols_for("python", ".py", &symbols),
            ["scip-qli . . . `.py`/foo()."]
        );
    }

    #[test]
    fn repeated_callables_are_disambiguated_in_source_order() {
        // `render` as a function and as two methods, listed out of order.
        let symbols = [
            definition("render", SymbolKind::Method, 9),
            definition("render", SymbolKind::Function, 1),
            definition("render", SymbolKind::Method, 5),
        ];
        assert_eq!(
            symbols_for("python", "w.py", &symbols),
            [
                "scip-qli . . . w/render(+2).",
                "scip-qli . . . w/render().",
                "scip-qli . . . w/render(+1).",
            ]
        );
    }

    #[test]
    fn repeated_variables_and_classes_share_one_symbol() {
        let symbols = [
            definition("x", SymbolKind::Variable, 0),
            definition("x", SymbolKind::Variable, 1),
        ];
        let formatted = symbols_for("python", "m.py", &symbols);
        assert_eq!(formatted[0], "scip-qli . . . m/x.");
        assert_eq!(formatted[0], formatted[1]);
    }

    #[test]
    fn package_fields_fill_the_placeholders_with_spaces_escaped() {
        let package = Package {
            manager: "pip".to_owned(),
            name: "my pkg".to_owned(),
            version: "1.0".to_owned(),
        };
        let formatted = document_symbols(
            &package,
            &LanguageId::from("python"),
            "a.py",
            &[definition("foo", SymbolKind::Function, 0)],
        );
        assert_eq!(formatted, ["scip-qli pip my  pkg 1.0 a/foo()."]);
    }

    #[test]
    fn every_emitted_symbol_parses_back_with_the_scip_crate() {
        let symbols = [
            definition("naïve", SymbolKind::Function, 0),
            definition("a`b", SymbolKind::Variable, 1),
            definition("Widget", SymbolKind::Class, 2),
        ];
        for formatted in symbols_for("typescript", "src/my dir/app.test.ts", &symbols) {
            let parsed = scip::symbol::parse_symbol(&formatted)
                .unwrap_or_else(|err| panic!("{formatted}: {err:?}"));
            assert_eq!(parsed.scheme, SCHEME);
            assert_eq!(parsed.descriptors.len(), 4, "{formatted}");
            assert_eq!(parsed.descriptors[1].name, "my dir");
            assert_eq!(parsed.descriptors[2].name, "app.test.ts");
        }
        assert_eq!(local_symbol(3), "local 3");
        assert!(scip::symbol::is_local_symbol(&local_symbol(0)));
    }
}
