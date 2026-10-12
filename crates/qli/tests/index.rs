//! `qli index` integration tests over the multi-file fixture
//! (`tests/fixtures/multifile`).
//!
//! Two layers:
//!   - **Decoded with the `scip` crate** (always runs): the emitted protobuf is
//!     parsed back and the symbols, cross-file references, ranges, and cache
//!     behavior are asserted directly.
//!   - **Read by the `scip` CLI** (`scip print --json`, `scip lint`): proves an
//!     independent SCIP implementation accepts the index. The CLI is an
//!     external binary — see [`scip_cli`] for how it is found and what happens
//!     when it is absent.

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use assert_cmd::Command as AssertCommand;
use common::XdgSandbox;
use predicates::prelude::*;
use qli_scip::types::{Document, Index, Occurrence, PositionEncoding, SymbolRole};
use serial_test::serial;
use tempfile::TempDir;

const PY_FOO: &str = "scip-qli . . . python/lib/foo().";
const PY_GADGET: &str = "scip-qli . . . python/lib/Gadget#";
const TS_FOO: &str = "scip-qli . . . typescript/`lib.ts`/foo().";

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/multifile")
        .canonicalize()
        .expect("multifile fixture exists")
}

fn qli() -> AssertCommand {
    let mut command = AssertCommand::cargo_bin("qli").unwrap();
    // A developer's `RUST_LOG` would override the `-v` level the cache test
    // relies on.
    command.env_remove("RUST_LOG").env("NO_COLOR", "1");
    command
}

/// Index the fixture into a fresh tempdir and decode the result.
fn index_fixture(extra_args: &[&str]) -> (TempDir, PathBuf, Index) {
    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("index.scip");
    qli()
        .arg("index")
        .arg(fixture_root())
        .arg("--output")
        .arg(&output)
        .args(extra_args)
        .assert()
        .success();
    let index = qli_scip::decode(&std::fs::read(&output).unwrap()).expect("valid SCIP index");
    (out_dir, output, index)
}

fn document<'a>(index: &'a Index, relative_path: &str) -> &'a Document {
    index
        .documents
        .iter()
        .find(|d| d.relative_path == relative_path)
        .unwrap_or_else(|| panic!("no document {relative_path}"))
}

/// The occurrence starting at zero-based `line` whose text is `text`, found by
/// searching that line of the fixture for it (`nth` match, zero-based) and
/// expecting the **byte** column.
fn occurrence_of<'a>(
    document: &'a Document,
    line: usize,
    text: &str,
    nth: usize,
) -> &'a Occurrence {
    let source = std::fs::read_to_string(fixture_root().join(&document.relative_path)).unwrap();
    let line_text = source.lines().nth(line).unwrap();
    let (start, _) = line_text.match_indices(text).nth(nth).unwrap();
    let expected = [line, start, start + text.len()].map(|n| i32::try_from(n).unwrap());
    document
        .occurrences
        .iter()
        .find(|o| o.range == expected)
        .unwrap_or_else(|| {
            panic!(
                "{}: no occurrence at {expected:?}; have {:#?}",
                document.relative_path, document.occurrences
            )
        })
}

fn display_name<'a>(document: &'a Document, symbol: &str) -> &'a str {
    document
        .symbols
        .iter()
        .find(|info| info.symbol == symbol)
        .map_or("", |info| info.display_name.as_str())
}

#[test]
#[serial]
fn index_writes_summary_to_stderr_and_nothing_to_stdout() {
    let _sandbox = XdgSandbox::new();
    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("index.scip");
    let assert = qli()
        .arg("index")
        .arg(fixture_root())
        .arg("--output")
        .arg(&output)
        .assert()
        .success()
        .stdout(predicate::str::is_empty());
    let bytes = std::fs::metadata(&output).unwrap().len();
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    assert_eq!(
        stderr,
        format!(
            "indexed 7 files to {}: {bytes} bytes, 13 symbols, 10 references \
             (1 same-file, 5 cross-file, 1 ambiguous, 3 unresolved)\n",
            output.display()
        )
    );
}

#[test]
#[serial]
fn call_in_one_file_resolves_to_the_definition_in_another() {
    let _sandbox = XdgSandbox::new();
    let (_out_dir, _output, index) = index_fixture(&[]);

    let paths: Vec<&str> = index
        .documents
        .iter()
        .map(|d| d.relative_path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "python/app.py",
            "python/helpers.py",
            "python/legacy.py",
            "python/lib.py",
            "typescript/app.ts",
            "typescript/lib.ts",
            "typescript/view.tsx",
        ]
    );
    assert_eq!(index.metadata.tool_info.name, "qli");
    assert_eq!(
        index.metadata.project_root,
        qli_scip::file_uri(fixture_root().to_str().unwrap())
    );

    // `foo` is defined in python/lib.py …
    let lib = document(&index, "python/lib.py");
    let definition = occurrence_of(lib, 5, "foo", 0);
    assert_eq!(definition.symbol, PY_FOO);
    assert_eq!(definition.symbol_roles, SymbolRole::Definition as i32);
    assert_eq!(display_name(lib, PY_FOO), "foo");
    // … called in the same file (resolved by the per-file analyzer) …
    assert_eq!(occurrence_of(lib, 11, "foo", 0).symbol, PY_FOO);
    // … and called in python/app.py, which only the project-wide pass links.
    let app = document(&index, "python/app.py");
    let call = occurrence_of(app, 6, "foo", 0);
    assert_eq!(call.symbol, PY_FOO);
    assert_eq!(call.symbol_roles, 0, "a reference, not a definition");
    assert_eq!(occurrence_of(app, 7, "Gadget", 0).symbol, PY_GADGET);

    // The same name in TypeScript resolves within TypeScript, including from
    // `.tsx` into `.ts`, and never to the Python definition.
    let ts_lib = document(&index, "typescript/lib.ts");
    assert_eq!(occurrence_of(ts_lib, 2, "foo", 0).symbol, TS_FOO);
    let ts_app = document(&index, "typescript/app.ts");
    assert_eq!(occurrence_of(ts_app, 3, "foo", 0).symbol, TS_FOO);
    let view = document(&index, "typescript/view.tsx");
    assert_eq!(occurrence_of(view, 3, "foo", 0).symbol, TS_FOO);
    assert_eq!(view.language, "TypeScriptReact");
    assert_eq!(app.language, "Python");
}

#[test]
#[serial]
fn ambiguous_and_unknown_names_stay_unresolved_but_recorded() {
    let _sandbox = XdgSandbox::new();
    let (_out_dir, _output, index) = index_fixture(&[]);

    // `helper` is defined in helpers.py *and* legacy.py: no winner is picked.
    let app = document(&index, "python/app.py");
    let helper = occurrence_of(app, 9, "helper", 0);
    assert!(helper.symbol.starts_with("local "), "{}", helper.symbol);
    assert_eq!(display_name(app, &helper.symbol), "helper");
    let missing = occurrence_of(app, 10, "missing", 0);
    assert!(missing.symbol.starts_with("local "), "{}", missing.symbol);
    assert_ne!(missing.symbol, helper.symbol);
    assert_eq!(display_name(app, &missing.symbol), "missing");

    // `render` exists only as a method of `Gadget`: not a bare-call target.
    let ts_app = document(&index, "typescript/app.ts");
    let render = occurrence_of(ts_app, 4, "render", 0);
    assert!(render.symbol.starts_with("local "), "{}", render.symbol);
}

#[test]
#[serial]
fn columns_on_a_non_ascii_line_are_utf8_byte_offsets() {
    let _sandbox = XdgSandbox::new();
    let (_out_dir, _output, index) = index_fixture(&[]);
    let app = document(&index, "python/app.py");
    assert_eq!(
        app.position_encoding.enum_value(),
        Ok(PositionEncoding::UTF8CodeUnitOffsetFromLineStart)
    );

    // Line 8 is `    naïve = "é"; foo(naïve)`: two 2-byte characters precede
    // the call, so its byte column (19) is two past its character column (17).
    let source = std::fs::read_to_string(fixture_root().join("python/app.py")).unwrap();
    let line = source.lines().nth(8).unwrap();
    let byte_column = line.find("foo").unwrap();
    let char_column = line[..byte_column].chars().count();
    assert_eq!((byte_column, char_column), (19, 17));

    let call = occurrence_of(app, 8, "foo", 0);
    assert_eq!(call.range, [8, 19, 22]);
    assert_eq!(call.symbol, PY_FOO);
    // The typed encoding agrees with the packed one.
    assert_eq!(call.single_line_range().line, 8);
    assert_eq!(call.single_line_range().start_character, 19);
    assert_eq!(call.single_line_range().end_character, 22);
    // `naïve` itself is 6 bytes wide.
    assert_eq!(
        occurrence_of(app, 8, "naïve", 0).symbol,
        "scip-qli . . . python/app/`naïve`."
    );
}

#[test]
#[serial]
fn reindex_reads_the_cache_and_leaves_cached_results_unresolved() {
    let _sandbox = XdgSandbox::new();
    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("index.scip");
    let run = || {
        let assert = qli()
            .arg("-vv")
            .arg("index")
            .arg(fixture_root())
            .arg("--output")
            .arg(&output)
            .assert()
            .success();
        let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
        (stderr, std::fs::read(&output).unwrap())
    };

    let (cold_log, cold_index) = run();
    assert_eq!(cold_log.matches("cache miss").count(), 7, "{cold_log}");
    assert_eq!(cold_log.matches("cache hit").count(), 0, "{cold_log}");

    let (warm_log, warm_index) = run();
    assert_eq!(warm_log.matches("cache hit").count(), 7, "{warm_log}");
    assert_eq!(warm_log.matches("cache miss").count(), 0, "{warm_log}");
    assert_eq!(warm_index, cold_index, "same input, same bytes");

    // Cross-file resolution was computed over the cached per-file results, not
    // written into them: `qli analyze` reading the same cache entry still
    // reports the call as unresolved, exactly as an uncached run does.
    let analyze = |extra: &[&str]| {
        let assert = qli()
            .args(["analyze", "--analyzer", "defrefs", "--format", "jsonl"])
            .args(extra)
            .arg(fixture_root().join("python/app.py"))
            .assert()
            .success();
        String::from_utf8(assert.get_output().stdout.clone()).unwrap()
    };
    let cached = analyze(&[]);
    assert_eq!(cached, analyze(&["--no-cache"]));
    let foo_call = cached
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|v| v["type"] == "reference" && v["name"] == "foo")
        .expect("a reference to foo");
    assert!(foo_call.get("resolved").is_none(), "{foo_call}");
}

#[test]
#[serial]
fn lang_filter_limits_the_index_and_rejects_unknown_ids() {
    let _sandbox = XdgSandbox::new();
    let (_out_dir, _output, index) = index_fixture(&["--lang", "typescript,tsx"]);
    let paths: Vec<&str> = index
        .documents
        .iter()
        .map(|d| d.relative_path.as_str())
        .collect();
    assert_eq!(
        paths,
        [
            "typescript/app.ts",
            "typescript/lib.ts",
            "typescript/view.tsx"
        ]
    );

    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("index.scip");
    qli()
        .arg("index")
        .arg(fixture_root())
        .arg("--output")
        .arg(&output)
        .args(["--lang", "pythn"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "unknown language `pythn`; known languages: ",
        ));
    assert!(!output.exists(), "nothing is written on a rejected filter");
}

#[test]
#[serial]
fn root_that_is_not_a_directory_exits_1() {
    let _sandbox = XdgSandbox::new();
    let out_dir = tempfile::tempdir().unwrap();
    qli()
        .arg("index")
        .arg(fixture_root().join("python/app.py"))
        .arg("--output")
        .arg(out_dir.path().join("index.scip"))
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("project root is not a directory"));
}

/// The SCIP CLI (<https://github.com/scip-code/scip>) to validate against, or
/// `None` after printing a notice when there is none to use.
///
/// Looked up as `$QLI_SCIP_BIN`, else `scip` on `PATH`. Either candidate must
/// identify itself as the SCIP *code-intelligence* CLI: Homebrew's `scip` is
/// the unrelated SCIP optimization solver, and must not be mistaken for it.
///
/// Under CI (`$CI` set, as GitHub Actions does) a missing CLI is a test
/// failure — the workflow installs it, so absence means the validation
/// silently stopped running. Locally it is a skip.
fn scip_cli() -> Option<PathBuf> {
    let candidate =
        std::env::var_os("QLI_SCIP_BIN").map_or_else(|| PathBuf::from("scip"), PathBuf::from);
    let is_scip_cli = run_scip(&candidate, &["print", "--help"], Path::new("."))
        .is_some_and(|(success, stdout)| success && stdout.contains("Print a SCIP index"));
    if is_scip_cli {
        return Some(candidate);
    }
    let notice = format!(
        "SCIP CLI not available (`{}` is missing or is not the scip-code/scip CLI); \
         install it from https://github.com/scip-code/scip/releases and point \
         QLI_SCIP_BIN at it",
        candidate.display()
    );
    assert!(std::env::var_os("CI").is_none(), "{notice}");
    // Written to the handle directly: libtest captures `eprintln!` from a
    // passing test, which would make the skip silent.
    let _ = writeln!(std::io::stderr(), "SKIPPED: {notice}");
    None
}

/// Run the SCIP CLI with no stdin and a deadline, returning its success flag
/// and stdout. `None` if it could not be spawned or had to be killed.
fn run_scip(bin: &Path, args: &[&str], cwd: &Path) -> Option<(bool, String)> {
    let stdout_file = tempfile::NamedTempFile::new().unwrap();
    let mut child = Command::new(bin)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(stdout_file.reopen().unwrap())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = std::fs::read_to_string(stdout_file.path()).unwrap();
    Some((status.success(), stdout))
}

#[test]
#[serial]
fn scip_cli_prints_and_lints_the_index() {
    let Some(scip) = scip_cli() else {
        return;
    };
    let _sandbox = XdgSandbox::new();
    let (out_dir, _output, _index) = index_fixture(&[]);

    let (success, stdout) = run_scip(&scip, &["print", "--json", "index.scip"], out_dir.path())
        .expect("scip print ran");
    assert!(success, "scip print failed");
    let printed: serde_json::Value = serde_json::from_str(&stdout).expect("scip print emits JSON");
    assert_eq!(printed["metadata"]["tool_info"]["name"], "qli");

    let documents = printed["documents"].as_array().expect("documents array");
    assert_eq!(documents.len(), 7);
    let occurrences = |relative_path: &str| -> &Vec<serde_json::Value> {
        documents
            .iter()
            .find(|d| d["relative_path"] == relative_path)
            .unwrap_or_else(|| panic!("scip print has no document {relative_path}"))["occurrences"]
            .as_array()
            .unwrap()
    };
    let has = |relative_path: &str, range: [i64; 3], symbol: &str, roles: Option<i64>| {
        occurrences(relative_path).iter().any(|o| {
            o["range"] == serde_json::json!(range)
                && o["symbol"] == symbol
                && o["symbol_roles"].as_i64() == roles
        })
    };
    // Definition in one file, reference to the same symbol in another.
    assert!(has("python/lib.py", [5, 4, 7], PY_FOO, Some(1)), "{stdout}");
    assert!(has("python/app.py", [6, 4, 7], PY_FOO, None), "{stdout}");
    assert!(has("python/app.py", [8, 19, 22], PY_FOO, None), "{stdout}");
    assert!(
        has("typescript/lib.ts", [2, 16, 19], TS_FOO, Some(1)),
        "{stdout}"
    );
    assert!(
        has("typescript/app.ts", [3, 2, 5], TS_FOO, None),
        "{stdout}"
    );
    assert!(
        has("typescript/view.tsx", [3, 9, 12], TS_FOO, None),
        "{stdout}"
    );
    // Unresolved references are present, as document-local symbols.
    assert!(
        occurrences("python/app.py")
            .iter()
            .filter(|o| o["symbol"]
                .as_str()
                .is_some_and(|s| s.starts_with("local ")))
            .count()
            == 2,
        "{stdout}"
    );

    let (success, stdout) =
        run_scip(&scip, &["lint", "index.scip"], out_dir.path()).expect("scip lint ran");
    assert!(success, "scip lint failed: {stdout}");
    assert_eq!(stdout, "", "scip lint reported issues");
}
