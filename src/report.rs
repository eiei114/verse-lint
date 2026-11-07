use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::{
    cli::{Color, OutputFormat},
    diagnostic::Diagnostic,
    rules::{self, Severity},
};

#[derive(Debug, Serialize)]
pub struct ExecutionError {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub message: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub files_checked: usize,
    pub files_changed: usize,
    pub errors: usize,
    pub warnings: usize,
    pub suppressed: usize,
    pub complete: bool,
}

/// Appended to a suppressed finding's text line so it stays visible without
/// reading as an active finding. No active line can contain it, because rule
/// messages never do; `docs/cli.md` documents it as the stable marker.
pub const SUPPRESSED_MARKER: &str = " (suppressed)";

#[derive(Serialize)]
pub struct Tool {
    name: &'static str,
    version: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    schema_version: u32,
    tool: Tool,
    pub diagnostics: Vec<Diagnostic>,
    pub execution_errors: Vec<ExecutionError>,
    pub summary: Summary,
    #[serde(skip)]
    pub stdin: bool,
    #[serde(skip)]
    pub source_root: Option<String>,
    #[serde(skip)]
    pub exit_code: u8,
    /// Findings already taken from the 10,000-diagnostic budget, suppressed or
    /// not. Suppressed findings enter `diagnostics` only under
    /// `--show-suppressed`, so the budget cannot be derived from the list alone.
    #[serde(skip)]
    pub budget_used: usize,
    /// Reporting policy, not data: whether suppressed findings are listed. It
    /// never changes what is analyzed, what the budget counts or the exit code.
    #[serde(skip)]
    pub show_suppressed: bool,
    /// A rendered `--fix --diff` preview, printed on stdout ahead of nothing else.
    /// `None` means the run did not ask for a preview; `Some("")` means it asked
    /// and the plan changes nothing. While a preview owns stdout, the text
    /// diagnostics a report would print there move to stderr, so stdout stays
    /// one format.
    #[serde(skip)]
    pub diff: Option<String>,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            schema_version: 1,
            tool: Tool {
                name: "verse-lint",
                version: env!("CARGO_PKG_VERSION"),
            },
            diagnostics: vec![],
            execution_errors: vec![],
            summary: Summary::default(),
            stdin: false,
            source_root: None,
            exit_code: 0,
            budget_used: 0,
            show_suppressed: false,
            diff: None,
        }
    }
}

impl Report {
    pub fn error(&mut self, path: Option<String>, message: impl Into<String>) {
        self.execution_errors.push(ExecutionError {
            path,
            message: message.into(),
        });
    }

    /// Record one analyzed source's findings. A suppressed finding consumes the
    /// same budget a reported one does and always counts in
    /// `summary.suppressed`; it reaches `diagnostics` only when the caller asked
    /// for suppressed findings, where its `suppressed` flag marks it.
    pub fn record(&mut self, result: &rules::LintResult) {
        self.budget_used += result.diagnostics.len() + result.suppressed_count();
        self.summary.suppressed += result.suppressed_count();
        self.diagnostics.extend(result.diagnostics.iter().cloned());
        if self.show_suppressed {
            self.diagnostics.extend(result.suppressed.iter().cloned());
        }
    }

    /// Replace one path's recorded findings after a successful save. Like
    /// reported findings, shown suppressed findings describe the saved source.
    pub fn replace(&mut self, path: &str, before: &rules::LintResult, after: &rules::LintResult) {
        self.budget_used = self.budget_used - before.diagnostics.len() - before.suppressed_count()
            + after.diagnostics.len()
            + after.suppressed_count();
        self.summary.suppressed =
            self.summary.suppressed - before.suppressed_count() + after.suppressed_count();
        self.diagnostics.retain(|d| d.path != path);
        self.diagnostics.extend(after.diagnostics.iter().cloned());
        if self.show_suppressed {
            self.diagnostics.extend(after.suppressed.iter().cloned());
        }
    }

    pub fn finish(&mut self, deny_warnings: bool) -> u8 {
        self.diagnostics.sort_by(|a, b| {
            (&a.path, a.range.start.byte_offset, a.rule_id).cmp(&(
                &b.path,
                b.range.start.byte_offset,
                b.rule_id,
            ))
        });
        // Suppressed findings are visible but never severity-bearing: they must
        // not reach `errors`/`warnings`, `--deny-warnings` or the exit code.
        self.summary.errors = self
            .diagnostics
            .iter()
            .filter(|d| !d.suppressed && d.severity == Severity::Error)
            .count();
        self.summary.warnings = self
            .diagnostics
            .iter()
            .filter(|d| !d.suppressed && d.severity == Severity::Warning)
            .count();
        self.summary.complete = self.execution_errors.is_empty();
        self.exit_code = if !self.summary.complete {
            2
        } else {
            u8::from(self.summary.errors > 0 || (deny_warnings && self.summary.warnings > 0))
        };
        self.exit_code
    }

    pub fn emit(&self, format: OutputFormat, color: Color) -> Result<(), String> {
        let mut out = std::io::stdout().lock();
        // A preview is text-only by contract: every other format is rejected
        // before a plan is collected, so this condition never mixes two formats
        // on stdout.
        let preview = matches!(format, OutputFormat::Text) && self.diff.is_some();
        if preview && let Some(diff) = &self.diff {
            out.write_all(diff.as_bytes()).map_err(|e| e.to_string())?;
        }
        match format {
            OutputFormat::Json => {
                serde_json::to_writer_pretty(&mut out, self).map_err(|e| e.to_string())?;
                writeln!(out).map_err(|e| e.to_string())?;
            }
            OutputFormat::Text => {
                let colored = match color {
                    Color::Always => true,
                    Color::Never => false,
                    Color::Auto => std::env::var_os("NO_COLOR").is_none() && out.is_terminal(),
                };
                // In preview mode stdout is the diff, so the findings go to
                // stderr and the terminal still shows both. The summary line
                // below already uses stderr in text mode.
                let mut stderr;
                let listing: &mut dyn Write = if preview {
                    stderr = std::io::stderr().lock();
                    &mut stderr
                } else {
                    &mut out
                };
                for diagnostic in &self.diagnostics {
                    let (begin, end) = if colored {
                        ("\x1b[31m", "\x1b[0m")
                    } else {
                        ("", "")
                    };
                    writeln!(
                        listing,
                        "{begin}{}:{}:{}: {} {} {}{}{end}",
                        diagnostic.path,
                        diagnostic.range.start.line,
                        diagnostic.range.start.column,
                        diagnostic.severity,
                        diagnostic.rule_id,
                        diagnostic.message,
                        if diagnostic.suppressed {
                            SUPPRESSED_MARKER
                        } else {
                            ""
                        }
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            OutputFormat::Sarif => {
                serde_json::to_writer_pretty(&mut out, &crate::sarif::render(self))
                    .map_err(|e| e.to_string())?;
                writeln!(out).map_err(|e| e.to_string())?;
            }
            // Colors never apply here: a workflow command carries no ANSI. This
            // also keeps `--color=always` deterministic in a machine format.
            OutputFormat::Github => crate::github::emit(self, &mut out)?,
        }
        if !self.execution_errors.is_empty() {
            let mut err = std::io::stderr().lock();
            for error in &self.execution_errors {
                writeln!(
                    err,
                    "verse-lint: {}{}{}",
                    error.path.as_deref().unwrap_or(""),
                    if error.path.is_some() { ": " } else { "" },
                    error.message
                )
                .map_err(|e| e.to_string())?;
            }
        }
        if matches!(format, OutputFormat::Text)
            && (self.summary.files_changed > 0 || self.summary.suppressed > 0)
        {
            writeln!(
                std::io::stderr().lock(),
                "checked {} file(s), changed {}, suppressed {} diagnostic(s)",
                self.summary.files_checked,
                self.summary.files_changed,
                self.summary.suppressed
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
