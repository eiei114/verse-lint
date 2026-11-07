//! Filesystem orchestration: diagnostics change to candidate positions only
//! after a successful save, never after analysis or staging alone.
//!
//! `--fix --diff` shares the analysis half of this module and stops there: the
//! candidate plans are validated exactly as a write run validates them, the
//! rendered unified diff replaces the write stage, and no path is opened for
//! replacement, so a preview can report the change a read-only file would
//! refuse to save.
use crate::{diff, files, fix, report::Report, rules, source::Source, write};
use std::{collections::BTreeSet, path::PathBuf};

struct Candidate {
    label: String,
    snapshot: Option<write::Snapshot>,
    plan: fix::Plan,
}

/// The two report-shaping choices a fix run takes. They are bundled so the run
/// signature stays one argument per concern instead of growing a fifth boolean.
#[derive(Clone, Copy, Debug)]
pub struct Mode {
    pub show_suppressed: bool,
    /// `--fix --diff`: validate the plan, render it, write nothing.
    pub preview: bool,
}

pub fn run(
    paths: Vec<PathBuf>,
    root: &std::path::Path,
    active: &BTreeSet<String>,
    severities: &rules::SeverityOverrides,
    options: rules::Options,
    mode: Mode,
    report: &mut Report,
) {
    report.show_suppressed = mode.show_suppressed;
    // Claim stdout for the diff before analysis runs, so a failed preview still
    // prints no diagnostic there. `Some("")` with no rendered text is the
    // documented "nothing to change" answer.
    if mode.preview {
        report.diff = Some(String::new());
    }
    let mut candidates = Vec::new();
    analyse(
        paths,
        root,
        active,
        severities,
        options,
        report,
        &mut candidates,
    );
    if !report.execution_errors.is_empty() {
        return;
    }
    if mode.preview {
        preview(&mut candidates, report);
    } else {
        write_using(&mut candidates, report, |_, pending| pending.commit());
    }
}

/// Collect one validated plan per readable file. A failure before the write
/// stage stops the whole run, so no file is changed because of a partial plan.
fn analyse(
    paths: Vec<PathBuf>,
    root: &std::path::Path,
    active: &BTreeSet<String>,
    severities: &rules::SeverityOverrides,
    options: rules::Options,
    report: &mut Report,
    candidates: &mut Vec<Candidate>,
) {
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
            if files::total_bytes_exceeded(total) {
                return Err("inputs exceed the 64 MiB total limit; no fixes written".into());
            }
            let source = Source::from_bytes(&snapshot.bytes).map_err(|e| e.to_string())?;
            let budget = rules::MAX_DIAGNOSTICS - report.budget_used;
            let plan = fix::plan(&source, &label, active, severities, options, budget)
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
                // Uncommitted files keep inspected-original findings, which is
                // what the failure message promises; a successful save replaces
                // them with the saved source's findings below.
                report.record(&candidate.plan.original);
                candidates.push(candidate);
            }
            Err(e) => report.error(Some(label), e),
        }
        if files::total_bytes_exceeded(total) {
            break;
        }
    }
}

/// Render every planned change and swap the diagnostics to candidate positions,
/// exactly as a successful save would, without touching the filesystem.
fn preview(candidates: &mut [Candidate], report: &mut Report) {
    let mut rendered = String::new();
    for candidate in candidates.iter_mut() {
        let snapshot = candidate
            .snapshot
            .take()
            .expect("snapshot owned until preview");
        if snapshot.bytes == candidate.plan.output.as_bytes() {
            continue;
        }
        let before = std::str::from_utf8(&snapshot.bytes).expect("source was validated as UTF-8");
        rendered.push_str(&diff::render(
            &candidate.label,
            before,
            &candidate.plan.output,
        ));
        report.replace(
            &candidate.label,
            &candidate.plan.original,
            &candidate.plan.after,
        );
    }
    report.diff = Some(rendered);
}
fn write_using(
    candidates: &mut [Candidate],
    report: &mut Report,
    mut commit: impl FnMut(&str, write::Pending) -> Result<Option<String>, String>,
) {
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
                report.replace(
                    &candidate.label,
                    &candidate.plan.original,
                    &candidate.plan.after,
                );
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

    fn plan_of(dir: &std::path::Path, name: &str) -> (Vec<Candidate>, Report) {
        let active = rules::Settings::default().active().unwrap();
        let mut report = Report::default();
        let mut candidates = Vec::new();
        analyse(
            vec![dir.join(name)],
            dir,
            &active,
            &rules::SeverityOverrides::new(),
            rules::Options::default(),
            &mut report,
            &mut candidates,
        );
        assert!(
            report.execution_errors.is_empty(),
            "{:?}",
            report.execution_errors
        );
        (candidates, report)
    }

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
        let mut candidates = Vec::new();
        analyse(
            paths.clone(),
            dir.path(),
            &active,
            &rules::SeverityOverrides::new(),
            rules::Options::default(),
            &mut report,
            &mut candidates,
        );
        write_using(&mut candidates, &mut report, |label, pending| {
            if label == "b.verse" {
                Err("injected commit failure".into())
            } else {
                pending.commit()
            }
        });
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

    #[test]
    fn preview_renders_the_candidate_bytes_and_leaves_the_file_alone() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.verse"), b"A := 1  ").unwrap();
        let (mut candidates, mut report) = plan_of(dir.path(), "a.verse");
        preview(&mut candidates, &mut report);
        assert_eq!(
            report.diff.as_deref(),
            Some(
                "--- a/a.verse\n+++ b/a.verse\n@@ -1,1 +1,1 @@\n-A := 1  \n\\ No newline at end of file\n+A := 1\n"
            )
        );
        assert_eq!(report.summary.files_changed, 0);
        assert_eq!(
            std::fs::read(dir.path().join("a.verse")).unwrap(),
            b"A := 1  "
        );
        // The report describes the candidate, not the inspected original.
        assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    }
}
