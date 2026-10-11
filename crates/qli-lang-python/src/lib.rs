//! Python language adapter for the qli polyglot CLI.
//!
//! Wraps the `tree-sitter-python` grammar behind [`qli_lang::Language`] via
//! [`qli_lang::TreeSitterLanguage`]. The only Python-specific data lives here:
//! the grammar handle, the `.py` extension, and the tree-sitter queries
//! analyzers ask for by name (the `"defrefs"` query drives Phase 2I's
//! definition/reference extractor).

use qli_lang::TreeSitterLanguage;

/// Tree-sitter query naming Python definitions and call sites for the
/// `DefRefs` analyzer. Capture names are the contract with the analyzer:
/// `@def.function` / `@def.class` mark definitions (with `@name` the bound
/// identifier), and `@ref.call` marks a called name.
const DEFREFS_QUERY: &str = r"
(function_definition
  name: (identifier) @name) @def.function

(class_definition
  name: (identifier) @name) @def.class

(assignment
  left: (identifier) @name) @def.variable

(call
  function: (identifier) @ref.call)
";

/// The Python [`Language`](qli_lang::Language) adapter (`.py`).
#[must_use]
pub fn language() -> TreeSitterLanguage {
    TreeSitterLanguage::new(
        "python",
        &["py"],
        tree_sitter_python::LANGUAGE.into(),
        &[("defrefs", DEFREFS_QUERY)],
    )
}

#[cfg(test)]
mod tests {
    use qli_core::{LanguageId, Source};
    use qli_lang::Language;

    use super::*;

    fn src(content: &str) -> Source {
        Source::new(
            "t.py".into(),
            content.as_bytes().to_vec(),
            LanguageId::from("python"),
        )
    }

    #[test]
    fn parses_python_into_nonempty_tree() {
        let lang = language();
        let tree = lang.parse(&src("def f():\n    return 1\n")).expect("parse");
        let root = tree.root_node();
        assert_eq!(root.kind(), "module");
        assert!(root.child_count() > 0, "expected a non-empty tree");
        assert!(!root.has_error(), "well-formed source should not error");
    }

    #[test]
    fn claims_py_extension() {
        assert_eq!(language().extensions(), &["py"]);
        assert_eq!(language().id(), LanguageId::from("python"));
    }

    #[test]
    fn exposes_defrefs_query() {
        assert!(language().named_query("defrefs").is_some());
        assert!(language().named_query("nonexistent").is_none());
    }
}
