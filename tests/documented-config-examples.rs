//! The `[lint.severity]` examples in the docs are copy-paste starting points, so
//! they must actually produce the behaviour their prose claims. Severity and
//! selection are independent: promoting V2001 to `"error"` only fails a run when
//! V2001 is also selected. Each documented example is executed here, so an
//! example that forgets `select` fails the suite instead of misleading a reader.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// One V2001-only finding: single line, over the 120-column default and with no
/// trailing whitespace and no missing final newline.
const LONG_LINE: &str = "# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";

/// The first ```toml fenced block of a document — the example a reader copies.
fn first_toml_block(document: &str, name: &str) -> String {
    let mut lines = document.lines();
    lines
        .by_ref()
        .find(|line| line.trim() == "```toml")
        .unwrap_or_else(|| panic!("{name} has no ```toml example to verify"));
    let body: Vec<&str> = lines
        .by_ref()
        .take_while(|line| line.trim() != "```")
        .collect();
    assert!(!body.is_empty(), "{name}: the toml example is empty");
    let mut block = body.join("\n");
    block.push('\n');
    block
}

fn run(root: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(root)
        .args(["--output-format=json"])
        .output()
        .unwrap()
}

fn documented_example_builds_the_claimed_hard_failure(name: &str, document: &str) {
    let dir = tempfile::tempdir().unwrap();
    let config = first_toml_block(document, name);
    fs::write(dir.path().join("verse.toml"), &config).unwrap();
    fs::write(dir.path().join("long.verse"), LONG_LINE).unwrap();

    let out = run(dir.path());
    let report: serde_json::Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{name}: {e}: {}", String::from_utf8_lossy(&out.stderr)));

    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        1,
        "{name}: the example must select exactly the long line's V2001 finding: {config}\n{report}"
    );
    assert_eq!(diagnostics[0]["ruleId"], "V2001", "{name}: {report}");
    assert_eq!(
        diagnostics[0]["severity"], "error",
        "{name}: the promoted severity must reach the finding: {report}"
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "{name}: a promoted unselected rule is inactive, so this exit code proves the example also selects V2001: {config}\n{report}"
    );

    // The severity table itself must stay the tool default even for a promoted rule.
    let sarif = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args(["--output-format=sarif"])
        .output()
        .unwrap();
    let sarif: serde_json::Value = serde_json::from_slice(&sarif.stdout).unwrap();
    let driver = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V2001")
        .unwrap();
    assert_eq!(
        driver["defaultConfiguration"]["level"], "warning",
        "{name}: SARIF driver metadata documents the tool default, not the project policy"
    );
}

#[test]
fn readme_hard_failure_example_really_fails_the_run() {
    documented_example_builds_the_claimed_hard_failure("README.md", include_str!("../README.md"));
}

#[test]
fn rules_guide_severity_example_really_fails_the_run() {
    documented_example_builds_the_claimed_hard_failure(
        "docs/rules.md",
        include_str!("../docs/rules.md"),
    );
}

#[test]
fn cli_configuration_example_really_fails_the_run() {
    documented_example_builds_the_claimed_hard_failure(
        "docs/cli.md",
        include_str!("../docs/cli.md"),
    );
}

/// Guards the regression directly: the prose is explicit that severity alone
/// never activates a rule, and each example has to carry its own `select`.
#[test]
fn every_severity_example_documents_the_selection_it_depends_on() {
    for (name, document) in [
        ("README.md", include_str!("../README.md")),
        ("docs/rules.md", include_str!("../docs/rules.md")),
        ("docs/cli.md", include_str!("../docs/cli.md")),
    ] {
        let config = first_toml_block(document, name);
        assert!(
            config.contains("[lint.severity]"),
            "{name}: expected the severity example"
        );
        let selected = config
            .lines()
            .filter_map(|line| line.split_once("select =").map(|(_, rest)| rest))
            .any(|rest| rest.contains("\"V2001\""));
        assert!(
            selected,
            "{name}: an example that promotes V2001 must also select it:\n{config}"
        );
    }
}
