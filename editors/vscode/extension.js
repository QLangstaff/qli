// VS Code client for the qli language server.
//
// The extension holds no analysis logic: it spawns `qli lsp --stdio` and lets
// `vscode-languageclient` speak LSP to it. Which documents it attaches to is
// the `documentSelector` below; which languages actually get results is
// decided by the server (by file extension), so the two lists must agree.

const vscode = require("vscode");
const { LanguageClient } = require("vscode-languageclient/node");

// Language ids VS Code assigns to the file types `qli lsp` analyzes. Keep in
// step with `activationEvents` in package.json.
const LANGUAGES = ["python", "typescript", "typescriptreact", "csharp", "html"];

/** @type {LanguageClient | undefined} */
let client;

async function activate(context) {
  const command = vscode.workspace.getConfiguration("qli").get("path", "qli");

  client = new LanguageClient(
    "qli",
    "qli",
    // stdout is the LSP channel; the server logs to stderr, which the client
    // shows in the "qli" output channel.
    { command, args: ["lsp", "--stdio"] },
    {
      // Saved files only: the server picks a language from the file
      // extension, which an untitled buffer doesn't have.
      documentSelector: LANGUAGES.map((language) => ({ scheme: "file", language })),
    },
  );
  context.subscriptions.push(client);

  try {
    await client.start();
  } catch (err) {
    vscode.window.showErrorMessage(
      `qli: could not start \`${command} lsp --stdio\` (${err.message}). ` +
        "Install qli, or set `qli.path` to a qli binary that has the `lsp` subcommand " +
        "(an older qli on PATH fails with \"unrecognized subcommand 'lsp'\"; see the qli output channel).",
    );
  }
}

function deactivate() {
  return client?.stop();
}

module.exports = { activate, deactivate };
