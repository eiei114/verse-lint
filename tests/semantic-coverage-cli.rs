//! `--semantic-coverage` and `--require-complete-semantics` CLI contract:
//! versioned summary, no rules, exit 0 for low coverage, and a documented exit-2
//! switch for strict projects.
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

#[path = "support/json_schema.rs"]
mod json_schema;

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_verse-lint"));
    command
        .current_dir(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match stdin {
        Some(bytes) => {
            let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            child.wait_with_output().unwrap()
        }
        None => command.output().unwrap(),
    }
}

fn coverage_of(source: &str) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap();
    let out = run(
        dir.path(),
        &["-", "--semantic-coverage"],
        Some(source.as_bytes()),
    );
    let code = out.status.code().unwrap();
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    (code, value)
}

/// The summary is a stable contract, so it is validated against the pinned
/// document in `docs/coverage.schema.json` rather than only spot-checked.
fn schema() -> json_schema::Schema {
    json_schema::Schema::new(
        include_str!("../docs/coverage.schema.json"),
        "docs/coverage.schema.json",
    )
}

#[test]
fn coverage_prints_only_the_summary_runs_no_rule_and_exits_zero() {
    // The source carries a trailing-whitespace error, a bare TODO, and a
    // missing final newline: a lint run would exit 1. The coverage report must
    // still exit 0 and must not mention a rule or a diagnostic.
    let source = "F(X:int):void =\n\tPrint(\"x\")  \n# TODO later";
    let (code, value) = coverage_of(source);
    assert_eq!(code, 0, "{value}");
    let text = serde_json::to_string(&value).unwrap();
    for absent in ["diagnostics", "ruleId", "V1001", "V2002", "executionErrors"] {
        assert!(
            !text.contains(absent),
            "coverage must not run rules: {absent}"
        );
    }
    assert_eq!(value["schemaVersion"], 1);
    // Only the summary and the per-file numbers it was made of.
    let keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["files", "schemaVersion", "summary"], "{value}");
    assert_eq!(value["files"][0]["path"], "<stdin>");
    assert_eq!(value["files"][0]["isComplete"], true);
    schema().validate(&value).expect("coverage document schema");
}

/// Low coverage is information, never an error: one incomplete source out of two
/// keeps the exit code at 0 and reports the number instead of failing.
#[test]
fn low_coverage_is_information_and_still_exits_zero() {
    let structural = "F():void =\n    P := point{X := 1}\n    Print(P)\n";
    let (code, value) = coverage_of(structural);
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["summary"]["sourcesAnalyzed"], 1);
    assert_eq!(value["summary"]["completeSources"], 0);
    assert_eq!(value["summary"]["incompleteSources"], 1);
    assert_eq!(value["summary"]["skippedKinds"]["field_initializer"], 1);
    assert_eq!(value["files"][0]["isComplete"], false);
    schema().validate(&value).expect("coverage document schema");
}

/// A hand-built fixture with answers read off the source by hand: three
/// bindings, three references, two of them resolved. Nothing here is a snapshot.
#[test]
fn summary_counts_a_known_fixture_exactly() {
    let source = "F(X:int):void =\n    Local := X\n    Print(Local)\n";
    let (code, value) = coverage_of(source);
    assert_eq!(code, 0, "{value}");
    let summary = &value["summary"];
    assert_eq!(summary["sourcesAnalyzed"], 1);
    assert_eq!(summary["completeSources"], 1);
    assert_eq!(summary["incompleteSources"], 0);
    // `F` is a function, `X` a parameter, `Local` a constant.
    assert_eq!(summary["bindingsByKind"]["function"], 1);
    assert_eq!(summary["bindingsByKind"]["parameter"], 1);
    assert_eq!(summary["bindingsByKind"]["constant"], 1);
    // `X` resolves, `Local` resolves, `Print` is an external name this model
    // cannot see. All three are ordinary local references.
    assert_eq!(summary["referencesByKind"]["local"], 3);
    assert_eq!(summary["resolvedReferences"], 2);
    assert_eq!(summary["unresolvedReferences"], 1);
    assert_eq!(summary["skippedKinds"], serde_json::json!({}));
    schema().validate(&value).expect("coverage document schema");
}

#[test]
fn coverage_is_deterministic_for_the_same_input() {
    let source = "using { Helpers }\nF(X:int):void =\n    Local := X\n    Print(Local)\n";
    let (_, first) = coverage_of(source);
    let (_, second) = coverage_of(source);
    assert_eq!(first, second);
}

/// `--dump-scopes` keeps the graph and adds the same summary section, computed by
/// the same aggregation, so the two surfaces cannot disagree.
#[test]
fn dump_scopes_includes_the_summary_section() {
    let source = "F(X:int):void =\n    Local := X\n    Print(Local)\n";
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["-", "--dump-scopes"], Some(source.as_bytes()));
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let dumped: Value = serde_json::from_slice(&out.stdout).unwrap();
    let (_, coverage) = coverage_of(source);
    // The graph is still there, unchanged.
    assert!(dumped["files"][0]["scopes"].as_array().unwrap().len() > 1);
    assert!(dumped["files"][0]["bindings"].is_array());
    // The summary is byte-for-byte the same object `--semantic-coverage` prints,
    // because both are built from the same per-file counts.
    assert_eq!(dumped["summary"], coverage["summary"], "{dumped}");
    assert_eq!(dumped["schemaVersion"], 1);
}

/// Strict mode is opt-in and changes only the exit contract: the same run without
/// it exits 0, with it the incomplete source becomes an execution error and exit 2.
#[test]
fn require_complete_semantics_turns_an_incomplete_model_into_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("incomplete.verse"),
        "F():void =\n    P := point{X := 1}\n    Print(P)\n",
    )
    .unwrap();
    for args in [
        vec!["incomplete.verse", "--select", "V3001"],
        vec!["incomplete.verse", "--semantic-coverage"],
        vec!["incomplete.verse", "--dump-scopes"],
    ] {
        let relaxed = run(dir.path(), &args, None);
        assert_eq!(relaxed.status.code(), Some(0), "{args:?}: {relaxed:?}");
        let strict = run(
            dir.path(),
            &[&args[..], &["--require-complete-semantics"][..]].concat(),
            None,
        );
        assert_eq!(strict.status.code(), Some(2), "{args:?}: {strict:?}");
        // A strict failure emits no partial document and names the unmodelled kind.
        assert!(strict.stdout.is_empty(), "{args:?} wrote stdout");
        let message = String::from_utf8_lossy(&strict.stderr);
        assert!(message.contains("incomplete.verse"), "{args:?}: {message}");
        assert!(message.contains("field_initializer"), "{args:?}: {message}");
        assert!(
            message.contains("--require-complete-semantics"),
            "{args:?}: {message}"
        );
    }

    // A complete project passes the strict switch untouched, so the refusal above
    // is about incompleteness and not about the flag itself.
    std::fs::write(
        dir.path().join("complete.verse"),
        "F(X:int):void =\n    Print(X)\n",
    )
    .unwrap();
    for args in [
        vec!["complete.verse", "--semantic-coverage"],
        vec!["complete.verse", "--dump-scopes"],
        vec!["complete.verse", "--select", "V3001"],
    ] {
        let strict = run(
            dir.path(),
            &[&args[..], &["--require-complete-semantics"][..]].concat(),
            None,
        );
        assert_eq!(strict.status.code(), Some(0), "{args:?}: {strict:?}");
        assert!(strict.stderr.is_empty(), "{args:?}: {strict:?}");
    }
}

/// The documented conflicts: each surface refuses a write run and a
/// configuration print before any source is read, with a message naming the flag.
#[test]
fn both_flags_refuse_fix_and_show_config_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.verse"), b"A := 1\n").unwrap();
    for surface in [
        "--semantic-coverage",
        "--dump-scopes",
        "--require-complete-semantics",
    ] {
        for args in [
            // A write run and a configuration print are both refused before any
            // source is read, whichever spelling reaches the surface first.
            vec!["a.verse", surface, "--fix"],
            vec!["a.verse", surface, "--fix", "--diff"],
            vec!["a.verse", surface, "--show-config"],
        ] {
            let out = run(dir.path(), &args, None);
            assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
            assert!(out.stdout.is_empty(), "{args:?} wrote stdout");
            let message = String::from_utf8_lossy(&out.stderr);
            assert!(message.contains(surface), "{args:?}: {message}");
            let refused = if args.contains(&"--show-config") {
                "--show-config"
            } else {
                "--fix"
            };
            assert!(message.contains(refused), "{args:?}: {message}");
        }
    }

    // Both JSON surfaces write a document to stdout, so they cannot run together.
    let out = run(
        dir.path(),
        &["a.verse", "--dump-scopes", "--semantic-coverage"],
        None,
    );
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty());
}

/// A machine format keeps being reported through its own envelope: the conflict
/// message must not be swallowed into a scope graph or a coverage summary.
#[test]
fn machine_formats_report_the_conflict_without_emitting_a_document() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.verse"), b"A := 1\n").unwrap();
    for format in ["json", "sarif"] {
        for surface in ["--semantic-coverage", "--dump-scopes"] {
            let flag = format!("--output-format={format}");
            let out = run(dir.path(), &["a.verse", surface, flag.as_str()], None);
            assert_eq!(out.status.code(), Some(2), "{surface} {format}: {out:?}");
            let text = String::from_utf8(out.stdout).unwrap();
            // The machine envelope carries the execution error; a coverage summary
            // or a scope graph must never appear in its place.
            match format {
                "json" => assert!(text.contains("executionErrors"), "{surface}: {text}"),
                _ => assert!(
                    text.contains("toolExecutionNotifications"),
                    "{surface}: {text}"
                ),
            }
            assert!(
                text.contains("cannot be combined") || text.contains("emits its own JSON"),
                "{surface} {format}: the conflict must be named: {text}"
            );
            for absent in ["sourcesAnalyzed", "\"scopes\"", "\"bindings\""] {
                assert!(
                    !text.contains(absent),
                    "{surface} {format} emitted a document: {absent} in {text}"
                );
            }
        }
    }
}

/// A file the tool refuses to parse stays an execution error with no JSON
/// document, exactly as `--dump-scopes` already behaves: a partial summary would
/// look like a complete measurement of an incomplete run.
#[test]
fn unparseable_input_fails_closed_with_no_summary() {
    let dir = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("markup.verse", "A := <p>text</p>\n"),
        ("unicode.verse", "得点 := 1\n"),
        ("incomplete.verse", "A:int=\n"),
        ("digest.verse", "<#> digest\n    detail\nA := 1\n"),
    ] {
        std::fs::write(dir.path().join(name), source).unwrap();
        for surface in ["--semantic-coverage", "--dump-scopes"] {
            let out = run(dir.path(), &[name, surface], None);
            assert_eq!(out.status.code(), Some(2), "{name} {surface}: {out:?}");
            assert!(out.stdout.is_empty(), "{name} {surface} printed a document");
            assert!(!out.stderr.is_empty(), "{name} {surface} was silent");
        }
    }
}

/// Discovery is shared with a lint run, so the reported file set follows
/// `[files] exclude`, `verse-lint.ignore` and `--no-ignore-file`.
#[test]
fn discovery_matches_a_lint_run_including_the_ignore_file() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("verse.toml"), "").unwrap();
    std::fs::write(root.join("verse-lint.ignore"), "Generated/**\n").unwrap();
    std::fs::create_dir(root.join("Generated")).unwrap();
    std::fs::write(root.join("kept.verse"), "F():void = {}\n").unwrap();
    std::fs::write(root.join("Generated/skipped.verse"), "G():void = {}\n").unwrap();
    let paths = |args: &[&str]| -> Vec<String> {
        let out = run(root, args, None);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        schema().validate(&value).expect("coverage document schema");
        value["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(paths(&["--semantic-coverage"]), ["kept.verse"]);
    assert_eq!(
        paths(&["--semantic-coverage", "--no-ignore-file"]),
        ["Generated/skipped.verse", "kept.verse"]
    );
}
