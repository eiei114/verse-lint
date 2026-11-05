//! `--show-suppressed` reports findings a directive removed, without changing
//! what a default run prints or exits with. Every case here drives the real
//! binary: the flag has to hold for text, JSON, SARIF, `--fix` and the exit code.
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn json(root: &Path, args: &[&str]) -> (i32, Value) {
    let out = run(root, &[args, &["--output-format=json"]].concat());
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code().unwrap(), value)
}

fn text(root: &Path, args: &[&str]) -> (i32, String) {
    let out = run(root, args);
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("verse.toml"), "schema-version = 1\n").unwrap();
    for (name, body) in files {
        fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

/// One suppressed V1001, one active V1001, and enough control that a mistake in
/// ordering, marking or counting is visible.
const MIXED: &str = concat!(
    "A := 1  \n",
    "# verse-lint: disable-next-line V1001 -- kept for the review job\n",
    "B := 2  \n",
    "C := 3  \n",
);

#[test]
fn default_output_is_unchanged_for_text_json_and_exit() {
    let dir = project(&[("a.verse", MIXED)]);
    let (code, out) = text(dir.path(), &["a.verse"]);
    assert_eq!(code, 1);
    assert_eq!(
        out,
        "a.verse:1:7: error V1001 Trailing whitespace\n\
              a.verse:4:7: error V1001 Trailing whitespace\n",
        "a default run must not print the suppressed line"
    );
    assert!(!out.contains("suppressed)"));

    let (code, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(code, 1);
    assert_eq!(report["summary"]["errors"], 2);
    assert_eq!(report["summary"]["suppressed"], 1);
    assert_eq!(report["summary"]["warnings"], 0);
    for diagnostic in report["diagnostics"].as_array().unwrap() {
        assert_eq!(
            diagnostic["suppressed"], false,
            "a default run lists active findings only"
        );
    }
}

#[test]
fn text_lists_suppressed_findings_in_source_order_with_a_stable_marker() {
    let dir = project(&[("a.verse", MIXED)]);
    let (code, out) = text(dir.path(), &["a.verse", "--show-suppressed"]);
    assert_eq!(code, 1, "the flag must not change the exit code");
    assert_eq!(
        out,
        "a.verse:1:7: error V1001 Trailing whitespace\n\
              a.verse:3:7: error V1001 Trailing whitespace (suppressed)\n\
              a.verse:4:7: error V1001 Trailing whitespace\n",
        "suppressed findings must sit at their byte offset, in the same order"
    );
    // The marker is the documented trailing text and nothing else.
    let marked: Vec<_> = out
        .lines()
        .filter(|l| l.ends_with("(suppressed)"))
        .collect();
    assert_eq!(marked.len(), 1);
    assert!(marked[0].ends_with("V1001 Trailing whitespace (suppressed)"));
}

#[test]
fn json_marks_each_finding_and_keeps_the_summary_counting_rule() {
    let dir = project(&[("a.verse", MIXED)]);
    let (code, report) = json(dir.path(), &["a.verse", "--show-suppressed"]);
    assert_eq!(code, 1);
    let suppressed: Vec<&Value> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|d| d["suppressed"] == true)
        .collect();
    assert_eq!(suppressed.len(), 1, "{report}");
    assert_eq!(suppressed[0]["range"]["start"]["line"], 3);
    assert_eq!(suppressed[0]["ruleId"], "V1001");
    assert_eq!(suppressed[0]["severity"], "error");
    // Every listed object carries the flag, not only the suppressed one.
    for diagnostic in report["diagnostics"].as_array().unwrap() {
        assert!(
            diagnostic.get("suppressed").is_some(),
            "the flag must be present on both kinds: {diagnostic}"
        );
    }
    // Active findings still count; the suppressed one only shows up here.
    assert_eq!(report["summary"]["errors"], 2);
    assert_eq!(report["summary"]["suppressed"], 1);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 3);
}

#[test]
fn every_documented_rule_family_suppresses_and_is_reported_when_asked() {
    // V1001 and V2001 are the two documented examples: a fixable error and an
    // opt-in policy warning, each removed by a next-line directive.
    let source = concat!(
        "# verse-lint: disable-next-line V1001 -- keep this padding\n",
        "A := 1  \n",
        "# verse-lint: disable-next-line V2001 -- intentional long line\n",
        "# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
        "B := 2  \n",
    );
    let dir = project(&[("a.verse", source)]);
    let (code, report) = json(
        dir.path(),
        &["a.verse", "--select", "V1001,V2001", "--show-suppressed"],
    );
    assert_eq!(code, 1, "the active V1001 still fails: {report}");
    let mut shown: Vec<(String, bool)> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["ruleId"].as_str().unwrap().to_string(),
                d["suppressed"].as_bool().unwrap(),
            )
        })
        .collect();
    shown.sort();
    assert_eq!(
        shown,
        [
            ("V1001".to_string(), false),
            ("V1001".to_string(), true),
            ("V2001".to_string(), true),
        ],
        "both families must be listed and marked: {report}"
    );
    // The suppressed warning must not become a warning.
    assert_eq!(report["summary"]["warnings"], 0);
    assert_eq!(report["summary"]["errors"], 1);
    assert_eq!(report["summary"]["suppressed"], 2);
}

#[test]
fn suppressed_findings_never_change_the_exit_code_or_deny_warnings() {
    let suppressed_only = "# verse-lint: disable-next-line V1001 -- intentional\nA := 1  \n";
    let dir = project(&[("a.verse", suppressed_only)]);
    for args in [
        vec!["a.verse"],
        vec!["a.verse", "--show-suppressed"],
        vec!["a.verse", "--deny-warnings"],
        vec!["a.verse", "--show-suppressed", "--deny-warnings"],
    ] {
        let (code, report) = json(dir.path(), &args);
        assert_eq!(code, 0, "{args:?}: {report}");
        assert_eq!(report["summary"]["suppressed"], 1, "{args:?}");
        assert_eq!(report["summary"]["errors"], 0, "{args:?}");
        assert_eq!(report["summary"]["warnings"], 0, "{args:?}");
    }

    // A suppressed warning behaves the same under `--deny-warnings`: the flag
    // reports it, the exit code stays clean.
    let dir = project(&[(
        "a.verse",
        "# verse-lint: disable-next-line V2001 -- intentional\n# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )]);
    for extra in [vec![], vec!["--show-suppressed"]] {
        let args = [
            vec!["a.verse", "--select", "V2001", "--deny-warnings"],
            extra,
        ]
        .concat();
        let (code, report) = json(dir.path(), &args);
        assert_eq!(code, 0, "{args:?}: {report}");
        assert_eq!(report["summary"]["warnings"], 0, "{args:?}");
        assert_eq!(report["summary"]["suppressed"], 1, "{args:?}");
    }
}

#[test]
fn show_config_conflict_exits_two_without_diagnostics() {
    let dir = project(&[("a.verse", MIXED)]);
    let out = run(dir.path(), &["--show-config", "--show-suppressed"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty(), "a conflict must not print a config");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("--show-suppressed"),
        "the message must name the conflicting flag: {stderr}"
    );

    // The same conflict in JSON mode is a recoverable argument failure.
    let out = run(
        dir.path(),
        &["--show-config", "--show-suppressed", "--output-format=json"],
    );
    assert_eq!(out.status.code(), Some(2));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["summary"]["complete"], false);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
    assert!(!report["executionErrors"].as_array().unwrap().is_empty());
}

#[test]
fn sarif_never_presents_a_suppressed_finding_as_a_normal_result() {
    let dir = project(&[("a.verse", MIXED)]);
    for extra in [vec![], vec!["--show-suppressed"]] {
        let args = [vec!["a.verse", "--output-format=sarif"], extra].concat();
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        let run_object = &value["runs"][0];
        let results = run_object["results"].as_array().unwrap();
        assert_eq!(
            results.len(),
            2,
            "{args:?}: suppressed findings must stay out of results"
        );
        for result in results {
            assert_eq!(result["ruleId"], "V1001");
            assert!(
                result.get("suppressions").is_none(),
                "an active result must not carry a suppression"
            );
        }
        // The count survives in the run summary; nothing is silently dropped.
        assert_eq!(run_object["properties"]["summary"]["suppressed"], 1);
        assert_eq!(run_object["properties"]["summary"]["errors"], 2);
        // No artifact exists for a path with no emitted result.
        assert_eq!(run_object["artifacts"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn fix_reports_suppressed_findings_at_the_saved_source_positions() {
    // The directive keeps the V1001 on line 2, so `--fix` removes the trailing
    // whitespace on line 3 and the suppressed finding keeps its own line.
    let dir = project(&[(
        "a.verse",
        concat!(
            "# verse-lint: disable-next-line V1001 -- keeps the padding\n",
            "A := 1  \n",
            "B := 2  \n",
        ),
    )]);
    let (code, report) = json(dir.path(), &["a.verse", "--fix", "--show-suppressed"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["filesChanged"], 1);
    assert_eq!(report["summary"]["suppressed"], 1);
    assert_eq!(
        fs::read(dir.path().join("a.verse")).unwrap(),
        b"# verse-lint: disable-next-line V1001 -- keeps the padding\nA := 1  \nB := 2\n"
    );
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{report}");
    assert_eq!(diagnostics[0]["suppressed"], true);
    assert_eq!(diagnostics[0]["range"]["start"]["line"], 2);
    assert_eq!(
        diagnostics[0]["range"]["start"]["byteOffset"], 65,
        "the shown position must be the saved source, like an active finding"
    );

    // Without the flag the same fix produces the same file and an empty list.
    let dir = project(&[(
        "a.verse",
        concat!(
            "# verse-lint: disable-next-line V1001 -- keeps the padding\n",
            "A := 1  \n",
            "B := 2  \n",
        ),
    )]);
    let (code, report) = json(dir.path(), &["a.verse", "--fix"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["filesChanged"], 1);
    assert_eq!(report["summary"]["suppressed"], 1);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
}

#[test]
fn mixed_active_and_suppressed_ordering_is_shared_across_paths() {
    let dir = project(&[
        (
            "a.verse",
            "# verse-lint: disable-next-line V1001 -- first file\nA := 1  \nB := 2  \n",
        ),
        ("b.verse", "C := 3  \n"),
    ]);
    let (_, report) = json(dir.path(), &[".", "--show-suppressed"]);
    let listed: Vec<(String, usize, bool)> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            (
                d["path"].as_str().unwrap().to_string(),
                d["range"]["start"]["byteOffset"].as_u64().unwrap() as usize,
                d["suppressed"].as_bool().unwrap(),
            )
        })
        .collect();
    let mut sorted = listed.clone();
    sorted.sort();
    assert_eq!(
        listed, sorted,
        "path, then byte offset: a suppressed finding must not reorder the list"
    );
    assert_eq!(
        listed,
        [
            ("a.verse".to_string(), 58, true),
            ("a.verse".to_string(), 67, false),
            ("b.verse".to_string(), 6, false),
        ],
        "{report}"
    );
}

#[test]
fn suppressed_findings_still_consume_the_diagnostic_budget() {
    // 10,000 suppressed findings fill the budget exactly; the limit failure and
    // its message are unchanged by the flag.
    let mut body = String::new();
    for _ in 0..10_000 {
        body.push_str("# verse-lint: disable-next-line V1001 -- test\n \n");
    }
    let dir = project(&[("a.verse", &body)]);
    let (code, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["suppressed"], 10_000);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());

    // Shown, they are still counted once each and never become errors.
    let (code, report) = json(dir.path(), &["a.verse", "--show-suppressed"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["suppressed"], 10_000);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 10_000);
    assert_eq!(report["summary"]["errors"], 0);
}

#[test]
fn stdin_suppressed_output_is_identical_in_shape_to_file_input() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args(["-", "--show-suppressed", "--output-format=json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(MIXED.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["diagnostics"][1]["path"], "<stdin>");
    assert_eq!(report["diagnostics"][1]["suppressed"], true);
    assert_eq!(report["summary"]["suppressed"], 1);
}
