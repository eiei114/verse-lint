//! `--dump-scopes` and `--semantic-coverage`: the two run-free JSON surfaces.
//!
//! Both paths run no rule and change no guard: they read each source through the
//! same byte limits as a lint run, require the same [`syntax::Document`]
//! coverage validation, and then print versioned JSON. `--dump-scopes` adds a
//! per-file scope graph; `--semantic-coverage` prints the run summary and the
//! per-file counts it was made of, so a user can tell *how much* of the project
//! the semantic rules could have seen before trusting a clean result.

use std::path::PathBuf;

use crate::{
    cli, config, files, report,
    semantic::{self, CoverageDump, FileCoverage, GraphDump, ScopeGraph},
    source::{self, Source},
    syntax::Document,
};

/// Collects one run's graphs and coverage. Split out so the two surfaces share
/// exactly one reading, validation and walking path.
struct Collector<'a> {
    /// Present only for `--dump-scopes`; the coverage surface never builds one.
    dumps: Option<Vec<semantic::FileDump>>,
    coverages: Vec<FileCoverage>,
    report: &'a mut report::Report,
    /// `--require-complete-semantics`: an incomplete model becomes an execution
    /// error instead of information, in this path as well as in a lint run.
    strict: bool,
}

impl Collector<'_> {
    fn one(&mut self, bytes: Result<Vec<u8>, String>, label: &str) {
        let result = bytes.and_then(|bytes| {
            let source = Source::from_bytes(&bytes).map_err(|e| e.to_string())?;
            let document = Document::parse(&source).map_err(|e| positioned(&source, &e))?;
            ScopeGraph::build(&source, &document).map_err(|e| positioned(&source, &e))
        });
        match result {
            Ok(graph) => {
                if self.strict
                    && let Some(reason) = graph.incomplete_reason()
                {
                    self.report.error(Some(label.into()), reason);
                    return;
                }
                self.coverages.push(FileCoverage::new(label, &graph));
                if let Some(dumps) = &mut self.dumps {
                    dumps.push(graph.dump(label));
                }
            }
            Err(error) => self.report.error(Some(label.into()), error),
        }
    }
}

/// Returns the JSON document to print, or `None` when any input failed. A
/// failure is recorded as an execution error so the caller exits 2 without
/// emitting a partial document that could be mistaken for a complete one.
pub fn run(
    cli: &cli::Cli,
    config: &config::Config,
    report: &mut report::Report,
) -> Result<Option<String>, String> {
    let graph_surface = cli.dump_scopes;
    let mut collector = Collector {
        dumps: graph_surface.then(Vec::new),
        coverages: Vec::new(),
        report,
        strict: cli.require_complete_semantics,
    };
    if cli.paths.first().is_some_and(|p| p.as_os_str() == "-") {
        let label = match &cli.stdin_filepath {
            Some(path) => path
                .to_str()
                .ok_or("stdin path must be Unicode")?
                .replace('\\', "/"),
            None => "<stdin>".into(),
        };
        collector.one(read(std::io::stdin()), &label);
    } else {
        let paths = if cli.paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            cli.paths.clone()
        };
        // Same discovery as a lint run, including `verse-lint.ignore` (or its
        // absence under `--no-ignore-file`), so the reported file set matches.
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
            collector.report.error(None, error);
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
                collector.report.error(
                    Some(label),
                    "inputs exceed the 64 MiB total limit; inspection incomplete",
                );
                break;
            }
            collector.one(bytes, &label);
        }
    }
    if !collector.report.execution_errors.is_empty() {
        return Ok(None);
    }
    // Both surfaces add up the same per-file counts, so the aggregate summary and
    // a per-file entry can never disagree.
    let coverage = CoverageDump::new(collector.coverages);
    if graph_surface {
        let summary = coverage.summary().clone();
        let dumps = collector
            .dumps
            .ok_or("--dump-scopes collected no graph document")?;
        return Ok(Some(GraphDump::new(dumps, summary).to_json()?));
    }
    Ok(Some(coverage.to_json()?))
}

/// Strict refusal for a lint run: the position-formatted reason one source
/// cannot be reported on completely, or `None` when the model covered it.
/// Parsing failures stay the ordinary coverage refusals; the caller reports them
/// the same way.
pub fn strict_violation(bytes: &[u8]) -> Result<Option<String>, String> {
    let source = Source::from_bytes(bytes).map_err(|e| e.to_string())?;
    let document = Document::parse(&source).map_err(|e| positioned(&source, &e))?;
    let graph = ScopeGraph::build(&source, &document).map_err(|e| positioned(&source, &e))?;
    Ok(graph.incomplete_reason())
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
