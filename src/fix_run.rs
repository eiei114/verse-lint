//! Filesystem orchestration: diagnostics change to candidate positions only
//! after a successful save, never after analysis or staging alone.
use crate::{files, fix, report::Report, rules, source::Source, write};
use std::{collections::BTreeSet, path::PathBuf};

struct Candidate {
    label: String,
    snapshot: Option<write::Snapshot>,
    plan: fix::Plan,
}

pub fn run(
    paths: Vec<PathBuf>,
    root: &std::path::Path,
    active: &BTreeSet<String>,
    max_line_length: usize,
    report: &mut Report,
) {
    run_using(
        paths,
        root,
        active,
        max_line_length,
        report,
        |_, pending| pending.commit(),
    );
}

fn run_using(
    paths: Vec<PathBuf>,
    root: &std::path::Path,
    active: &BTreeSet<String>,
    max_line_length: usize,
    report: &mut Report,
    mut commit: impl FnMut(&str, write::Pending) -> Result<Option<String>, String>,
) {
    let mut candidates = Vec::new();
    let mut total = 0;
    for path in paths {
        let label = match files::label(&path, root) {
            Ok(label) => label,
            Err(e) => {
                report.error(None, e);
                continue;
            }
        };
        let result: Result<Candidate, String> = (|| {
            let snapshot = write::Snapshot::read(&path)?;
            total += snapshot.bytes.len();
            if total > files::MAX_TOTAL_BYTES {
                return Err("inputs exceed the 64 MiB total limit; no fixes written".into());
            }
            let source = Source::from_bytes(&snapshot.bytes).map_err(|e| e.to_string())?;
            let budget =
                rules::MAX_DIAGNOSTICS - report.diagnostics.len() - report.summary.suppressed;
            let plan = fix::plan(&source, &label, active, max_line_length, budget)
                .map_err(|e| e.to_string())?;
            Ok(Candidate {
                label: label.clone(),
                snapshot: Some(snapshot),
                plan,
            })
        })();
        match result {
            Ok(candidate) => {
                report.summary.files_checked += 1;
                report.summary.suppressed += candidate.plan.original.suppressed;
                report
                    .diagnostics
                    .extend(candidate.plan.original.diagnostics.clone());
                candidates.push(candidate);
            }
            Err(e) => report.error(Some(label), e),
        }
        if total > files::MAX_TOTAL_BYTES {
            break;
        }
    }
    if !report.execution_errors.is_empty() {
        return;
    }
    let mut pending = Vec::new();
    for (index, candidate) in candidates.iter_mut().enumerate() {
        let snapshot = candidate
            .snapshot
            .take()
            .expect("snapshot owned until preflight");
        if snapshot.bytes == candidate.plan.output.as_bytes() {
            continue;
        }
        match write::prepare(snapshot, candidate.plan.output.as_bytes()) {
            Ok(prepared) => pending.push((index, prepared)),
            Err(e) => {
                report.error(
                    Some(candidate.label.clone()),
                    format!("preflight failed; no files changed: {e}"),
                );
                return;
            }
        }
    }
    let labels: Vec<_> = pending
        .iter()
        .map(|(index, _)| candidates[*index].label.clone())
        .collect();
    for (sequence, (index, replacement)) in pending.into_iter().enumerate() {
        let candidate = &mut candidates[index];
        match commit(&candidate.label, replacement) {
            Ok(warning) => {
                if let Some(warning) = warning {
                    eprintln!("{}: {warning}", candidate.label);
                }
                report.summary.files_changed += 1;
                report.summary.suppressed = report.summary.suppressed
                    - candidate.plan.original.suppressed
                    + candidate.plan.after.suppressed;
                report.diagnostics.retain(|d| d.path != candidate.label);
                report
                    .diagnostics
                    .append(&mut candidate.plan.after.diagnostics);
            }
            Err(e) => {
                report.error(Some(candidate.label.clone()),format!("{e}; completed: {:?}; not attempted: {:?}. Uncommitted diagnostics describe inspected originals, not candidate fixes",&labels[..sequence],&labels[sequence+1..]));
                break;
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn mid_batch_failure_reports_only_persisted_fixes_and_original_uncommitted_diagnostics() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<_> = ["a.verse", "b.verse", "c.verse"]
            .into_iter()
            .map(|name| dir.path().join(name))
            .collect();
        for path in &paths {
            std::fs::write(path, b"A := 1  ").unwrap();
        }
        let active = rules::Settings::default().active().unwrap();
        let mut report = Report::default();
        run_using(
            paths.clone(),
            dir.path(),
            &active,
            120,
            &mut report,
            |label, pending| {
                if label == "b.verse" {
                    Err("injected commit failure".into())
                } else {
                    pending.commit()
                }
            },
        );
        assert_eq!(report.finish(false), 2);
        assert_eq!(report.summary.files_changed, 1);
        assert_eq!(report.summary.files_checked, 3);
        assert_eq!(std::fs::read(&paths[0]).unwrap(), b"A := 1\n");
        assert_eq!(std::fs::read(&paths[1]).unwrap(), b"A := 1  ");
        assert_eq!(std::fs::read(&paths[2]).unwrap(), b"A := 1  ");
        assert!(report.diagnostics.iter().all(|d| d.path != "a.verse"));
        assert_eq!(report.diagnostics.len(), 4);
        assert!(report.execution_errors[0].message.contains("c.verse"));
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
    }
}
