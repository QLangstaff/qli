//! C# language adapter for the qli polyglot CLI.
//!
//! Wraps the `tree-sitter-c-sharp` grammar behind [`qli_lang::Language`] via
//! [`qli_lang::TreeSitterLanguage`]. The only C#-specific data lives here:
//! the grammar handle, the `.cs` extension, and the tree-sitter queries
//! analyzers ask for by name (the `"defrefs"` query drives the
//! definition/reference extractor).

use qli_lang::TreeSitterLanguage;

/// Tree-sitter query naming C# definitions and call sites for the `DefRefs`
/// analyzer. The capture-name contract matches the Python adapter's: `@def.*`
/// with `@name`, `@ref.call`. Structs, interfaces, and records have no
/// `SymbolKind` of their own, so they are reported as `@def.class`; fields and
/// locals both declare through `variable_declarator`. Like the Python and
/// TypeScript queries, only a bare called name is a `@ref.call` (plus its
/// generic form, `Foo<T>()`): a member access (`widget.Render()`) cannot be
/// resolved by name without knowing the receiver's type.
const DEFREFS_QUERY: &str = r"
(method_declaration
  name: (identifier) @name) @def.method

(local_function_statement
  name: (identifier) @name) @def.function

(class_declaration
  name: (identifier) @name) @def.class

(struct_declaration
  name: (identifier) @name) @def.class

(interface_declaration
  name: (identifier) @name) @def.class

(record_declaration
  name: (identifier) @name) @def.class

(variable_declarator
  name: (identifier) @name) @def.variable

(invocation_expression
  function: (identifier) @ref.call)

(invocation_expression
  function: (generic_name
    (identifier) @ref.call))
";

/// The C# [`Language`](qli_lang::Language) adapter (`.cs`).
#[must_use]
pub fn language() -> TreeSitterLanguage {
    TreeSitterLanguage::new(
        "csharp",
        &["cs"],
        tree_sitter_c_sharp::LANGUAGE.into(),
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
            "t.cs".into(),
            content.as_bytes().to_vec(),
            LanguageId::from("csharp"),
        )
    }

    #[test]
    fn parses_csharp_into_nonempty_tree() {
        let lang = language();
        let tree = lang
            .parse(&src("class C {\n    int F() { return 1; }\n}\n"))
            .expect("parse");
        let root = tree.root_node();
        assert_eq!(root.kind(), "compilation_unit");
        assert!(root.child_count() > 0, "expected a non-empty tree");
        assert!(!root.has_error(), "well-formed source should not error");
    }

    #[test]
    fn claims_cs_extension() {
        assert_eq!(language().extensions(), &["cs"]);
        assert_eq!(language().id(), LanguageId::from("csharp"));
    }

    #[test]
    fn exposes_defrefs_query() {
        assert!(language().named_query("defrefs").is_some());
        assert!(language().named_query("nonexistent").is_none());
    }

    #[test]
    fn defrefs_query_compiles_against_grammar() {
        // `DefRefs` swallows a query that fails to compile and emits nothing,
        // so a node kind or field the grammar renames must fail here instead.
        tree_sitter::Query::new(&language().ts_language(), DEFREFS_QUERY).expect("valid query");
    }
}
