//! Blocking entry points that run the server over a transport.
//!
//! Each builds its own single-threaded tokio runtime, so the binary stays
//! synchronous and pays for the runtime only on the `qli lsp` path.

use std::io;
use std::net::Ipv4Addr;
use std::sync::Arc;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tower_service::Service;
// tower-lsp-server swap: `tower_lsp::jsonrpc` becomes `tower_lsp_server::jsonrpc`.
use tower_lsp::jsonrpc::Request;
// tower-lsp-server swap: `tower_lsp::` becomes `tower_lsp_server::`.
use tower_lsp::{LspService, Server};

use crate::pipeline::Pipeline;
use crate::server::Backend;

/// Serve one LSP session over stdin/stdout. Returns when the client sends
/// `exit` or closes stdin.
///
/// stdout carries LSP frames only; nothing else may write to it for the life
/// of the process.
pub fn serve_stdio(pipeline: Pipeline) -> io::Result<()> {
    let runtime = runtime()?;
    runtime.block_on(serve(tokio::io::stdin(), tokio::io::stdout(), pipeline));
    // tokio reads stdin on a blocking thread that can't be cancelled; a plain
    // drop would wait on it forever when the client sent `exit` but left the
    // pipe open.
    runtime.shutdown_background();
    Ok(())
}

/// Listen on `127.0.0.1:<port>`, serve the first connection as one LSP
/// session, and return when it ends.
///
/// Loopback only: the protocol is unauthenticated and hands out document
/// contents.
pub fn serve_tcp(pipeline: Pipeline, port: u16) -> io::Result<()> {
    let runtime = runtime()?;
    runtime.block_on(async {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        tracing::info!(port, "listening for an LSP client");
        let (stream, peer) = listener.accept().await?;
        tracing::info!(%peer, "LSP client connected");
        let (read, write) = stream.into_split();
        serve(read, write, pipeline).await;
        Ok(())
    })
}

fn runtime() -> io::Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()
}

/// Run one session until the client sends `exit` or the input ends.
///
/// `tower-lsp`'s `Server::serve` only returns at end of input: after `exit`
/// it keeps waiting for a next message that a well-behaved client never
/// sends, so a client that leaves the pipe open would leave the process
/// running. [`ExitSignal`] watches for the notification and ends the session
/// as the spec requires.
async fn serve<I, O>(input: I, output: O, pipeline: Pipeline)
where
    I: AsyncRead + Unpin,
    O: AsyncWrite,
{
    let (service, socket) = LspService::new(|client| Backend::new(client, pipeline));
    let exited = Arc::new(Notify::new());
    let service = ExitSignal {
        inner: service,
        exited: Arc::clone(&exited),
    };
    tokio::select! {
        () = Server::new(input, output, socket).serve(service) => {}
        () = exited.notified() => {}
    }
}

/// Passes every message through to the wrapped service and signals `exited`
/// once the `exit` notification has been handled.
struct ExitSignal<S> {
    inner: S,
    exited: Arc<Notify>,
}

impl<S: Service<Request>> Service<Request> for ExitSignal<S> {
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let is_exit = request.method() == "exit";
        // `tower-lsp` marks the session exited synchronously inside `call`,
        // so by the time this signals there is nothing left to flush: the
        // client only sends `exit` after reading the `shutdown` response.
        let future = self.inner.call(request);
        if is_exit {
            self.exited.notify_one();
        }
        future
    }
}
