use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum Color {
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    Sarif,
    Github,
}

#[derive(Debug, Parser)]
#[command(
    version,
    about = "Conservative Verse style linter (not an Epic compiler)"
)]
pub struct Cli {
    /// Files/directories (default '.'); use '-' alone for UTF-8 stdin.
    pub paths: Vec<PathBuf>,
    /// Apply only validated, safe edits to files; not supported on stdin.
    #[arg(long)]
    pub fix: bool,
    /// With `--fix`, print the unified diff of every planned edit to stdout and
    /// write nothing. Diagnostics then go to stderr, because stdout carries one
    /// format.
    #[arg(long)]
    pub diff: bool,
    /// Select comma-separated complete rule IDs or rule-family prefixes (`V1`),
    /// replacing configured selection.
    #[arg(long, value_delimiter = ',')]
    pub select: Option<Vec<String>>,
    /// Ignore comma-separated complete rule IDs or rule-family prefixes (`V2`),
    /// replacing configured ignores.
    #[arg(long, value_delimiter = ',')]
    pub ignore: Option<Vec<String>>,
    /// Diagnostic format. Machine formats never contain ANSI colors;
    /// `github` prints Actions workflow commands only.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output_format: OutputFormat,
    /// Exit 1 for warnings as well as errors.
    #[arg(long)]
    pub deny_warnings: bool,
    /// Virtual filename for stdin configuration and diagnostics.
    #[arg(long)]
    pub stdin_filepath: Option<PathBuf>,
    /// Explicit verse.toml configuration file.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Print the resolved configuration without processing source files.
    #[arg(
        long,
        conflicts_with_all = [
            "paths",
            "fix",
            "diff",
            "deny_warnings",
            "output_format",
            "show_suppressed"
        ]
    )]
    pub show_config: bool,
    /// Print the per-file scope graph as JSON and run no rule. This is an
    /// UNSTABLE diagnostic surface for rule authors, not a stable public
    /// contract; its shape may change without notice.
    #[arg(long)]
    pub dump_scopes: bool,
    /// Also report findings removed by a `verse-lint:` suppression directive.
    /// They are marked as suppressed and never change the exit code.
    #[arg(long)]
    pub show_suppressed: bool,
    /// Explain file exclusions on stderr.
    #[arg(long)]
    pub verbose: bool,
    /// Ignore `verse-lint.ignore` at the configuration root for this run.
    #[arg(long)]
    pub no_ignore_file: bool,
    /// ANSI color for text output. Accepted but inert for machine formats.
    #[arg(long, value_enum, default_value_t = Color::Auto)]
    pub color: Color,
}

impl Cli {
    pub fn validate(&self) -> Result<(), String> {
        let stdin = self.paths.iter().any(|p| p.as_os_str() == "-");
        if stdin && self.paths.len() != 1 {
            return Err("stdin '-' cannot be combined with other paths".into());
        }
        if stdin && self.fix {
            return Err("--fix cannot be used with stdin".into());
        }
        if self.diff && !self.fix {
            return Err("--diff previews the fixes --fix would apply, so it requires --fix".into());
        }
        if self.diff && !matches!(self.output_format, OutputFormat::Text) {
            return Err(
                "--diff writes a unified diff to stdout; drop --output-format (only the default text setting is accepted)"
                    .into(),
            );
        }
        if self.stdin_filepath.is_some() && !stdin && !self.show_config {
            return Err("--stdin-filepath requires stdin '-'".into());
        }
        if self.dump_scopes {
            if self.fix {
                return Err("--dump-scopes cannot be combined with --fix".into());
            }
            if self.show_config {
                return Err("--dump-scopes cannot be combined with --show-config".into());
            }
            if !matches!(self.output_format, OutputFormat::Text) {
                return Err(
                    "--dump-scopes emits its own JSON; drop --output-format (only the default text setting is accepted)"
                        .into(),
                );
            }
        }
        Ok(())
    }
}
