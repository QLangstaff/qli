//! Latency measurement for the server path: milliseconds per
//! `textDocument/didChange` on generated Python documents.
//!
//! This reports, it does not gate — timing thresholds would make the suite
//! flaky across machines — so it is `#[ignore]`d. Run it with:
//!
//! ```text
//! cargo test -p qli-lsp --release --test latency -- --ignored --nocapture
//! ```
//!
//! Each timed `didChange` goes through the real `LspService` (JSON-RPC
//! decode, handler, analysis, `publishDiagnostics` encode) and ends when its
//! diagnostics have been received. Per document size, two cases:
//!
//! - **changed content** — every iteration's text is new, so neither cache
//!   tier can answer: parse + both analyzers run;
//! - **unchanged content** — the same text again: the in-memory tier answers.
//!
//! followed by a per-stage breakdown (parse, each analyzer) of the same
//! document, measured directly against the engine.
//!
//! Not included: transport I/O (there is none in-process), and the on-disk
//! store — the persisted tier here is the in-memory `MemoryStore`, whereas
//! `qli lsp` does one cache-file lookup per analyzer on every changed
//! `didChange`.
//!
//! The numbers recorded in `editors/README.md` come from this test.

mod common;

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{MemoryStore, Session};
use qli_core::Source;
use qli_lang::{AnalysisContext, Language};

/// Document sizes to measure, in lines. The first is the headline figure;
/// the second shows how cost scales.
const SIZES: [usize; 2] = [1000, 5000];
const ITERATIONS: usize = 200;
const STAGE_ITERATIONS: u32 = 50;

/// A Python module of roughly `lines` lines: functions with a marker comment
/// each, calling one another, so both analyzers have real work.
fn python_document(lines: usize) -> String {
    const LINES_PER_FUNCTION: usize = 5;
    let mut text = String::from("\"\"\"Generated latency fixture.\"\"\"\n");
    for i in 0..lines / LINES_PER_FUNCTION {
        let marker = ["TODO", "FIXME", "XXX", "HACK"][i % 4];
        let _ = write!(
            text,
            "\ndef handler_{i}(value):\n    # {marker}: revisit handler {i}\n    \
             total = handler_{previous}(value) + {i}\n    return total\n",
            previous = i.saturating_sub(1),
        );
    }
    text
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let count = u32::try_from(samples.len()).expect("sample count fits in u32");
    let mean = millis(samples.iter().sum::<Duration>() / count);
    println!(
        "  {label}: mean {mean:.3} ms, median {:.3} ms, p95 {:.3} ms, max {:.3} ms ({} samples)",
        millis(samples[samples.len() / 2]),
        millis(samples[samples.len() * 95 / 100]),
        millis(samples[samples.len() - 1]),
        samples.len(),
    );
}

/// Time `didChange` → `publishDiagnostics` through the server for `base`.
async fn measure_did_change(base: &str) {
    let uri = "file:///tmp/latency.py";
    let mut session =
        Session::initialized(common::pipeline(&Arc::new(MemoryStore::default()))).await;
    session.open(uri, base).await;
    let diagnostics = session.take_one_diagnostics()["diagnostics"]
        .as_array()
        .map_or(0, Vec::len);
    println!(
        "document: {} lines, {} bytes, {diagnostics} diagnostics",
        base.lines().count(),
        base.len(),
    );

    let mut version = 1;
    let mut changed = Vec::with_capacity(ITERATIONS);
    for i in 0..ITERATIONS {
        // Unique trailing line: a content hash no tier has seen.
        let text = format!("{base}\nedit_{i} = {i}\n");
        version += 1;
        let start = Instant::now();
        session.change(uri, version, &text).await;
        changed.push(start.elapsed());
        session.take_one_diagnostics();
    }

    let mut unchanged = Vec::with_capacity(ITERATIONS);
    let text = format!("{base}\nsettled = True\n");
    version += 1;
    session.change(uri, version, &text).await;
    session.take_one_diagnostics();
    for _ in 0..ITERATIONS {
        version += 1;
        let start = Instant::now();
        session.change(uri, version, &text).await;
        unchanged.push(start.elapsed());
        session.take_one_diagnostics();
    }

    report("didChange, changed content   (analysis runs)", changed);
    report("didChange, unchanged content (memory tier)  ", unchanged);
}

/// Time each engine stage of a changed-content `didChange` on its own.
fn measure_stages(base: &str) {
    let language = qli_lang_python::language();
    let source = Source::new("latency.py".into(), base.as_bytes().to_vec(), language.id());
    let mean = |run: &dyn Fn()| {
        let start = Instant::now();
        for _ in 0..STAGE_ITERATIONS {
            run();
        }
        millis(start.elapsed() / STAGE_ITERATIONS)
    };

    println!(
        "  stage: tree-sitter parse    mean {:.3} ms",
        mean(&|| drop(language.parse(&source)))
    );
    let tree = language.parse(&source).expect("python grammar loads");
    let ctx = AnalysisContext::new(&language, &source, &tree);
    let analyzers = qli_analyzers::builtin();
    for id in analyzers.ids() {
        let analyzer = analyzers.get(id).expect("listed id is registered");
        println!(
            "  stage: {id:<20} mean {:.3} ms",
            mean(&|| drop(analyzer.analyze(&ctx)))
        );
    }
}

#[tokio::test]
#[ignore = "measurement, not a gate; see module docs for how to run"]
async fn did_change_latency_on_generated_python_files() {
    println!(
        "build: {}",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );
    for lines in SIZES {
        let base = python_document(lines);
        measure_did_change(&base).await;
        measure_stages(&base);
    }
}
