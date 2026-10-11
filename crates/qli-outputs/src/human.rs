//! Human-readable terminal output: `file:line:col` prefixes, severity colors,
//! and one line per finding.
//!
//! Styling is written unconditionally as ANSI via `anstyle`; the binary wraps
//! its stdout in an `anstream` auto-stream that strips the escapes when the
//! destination isn't a TTY or `NO_COLOR` is set. So this module never has to
//! know whether color is wanted.

use std::io::{self, Write};

use anstyle::{AnsiColor, Style};
use qli_core::{AnalysisResult, Diagnostic, Reference, Severity, Symbol, SymbolKind};

/// Write `result` for `file` in human form. Positions are converted to the
/// 1-based `line:col` humans expect (the engine stores them 0-based).
pub fn write(w: &mut dyn Write, file: &str, result: &AnalysisResult) -> io::Result<()> {
    for d in &result.diagnostics {
        write_diagnostic(w, file, d)?;
    }
    for s in &result.symbols {
        write_symbol(w, file, s)?;
    }
    for r in &result.references {
        write_reference(w, file, r)?;
    }
    Ok(())
}

fn write_diagnostic(w: &mut dyn Write, file: &str, d: &Diagnostic) -> io::Result<()> {
    let style = severity_style(d.severity);
    write!(
        w,
        "{file}:{}:{}: ",
        d.range.start.line + 1,
        d.range.start.column + 1
    )?;
    write!(w, "{style}{}{style:#}", severity_label(d.severity))?;
    write!(w, " {}", d.message)?;
    if let Some(code) = &d.code {
        let dim = Style::new().dimmed();
        write!(w, " {dim}[{code}]{dim:#}")?;
    }
    writeln!(w)
}

fn write_symbol(w: &mut dyn Write, file: &str, s: &Symbol) -> io::Result<()> {
    let style = AnsiColor::Green.on_default();
    writeln!(
        w,
        "{file}:{}:{}: {style}def{style:#} {} {}",
        s.range.start.line + 1,
        s.range.start.column + 1,
        symbol_kind_label(s.kind),
        s.name,
    )
}

fn write_reference(w: &mut dyn Write, file: &str, r: &Reference) -> io::Result<()> {
    let style = AnsiColor::Blue.on_default();
    let target = match &r.resolved {
        Some(id) => format!("-> {id}"),
        None => "(unresolved)".to_string(),
    };
    writeln!(
        w,
        "{file}:{}:{}: {style}ref{style:#} {} {}",
        r.range.start.line + 1,
        r.range.start.column + 1,
        r.name,
        target,
    )
}

fn severity_style(severity: Severity) -> Style {
    let color = match severity {
        Severity::Error => AnsiColor::Red,
        Severity::Warning => AnsiColor::Yellow,
        Severity::Info => AnsiColor::Blue,
        Severity::Hint => AnsiColor::Cyan,
    };
    color.on_default().bold()
}

fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
        Severity::Hint => "hint",
    }
}

fn symbol_kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Function => "function",
        SymbolKind::Method => "method",
        SymbolKind::Class => "class",
        SymbolKind::Variable => "variable",
    }
}
