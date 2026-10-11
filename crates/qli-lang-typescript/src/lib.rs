//! TypeScript language adapter for the qli polyglot CLI.
//!
//! `tree-sitter-typescript` ships two grammars — one for `.ts`, one for `.tsx`
//! — and a tree-sitter [`Query`](tree_sitter::Query) must be compiled against
//! the same grammar that produced the tree it runs on. So this crate exposes
//! **two** adapters ([`typescript`] and [`tsx`]) rather than one adapter that
//! switches grammars mid-flight: each is a self-consistent (grammar, queries)
//! pair. The binary registers both; `--lang auto` routes `.ts` → `typescript`
//! and `.tsx` → `tsx`.

use qli_lang::TreeSitterLanguage;

/// Definition/reference query for the `DefRefs` analyzer. The capture-name
/// contract matches the Python adapter's: `@def.*` with `@name`, `@ref.call`.
/// The same query source compiles against both the `.ts` and `.tsx` grammars —
/// the node kinds it names are shared between them.
const DEFREFS_QUERY: &str = r"
(function_declaration
  name: (identifier) @name) @def.function

(method_definition
  name: (property_identifier) @name) @def.method

(class_declaration
  name: (type_identifier) @name) @def.class

(variable_declarator
  name: (identifier) @name) @def.variable

(call_expression
  function: (identifier) @ref.call)
";

/// The TypeScript adapter (`.ts`).
#[must_use]
pub fn typescript() -> TreeSitterLanguage {
    TreeSitterLanguage::new(
        "typescript",
        &["ts"],
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        &[("defrefs", DEFREFS_QUERY)],
    )
}

/// The TSX adapter (`.tsx`).
#[must_use]
pub fn tsx() -> TreeSitterLanguage {
    TreeSitterLanguage::new(
        "tsx",
        &["tsx"],
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        &[("defrefs", DEFREFS_QUERY)],
    )
}

#[cfg(test)]
mod tests {
    use qli_core::{LanguageId, Source};
    use qli_lang::Language;

    use super::*;

    #[test]
    fn parses_ts_into_nonempty_tree() {
        let lang = typescript();
        let source = Source::new(
            "t.ts".into(),
            b"function f(): number { return 1; }\n".to_vec(),
            LanguageId::from("typescript"),
        );
        let tree = lang.parse(&source).expect("parse");
        assert!(tree.root_node().child_count() > 0);
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn parses_tsx_into_nonempty_tree() {
        let lang = tsx();
        let source = Source::new(
            "t.tsx".into(),
            b"const x = () => <div>hi</div>;\n".to_vec(),
            LanguageId::from("tsx"),
        );
        let tree = lang.parse(&source).expect("parse");
        assert!(tree.root_node().child_count() > 0);
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn extensions_and_ids() {
        assert_eq!(typescript().extensions(), &["ts"]);
        assert_eq!(typescript().id(), LanguageId::from("typescript"));
        assert_eq!(tsx().extensions(), &["tsx"]);
        assert_eq!(tsx().id(), LanguageId::from("tsx"));
    }
}
