//! Machine-readable JSON Lines output: one self-describing JSON object per
//! finding, one per line — parseable by `jq` a line at a time.
//!
//! Every record carries `"file"` and a `"type"` discriminator
//! (`diagnostic`/`symbol`/`reference`) so a stream mixing all three (the
//! `defrefs` analyzer emits symbols and references) stays unambiguous.

use std::io::{self, Write};

use qli_core::{AnalysisResult, Reference, Symbol};
use serde::Serialize;
use serde_json::json;

/// Write `result` for `file` as JSON Lines.
pub fn write(w: &mut dyn Write, file: &str, result: &AnalysisResult) -> io::Result<()> {
    for d in &result.diagnostics {
        write_line(w, &tagged(file, "diagnostic", d))?;
    }
    for s in &result.symbols {
        write_line(w, &symbol_record(file, s))?;
    }
    for r in &result.references {
        write_line(w, &reference_record(file, r))?;
    }
    Ok(())
}

fn write_line(w: &mut dyn Write, value: &serde_json::Value) -> io::Result<()> {
    // Compact, one line — `serde_json::to_writer` never emits interior
    // newlines, so this stays a valid single JSONL record.
    serde_json::to_writer(&mut *w, value)?;
    writeln!(w)
}

/// Serialize `item` and splice in `file` + `type`.
fn tagged<T: Serialize>(file: &str, ty: &str, item: &T) -> serde_json::Value {
    let mut value = serde_json::to_value(item).expect("qli-core types serialize");
    if let Some(obj) = value.as_object_mut() {
        obj.insert("file".into(), json!(file));
        obj.insert("type".into(), json!(ty));
    }
    value
}

fn symbol_record(file: &str, s: &Symbol) -> serde_json::Value {
    tagged(file, "symbol", s)
}

fn reference_record(file: &str, r: &Reference) -> serde_json::Value {
    tagged(file, "reference", r)
}
