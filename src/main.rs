mod cli;
mod config;
mod diagnostic;
mod files;
mod lex;
mod report;
mod rules;
mod source;
mod syntax;

use std::{
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;

fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    let cli = match cli::Cli::try_parse_from(&args) {
        Ok(cli) => cli,
        Err(error) => {
            if !error.use_stderr() {
                return ExitCode::from(if error.print().is_ok() { 0 } else { 2 });
            }
            if let Some(format) = requested_machine_format(&args) {
                let mut report = report::Report::default();
                report.error(None, error.to_string());
                report.finish(false);
                if let Err(e) = report.emit(format, cli::Color::Never) {
                    eprintln!("verse-lint: {e}");
                }
            } else {
                let _ = error.print();
            }
            return ExitCode::from(2);
        }
    };
    let mut report = report::Report::default();
    match cli.validate().and_then(|()| run(&cli, &mut report)) {
        Ok(Some(config)) => {
            return match std::io::stdout().lock().write_all(config.as_bytes()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("verse-lint: {e}");
                    ExitCode::from(2)
                }
            };
        }
        Ok(None) => (),
        Err(error) => report.error(None, error),
    }
    let code = report.finish(cli.deny_warnings);
    if let Err(e) = report.emit(cli.output_format, cli.color) {
        eprintln!("verse-lint: {e}");
        return ExitCode::from(2);
    }
    ExitCode::from(code)
}

// Recover only an unambiguous valid format on clap errors. Never scan filenames
// following '--', and never emit a machine envelope for an invalid format value.
fn requested_machine_format(args: &[OsString]) -> Option<cli::OutputFormat> {
    let mut requested = None;
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--" {
            break;
        }
        let value = if arg == "--output-format" {
            Some(args.next()?.to_str()?)
        } else {
            arg.to_str()
                .and_then(|s| s.strip_prefix("--output-format="))
        };
        if let Some(value) = value {
            if requested.is_some() {
                return None;
            }
            requested = Some(match value {
                "json" => cli::OutputFormat::Json,
                "sarif" => cli::OutputFormat::Sarif,
                _ => return None,
            });
        }
    }
    requested
}

fn run(cli: &cli::Cli, report: &mut report::Report) -> Result<Option<String>, String> {
    if cli.fix {
        return Err("safe fixes are not implemented in this slice; no files were changed".into());
    }
    if matches!(cli.output_format, cli::OutputFormat::Sarif) {
        return Err("SARIF output is not implemented in this slice".into());
    }
    let mut config = config::resolve(cli.config.as_deref(), cli.stdin_filepath.as_deref())?;
    if let Some(select) = &cli.select {
        config.settings.lint.select = select.clone();
    }
    if let Some(ignore) = &cli.ignore {
        config.settings.lint.ignore = ignore.clone();
    }
    let active = config.settings.lint.active()?;
    if cli.show_config {
        return Ok(Some(config.show()?));
    }
    if cli.paths.first().is_some_and(|p| p.as_os_str() == "-") {
        let label = match &cli.stdin_filepath {
            Some(path) => path
                .to_str()
                .ok_or("stdin path must be Unicode")?
                .replace('\\', "/"),
            None => "<stdin>".into(),
        };
        inspect(read(std::io::stdin()), &label, &active, report);
    } else {
        let paths = if cli.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            cli.paths.clone()
        };
        let discovered = files::discover(&paths, &config.root, &config.settings.files.exclude)?;
        if cli.verbose {
            for reason in discovered.excluded {
                eprintln!("{reason}");
            }
            eprintln!("{} eligible .verse file(s)", discovered.paths.len());
        }
        for error in discovered.errors {
            report.error(None, error);
        }
        let mut total = 0;
        for path in discovered.paths {
            let label = files::label(&path, &config.root)?;
            let bytes = files::guard_path(&path)
                .and_then(|()| std::fs::File::open(&path).map_err(|e| e.to_string()))
                .and_then(read);
            if let Ok(bytes) = &bytes {
                total += bytes.len();
            }
            if total > files::MAX_TOTAL_BYTES {
                report.error(
                    Some(label),
                    "inputs exceed the 64 MiB total limit; inspection incomplete",
                );
                break;
            }
            inspect(bytes, &label, &active, report);
        }
    }
    Ok(None)
}

fn read(reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take((source::MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > source::MAX_SOURCE_BYTES {
        return Err("source exceeds the 8 MiB limit".into());
    }
    Ok(bytes)
}

fn inspect(
    bytes: Result<Vec<u8>, String>,
    label: &str,
    active: &std::collections::BTreeSet<String>,
    report: &mut report::Report,
) {
    let result = bytes.and_then(|bytes| {
        let source = source::Source::from_bytes(&bytes).map_err(|e| e.to_string())?;
        rules::lint(
            &source,
            label,
            active,
            rules::MAX_DIAGNOSTICS - report.diagnostics.len(),
        )
        .map_err(|e| {
            let (line, column) = source.position(e.offset);
            format!("{line}:{column}: {}", e.message)
        })
    });
    match result {
        Ok(diagnostics) => {
            report.summary.files_checked += 1;
            report.diagnostics.extend(diagnostics);
        }
        Err(error) => report.error(Some(label.into()), error),
    }
}
