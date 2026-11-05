//! `--dump-scopes`: the unstable diagnostic surface for rule authors.
//!
//! This path runs no rule and changes no guard: it reads each source through the
//! same byte limits as a lint run, requires the same [`syntax::Document`]
//! coverage validation, and then prints the scope graph as versioned JSON.

use std::path::PathBuf;

use crate::{
    cli, config, files, report,
    semantic::{self, GraphDump, ScopeGraph},
    source::{self, Source},
    syntax::Document,
};
/// Returns the JSON document to print, or `None` when any input failed. A
/// failure is recorded as an execution error so the caller exits 2 without
/// emitting a partial graph that could be mistaken for a complete one.
pub fn run(
    cli: &cli::Cli,
    config: &config::Config,
    report: &mut report::Report,
) -> Result<Option<String>, String> {
    let mut dumps = Vec::new();
    if cli.paths.first().is_some_and(|p| p.as_os_str() == "-") {
        let label = match &cli.stdin_filepath {
            Some(path) => path
                .to_str()
                .ok_or("stdin path must be Unicode")?
                .replace('\\', "/"),
            None => "<stdin>".into(),
        };
        dump_one(read(std::io::stdin()), &label, &mut dumps, report);
    } else {
        let paths = if cli.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            cli.paths.clone()
        };
        // Same discovery as a lint run, including `verse-lint.ignore` (or its
        // absence under `--no-ignore-file`), so the dumped file set matches.
        let discovered = files::discover(
            &paths,
            &config.root,
            &config.settings.files.exclude,
            config.ignore_file.as_ref(),
        )?;
        if cli.verbose {
            for reason in discovered.excluded {
                eprintln!("{reason}");
            }
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
            dump_one(bytes, &label, &mut dumps, report);
        }
    }
    if !report.execution_errors.is_empty() {
        return Ok(None);
    }
    Ok(Some(GraphDump::new(dumps).to_json()?))
}

fn dump_one(
    bytes: Result<Vec<u8>, String>,
    label: &str,
    dumps: &mut Vec<semantic::FileDump>,
    report: &mut report::Report,
) {
    let result = bytes.and_then(|bytes| {
        let source = Source::from_bytes(&bytes).map_err(|e| e.to_string())?;
        let document = Document::parse(&source).map_err(|e| positioned(&source, &e))?;
        let graph = ScopeGraph::build(&source, &document).map_err(|e| positioned(&source, &e))?;
        Ok(graph.dump(label))
    });
    match result {
        Ok(dump) => dumps.push(dump),
        Err(error) => report.error(Some(label.into()), error),
    }
}

/// Same position formatting the lint path uses for a [`source::Failure`].
fn positioned(source: &Source, failure: &source::Failure) -> String {
    let (line, column) = source.position(failure.offset);
    format!("{line}:{column}: {}", failure.message)
}

fn read(reader: impl std::io::Read) -> Result<Vec<u8>, String> {
    use std::io::Read;
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
