//! SCIP index emission for the qli polyglot CLI.
//!
//! Turns the engine's per-file output ([`qli_core::AnalysisResult`]: symbols
//! and references) into a [SCIP](https://github.com/scip-code/scip) index:
//!
//! - [`symbol`] — the `scip-qli` symbol scheme, specified in
//!   [`SCHEME.md`](../SCHEME.md);
//! - [`build_index`] — cross-file resolution plus conversion to SCIP
//!   `Document`/`Occurrence` protobufs;
//! - [`encode`] / [`decode`] — the protobuf wire form.
//!
//! This crate is where the protobuf dependency lives. The conversion is here
//! rather than in `qli-outputs` so that the per-file formatters (and everything
//! that links them) do not pay for `protobuf`; only a frontend that writes an
//! index — `qli index` — depends on `qli-scip`. The crate is pure like the
//! engine: it is handed analysis results and returns bytes, and never touches
//! the filesystem.

mod index;
pub mod symbol;

pub use index::{build_index, file_uri, IndexDocument, IndexStats, Project};
pub use scip::types;
pub use symbol::Package;

/// Why a byte string is not a SCIP index.
#[derive(Debug)]
pub struct DecodeError(protobuf::Error);

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not a valid SCIP index: {}", self.0)
    }
}

impl std::error::Error for DecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Serialize an index to the bytes of an `index.scip` file.
#[must_use]
pub fn encode(index: &types::Index) -> Vec<u8> {
    use protobuf::Message;

    // Serialization can only fail on a message over protobuf's 2 GiB limit —
    // an index that large cannot be built in memory in the first place.
    index
        .write_to_bytes()
        .expect("a SCIP index built in memory is within protobuf's size limit")
}

/// Parse the bytes of an `index.scip` file.
pub fn decode(bytes: &[u8]) -> Result<types::Index, DecodeError> {
    use protobuf::Message;

    types::Index::parse_from_bytes(bytes).map_err(DecodeError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_then_decode_roundtrips() {
        let project = Project {
            root_uri: file_uri("/work/project"),
            tool_version: "0.0.0".to_owned(),
            ..Default::default()
        };
        let (index, _) = build_index(&project, &[]);
        let decoded = decode(&encode(&index)).expect("decodes");
        assert_eq!(decoded, index);
    }

    #[test]
    fn decode_rejects_garbage() {
        let err = decode(&[0xff, 0xff, 0xff]).unwrap_err();
        assert!(err.to_string().starts_with("not a valid SCIP index"));
    }
}
