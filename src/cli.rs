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
    /// Select comma-separated complete rule IDs, replacing configured selection.
    #[arg(long, value_delimiter = ',')]
    pub select: Option<Vec<String>>,
    /// Ignore comma-separated complete rule IDs, replacing configured ignores.
    #[arg(long, value_delimiter = ',')]
    pub ignore: Option<Vec<String>>,
    /// Diagnostic format. Machine formats never contain ANSI colors.
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
    #[arg(long, conflicts_with_all = ["paths", "fix", "deny_warnings", "output_format"])]
    pub show_config: bool,
    /// Explain file exclusions on stderr.
    #[arg(long)]
    pub verbose: bool,
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
        if self.stdin_filepath.is_some() && !stdin && !self.show_config {
            return Err("--stdin-filepath requires stdin '-'".into());
        }
        Ok(())
    }
}
