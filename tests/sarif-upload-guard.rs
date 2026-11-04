//! Guards ADR 0001: this repository must never upload SARIF to code scanning.
//!
//! `docs/adr/0001-sarif-upload-policy.md` chose option (A), "no upload". That
//! decision is only worth recording if a future contributor cannot undo it by
//! quietly adding a step, so this test fails when any workflow under
//! `.github/workflows/` gains an `upload-sarif` step or a `security-events`
//! permission. It is a text guard, not a YAML parser: it deliberately does not
//! care whether the offending line is live or commented out, because a
//! half-enabled upload is exactly the accident this is meant to catch.

use std::fs;
use std::path::{Path, PathBuf};

const ADR: &str = "docs/adr/0001-sarif-upload-policy.md";

fn workflows_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/workflows")
}

fn workflow_files() -> Vec<PathBuf> {
    let dir = workflows_dir();
    let entries = fs::read_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "cannot read {}: {e}. The SARIF upload guard must keep a directory to guard; \
             see {ADR}.",
            dir.display()
        )
    });
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
        })
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no workflow files found under {}. The SARIF upload guard would silently pass; \
         see {ADR}.",
        dir.display()
    );
    files
}

/// Returns the first offending line, or `None` when the workflow is clean.
fn offending_line(contents: &str) -> Option<String> {
    contents
        .lines()
        .map(str::trim)
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            // The upload action, plus the permission it requires. Either one alone is
            // enough to widen what a style linter's CI job can touch.
            lower.contains("upload-sarif")
                || (lower.contains("security-events") && !lower.starts_with('#'))
        })
        .map(str::to_owned)
}

#[test]
fn no_workflow_uploads_sarif_or_requests_security_events_write() {
    let mut violations = Vec::new();
    for path in workflow_files() {
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        if let Some(line) = offending_line(&contents) {
            violations.push(format!("{}: {line}", path.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "This repository does not upload SARIF to GitHub code scanning. \
         {ADR} chose option (A) (no upload) and records the trade-offs: what becomes public, \
         the security-events: write permission, style-linter result volume, false positives \
         visible to third parties, and error-level style findings masquerading as security \
         alerts. Remove the offending step or permission instead of weakening the guard; \
         changing the policy requires amending the ADR in the same pull request.\n\n{}",
        violations.join("\n")
    );
}

#[test]
fn guard_detects_each_prohibited_form() {
    // A guard that cannot fail is not a guard. These are the shapes a future
    // contributor would actually write, including the pinned-SHA `uses:` form
    // this repository prefers for actions.
    for sample in [
        "      - uses: github/codeql-action/upload-sarif@v3",
        "      - uses: github/codeql-action/upload-sarif@0123456789abcdef",
        "    uses: github/codeql-action/upload-SARIF@v3",
        "permissions:\n  security-events: write\n  contents: read",
        "  security-events: write",
    ] {
        assert!(
            offending_line(sample).is_some(),
            "guard missed a prohibited shape: {sample}"
        );
    }
}

#[test]
fn guard_allows_ordinary_read_only_permissions() {
    for sample in [
        "permissions:\n  contents: read",
        "          --output-format sarif",
        "      - name: Validate SARIF 2.1.0 output",
        "# security-events is intentionally not requested; see docs/adr/0001-sarif-upload-policy.md",
    ] {
        assert!(
            offending_line(sample).is_none(),
            "guard rejected a permitted shape: {sample}"
        );
    }
}
