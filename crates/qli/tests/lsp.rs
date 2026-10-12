//! Binary-level tests of `qli lsp`: the real process, real transports.
//!
//! The protocol itself is covered in-process in `crates/qli-lsp/tests/`.
//! These pin what only the binary can get wrong — the output-stream contract:
//!
//!   - `--stdio`: stdout is LSP frames and **nothing else**, even when
//!     startup would otherwise warn;
//!   - `--tcp`: stdout **and** stderr stay silent; logs go to
//!     `$XDG_STATE_HOME/qli/lsp.log`;
//!   - the server exits on `exit`, and on SIGTERM.
//!
//! Hermetic: every test runs in an `XdgSandbox`; the only network is a
//! loopback socket to the child.

#![cfg(unix)]

mod common;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use common::XdgSandbox;
use serde_json::{json, Value};
use serial_test::serial;

/// Upper bound on any single wait. Generous: it only matters on failure.
const TIMEOUT: Duration = Duration::from_secs(20);

fn fixture_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root is two levels up from crates/qli")
        .join("tests/fixtures")
        .join(relative)
}

/// Stage a group directory whose name shadows a built-in. Every other `qli`
/// invocation prints a `warning: group … shadows a built-in` line for it at
/// startup — the kind of output the `lsp` path must not emit.
fn stage_warning_trigger(sandbox: &XdgSandbox) {
    std::fs::create_dir_all(sandbox.extensions_dir().join("analyze")).expect("create group dir");
}

/// Positive control: the staged directory really does make an ordinary `qli`
/// invocation warn on stderr, so "no warning from `qli lsp`" is evidence and
/// not a fixture that never fires.
#[test]
#[serial]
fn staged_trigger_warns_on_other_subcommands() {
    let sandbox = XdgSandbox::new();
    stage_warning_trigger(&sandbox);
    let output = Command::new(assert_cmd::cargo::cargo_bin("qli"))
        .args(["ext", "list"])
        .output()
        .expect("run qli ext list");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("shadows a built-in"), "{stderr}");
}

fn spawn_lsp(args: &[&str]) -> Child {
    Command::new(assert_cmd::cargo::cargo_bin("qli"))
        .arg("lsp")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn qli lsp")
}

fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
}

fn initialize() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": { "capabilities": {} },
    })
}

fn did_open(uri: &str, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {
            "textDocument": { "uri": uri, "languageId": "python", "version": 1, "text": text },
        },
    })
}

fn shutdown() -> Value {
    json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown" })
}

fn exit() -> Value {
    json!({ "jsonrpc": "2.0", "method": "exit" })
}

/// Read one `Content-Length`-framed message. `Ok(None)` at a clean end of
/// stream; `Err` describes anything that is not a well-formed frame.
fn read_frame(reader: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut content_length = None;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|e| e.to_string())?;
        if read == 0 {
            return if content_length.is_none() {
                Ok(None)
            } else {
                Err("stream ended inside a frame header".to_owned())
            };
        }
        let Some(line) = line.strip_suffix("\r\n") else {
            return Err(format!("header line without CRLF: {line:?}"));
        };
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(": ")
            .ok_or_else(|| format!("not an LSP header: {line:?}"))?;
        match name {
            "Content-Length" => {
                content_length = Some(value.parse::<usize>().map_err(|e| e.to_string())?);
            }
            "Content-Type" => {}
            other => return Err(format!("unexpected header `{other}`")),
        }
    }
    let length = content_length.ok_or("frame without Content-Length")?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| format!("frame body is not JSON: {e}"))
}

/// Parse a stream into frames on a background thread, so the test can wait
/// for each with a timeout. The channel closes at end of stream; a malformed
/// byte anywhere arrives as an `Err`.
fn frames(stream: impl Read + Send + 'static) -> Receiver<Result<Value, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stream);
        loop {
            match read_frame(&mut reader) {
                Ok(Some(message)) => {
                    if tx.send(Ok(message)).is_err() {
                        return;
                    }
                }
                Ok(None) => return,
                Err(err) => {
                    let _ = tx.send(Err(err));
                    return;
                }
            }
        }
    });
    rx
}

fn next_frame(frames: &Receiver<Result<Value, String>>) -> Value {
    frames
        .recv_timeout(TIMEOUT)
        .expect("a frame arrives before the timeout")
        .expect("output is a well-formed LSP frame")
}

/// Assert the stream ended with nothing after the frames already consumed.
fn assert_stream_ended(frames: &Receiver<Result<Value, String>>) {
    match frames.recv_timeout(TIMEOUT) {
        Err(mpsc::RecvTimeoutError::Disconnected) => {}
        other => panic!("expected end of stream, got {other:?}"),
    }
}

fn wait_for_exit(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            return status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("qli lsp did not exit within {TIMEOUT:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_all(stream: Option<impl Read>) -> String {
    let mut out = String::new();
    stream
        .expect("stream was piped")
        .read_to_string(&mut out)
        .expect("read stream");
    out
}

#[test]
#[serial]
fn stdio_roundtrip_keeps_stdout_to_lsp_frames() {
    let sandbox = XdgSandbox::new();
    stage_warning_trigger(&sandbox);
    let path = fixture_path("python/sample.py");
    let uri = format!("file://{}", path.display());
    let text = std::fs::read_to_string(&path).expect("read fixture");

    let mut child = spawn_lsp(&["--stdio"]);
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = frames(child.stdout.take().expect("stdout piped"));

    stdin.write_all(&frame(&initialize())).unwrap();
    let response = next_frame(&stdout);
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "qli");
    assert_eq!(
        response["result"]["capabilities"]["definitionProvider"],
        true
    );

    stdin.write_all(&frame(&did_open(&uri, &text))).unwrap();
    let published = next_frame(&stdout);
    assert_eq!(published["method"], "textDocument/publishDiagnostics");
    assert_eq!(published["params"]["uri"], uri);
    let codes: Vec<&str> = published["params"]["diagnostics"]
        .as_array()
        .expect("diagnostics array")
        .iter()
        .map(|d| d["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["todo", "fixme", "xxx", "hack"]);

    stdin.write_all(&frame(&shutdown())).unwrap();
    let response = next_frame(&stdout);
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"], Value::Null);

    // `exit` ends the process even though stdin is still open.
    stdin.write_all(&frame(&exit())).unwrap();
    stdin.flush().unwrap();
    let status = wait_for_exit(&mut child);
    assert!(status.success(), "{status}");
    drop(stdin);

    // Every byte of stdout was consumed as a frame above; nothing trails.
    assert_stream_ended(&stdout);
    assert_eq!(read_all(child.stderr.take()), "", "stderr at default level");
}

#[test]
#[serial]
fn stdio_logs_to_stderr_never_stdout() {
    let sandbox = XdgSandbox::new();
    stage_warning_trigger(&sandbox);

    let mut child = spawn_lsp(&["--stdio", "-vv"]);
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = frames(child.stdout.take().expect("stdout piped"));

    stdin.write_all(&frame(&initialize())).unwrap();
    assert_eq!(next_frame(&stdout)["id"], 1);
    let initialized = json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} });
    stdin.write_all(&frame(&initialized)).unwrap();
    // Closing stdin without `exit` also ends the session.
    drop(stdin);

    assert!(wait_for_exit(&mut child).success());
    assert_stream_ended(&stdout);
    let stderr = read_all(child.stderr.take());
    assert!(stderr.contains("LSP session initialized"), "{stderr}");
    assert!(
        !stderr.contains('\u{1b}'),
        "piped stderr has no ANSI: {stderr}"
    );
    assert!(!stderr.contains("shadows a built-in"), "{stderr}");
}

#[test]
#[serial]
fn tcp_roundtrip_keeps_stdout_and_stderr_silent() {
    let sandbox = XdgSandbox::new();
    stage_warning_trigger(&sandbox);
    // Ask the OS for a free loopback port, then hand it to the server.
    let port = TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .expect("reserve a loopback port")
        .port();

    let mut child = spawn_lsp(&["--tcp", &port.to_string(), "-v"]);
    let deadline = Instant::now() + TIMEOUT;
    let mut stream = loop {
        match TcpStream::connect(("127.0.0.1", port)) {
            Ok(stream) => break stream,
            Err(err) => {
                assert!(Instant::now() < deadline, "server never listened: {err}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    let incoming = frames(stream.try_clone().expect("clone socket"));

    stream.write_all(&frame(&initialize())).unwrap();
    assert_eq!(next_frame(&incoming)["result"]["serverInfo"]["name"], "qli");
    stream.write_all(&frame(&shutdown())).unwrap();
    assert_eq!(next_frame(&incoming)["id"], 2);
    stream.write_all(&frame(&exit())).unwrap();

    assert!(wait_for_exit(&mut child).success());
    assert_eq!(read_all(child.stdout.take()), "", "stdout");
    assert_eq!(read_all(child.stderr.take()), "", "stderr");

    let log = std::fs::read_to_string(sandbox.state_dir().join("lsp.log")).expect("read lsp.log");
    assert!(log.contains("listening for an LSP client"), "{log}");
    assert!(
        !log.contains('\u{1b}'),
        "log file has no ANSI escapes: {log}"
    );
}

#[test]
#[serial]
fn sigterm_stops_the_server() {
    use nix::sys::signal::{kill, Signal};
    use nix::unistd::Pid;
    use std::os::unix::process::ExitStatusExt;

    let _sandbox = XdgSandbox::new();
    let mut child = spawn_lsp(&["--stdio"]);
    let mut stdin = child.stdin.take().expect("stdin piped");
    let stdout = frames(child.stdout.take().expect("stdout piped"));
    // A completed roundtrip proves the server is up before it is signalled.
    stdin.write_all(&frame(&initialize())).unwrap();
    assert_eq!(next_frame(&stdout)["id"], 1);

    let pid = Pid::from_raw(i32::try_from(child.id()).expect("pid fits in i32"));
    kill(pid, Signal::SIGTERM).expect("send SIGTERM");

    let status = wait_for_exit(&mut child);
    assert_eq!(status.signal(), Some(Signal::SIGTERM as i32), "{status}");
}
