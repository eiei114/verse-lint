use std::io::{IsTerminal, Write};

use serde::Serialize;

use crate::{
    cli::{Color, OutputFormat},
    diagnostic::Diagnostic,
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

    pub fn finish(&mut self, deny_warnings: bool) -> u8 {
        self.diagnostics.sort_by(|a, b| {
            (&a.path, a.range.start.byte_offset, a.rule_id).cmp(&(
                &b.path,
                b.range.start.byte_offset,
                b.rule_id,
            ))
        });
        self.summary.errors = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .count();
        self.summary.warnings = self
            .diagnostics
            .iter()
            .filter(|d| d.severity == "warning")
            .count();
        self.summary.complete = self.execution_errors.is_empty();
        if !self.summary.complete {
            2
        } else {
            u8::from(self.summary.errors > 0 || (deny_warnings && self.summary.warnings > 0))
        }
    }

    pub fn emit(&self, format: OutputFormat, color: Color) -> Result<(), String> {
        let mut out = std::io::stdout().lock();
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
                for diagnostic in &self.diagnostics {
                    let (begin, end) = if colored {
                        ("\x1b[31m", "\x1b[0m")
                    } else {
                        ("", "")
                    };
                    writeln!(
                        out,
                        "{begin}{}:{}:{}: {} {} {}{end}",
                        diagnostic.path,
                        diagnostic.range.start.line,
                        diagnostic.range.start.column,
                        diagnostic.severity,
                        diagnostic.rule_id,
                        diagnostic.message
                    )
                    .map_err(|e| e.to_string())?;
                }
            }
            OutputFormat::Sarif => {
                return Err("SARIF output is not implemented in this slice".into());
            }
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
