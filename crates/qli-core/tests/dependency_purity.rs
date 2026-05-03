//! Engine-purity gate: `qli-core` must remain a no-I/O / no-global-state
//! library. The first convenient `tracing`/`clap`/`tokio` import erodes
//! that invariant invisibly; this test fails before it can.
//!
//! Plan reference: Phase 1L "Engine-purity test" — lands before Phase 2A
//! merges so the moment a Phase 2A draft adds an unwanted dep, CI bites.
//!
//! Adding a permitted dep: extend `ALLOWED_DIRECT_DEPENDENCIES` below and
//! note in the PR why the new dep doesn't violate the no-I/O / no-global
//! invariant. The bar is high; "convenient" is not a justification.

use std::path::PathBuf;

use cargo_metadata::MetadataCommand;

/// Direct dependencies (`[dependencies]`, not `[dev-dependencies]`) that
/// `qli-core` is allowed to declare. The list starts empty: the crate
/// currently has zero direct deps and must justify every addition.
///
/// `[dev-dependencies]` are intentionally NOT gated — `cargo_metadata` is
/// the most obvious example: it's a test-only dep used by *this* test.
const ALLOWED_DIRECT_DEPENDENCIES: &[&str] = &[];

#[test]
fn qli_core_has_no_unallowlisted_direct_dependencies() {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let metadata = MetadataCommand::new()
        .manifest_path(&manifest_path)
        .no_deps()
        .exec()
        .expect("cargo metadata failed for qli-core");

    let pkg = metadata
        .packages
        .iter()
        .find(|p| p.name == "qli-core")
        .expect("qli-core package not found in metadata");

    let normal_deps: Vec<&str> = pkg
        .dependencies
        .iter()
        .filter(|d| matches!(d.kind, cargo_metadata::DependencyKind::Normal))
        .map(|d| d.name.as_str())
        .collect();

    let offenders: Vec<&str> = normal_deps
        .iter()
        .copied()
        .filter(|name| !ALLOWED_DIRECT_DEPENDENCIES.contains(name))
        .collect();

    assert!(
        offenders.is_empty(),
        "qli-core must stay a pure no-I/O library. Disallowed direct dependencies: {offenders:?}. \
         Either remove them from qli-core/Cargo.toml or add them to \
         ALLOWED_DIRECT_DEPENDENCIES in this test with a justifying note. \
         See plan: 'qli-core engine purity'."
    );
}
