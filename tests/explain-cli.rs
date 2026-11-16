//! `--explain <RULE>` CLI contract: the embedded document reaches stdout for
//! every registered rule, nothing is read from the working directory, and every
//! documented refusal exits 2 without printing a document.
use std::{
    path::Path,
    process::{Command, Output},
};

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("launch verse-lint")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

/// Every registered rule ID, in registry order. Kept here rather than derived at
/// run time so an ID that silently disappears from the registry also fails.
const IDS: [&str; 11] = [
    "V1001", "V1002", "V1003", "V2001", "V2002", "V2003", "V3001", "V3002", "V3003", "V1004",
    "V1005",
];

/// The registry summary line and the document heading an explain run must print.
fn expected_name(id: &str) -> &'static str {
    match id {
        "V1001" => "TrailingWhitespace",
        "V1002" => "MissingFinalNewline",
        "V1003" => "TabIndentation",
        "V1004" => "SpaceIndentWidth",
        "V1005" => "ExcessFinalBlankLines",
        "V2001" => "LineTooLong",
        "V2002" => "BareTodoComment",
        "V2003" => "MixedLineEndings",
        "V3001" => "UnusedDeclaration",
        "V3002" => "UnresolvedReference",
        "V3003" => "VariableShadowing",
        other => panic!("{other} has no documented name"),
    }
}

#[test]
fn every_registered_rule_explains_itself_from_an_empty_directory() {
    // An empty directory has no `verse.toml` and no source: the run must not
    // need either, which is what "embedded in the binary" buys.
    let directory = tempfile::tempdir().unwrap();
    for id in IDS {
        let out = run(directory.path(), &["--explain", id]);
        assert_eq!(out.status.code(), Some(0), "{id}: {out:?}");
        assert!(out.stderr.is_empty(), "{id}: {}", stderr(&out));
        let text = stdout(&out);
        assert!(
            text.starts_with(&format!("{id} {} — default: ", expected_name(id))),
            "{id}: the registry summary line must come first: {text}"
        );
        // The document's own heading follows the summary, and its prose is the
        // repository text rather than a second hand-written copy.
        assert!(
            text.contains(&format!("\n# {id} — ")),
            "{id}: the embedded document heading is missing: {text}"
        );
        assert!(
            text.contains(&format!("document: docs/rules/{}.md", id.to_lowercase())),
            "{id}: the document source line is missing: {text}"
        );
        // Plain text only: no ANSI regardless of terminal detection.
        assert!(!text.contains('\u{1b}'), "{id}: explain must not emit ANSI");
    }
}

#[test]
fn explain_summary_matches_the_registry_policy() {
    // The four policy facts a reader needs, checked on the two interesting
    // shapes: an on-by-default fixable rule and an opt-in unfixable one.
    let directory = tempfile::tempdir().unwrap();
    let first = stdout(&run(directory.path(), &["--explain", "V1001"]));
    assert!(
        first.starts_with(
            "V1001 TrailingWhitespace — default: on; default severity: error; safe fix: yes"
        ),
        "{first}"
    );
    let policy = stdout(&run(directory.path(), &["--explain", "V2003"]));
    assert!(
        policy.starts_with(
            "V2003 MixedLineEndings — default: off; default severity: warning; safe fix: none"
        ),
        "{policy}"
    );
}

#[test]
fn explain_reads_no_configuration_and_does_not_follow_it() {
    // A configuration that rejects every run (unsupported schema version, an
    // invalid severity value and an unknown rule in select) must not affect
    // `--explain`: it never resolves configuration.
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(
        directory.path().join("verse.toml"),
        "schema-version = 99\n[lint.severity]\nV2001 = \"Error\"\n",
    )
    .unwrap();
    std::fs::write(directory.path().join("broken.verse"), "A := \n").unwrap();
    let out = run(directory.path(), &["--explain", "V2001"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stderr.is_empty(), "{}", stderr(&out));
    assert!(stdout(&out).starts_with("V2001 LineTooLong — "));
}

#[test]
fn unknown_and_malformed_ids_exit_two_without_a_document() {
    let directory = tempfile::tempdir().unwrap();
    for (entry, needle) in [
        ("V9999", "matched no registered rule"),
        ("V1", "matched no registered rule"),
        ("v1001", "invalid rule entry"),
        ("V1001x", "invalid rule entry"),
        ("1001", "invalid rule entry"),
    ] {
        let out = run(directory.path(), &["--explain", entry]);
        assert_eq!(out.status.code(), Some(2), "{entry}: {out:?}");
        assert!(
            out.stdout.is_empty(),
            "{entry}: a refused explain must print no document: {}",
            stdout(&out)
        );
        let error = stderr(&out);
        assert!(
            error.contains(needle),
            "{entry}: the message must say {needle:?}: {error}"
        );
        assert!(
            error.contains(entry),
            "{entry}: the message must name the rejected entry: {error}"
        );
    }
}

#[test]
fn explain_conflicts_with_a_source_run_and_the_other_run_free_surfaces() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.verse"), "A := 1\n").unwrap();
    for args in [
        vec!["a.verse", "--explain", "V1001"],
        vec!["--explain", "V1001", "--fix"],
        vec!["--explain", "V1001", "--diff"],
        vec!["--explain", "V1001", "--output-format", "json"],
        vec!["--explain", "V1001", "--select", "V1002"],
        vec!["--explain", "V1001", "--ignore", "V1002"],
        vec!["--explain", "V1001", "--show-suppressed"],
        vec!["--explain", "V1001", "--show-config"],
        vec!["--explain", "V1001", "--dump-scopes"],
        vec!["--explain", "V1001", "--semantic-coverage"],
        vec!["--explain", "V1001", "--require-complete-semantics"],
    ] {
        let out = run(directory.path(), &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        // A machine output format turns the clap rejection into the ordinary
        // JSON error envelope (never a document, never a diagnostic); the other
        // combinations print nothing at all.
        let printed = stdout(&out);
        assert!(
            !printed.contains("\n# V") && !printed.contains("document: docs/rules/"),
            "{args:?}: a refused explain must print no rule document: {printed}"
        );
        assert!(!stderr(&out).is_empty(), "{args:?}: needs a message");
    }
}

#[test]
fn explain_is_help_visible_and_inert_with_color() {
    let directory = tempfile::tempdir().unwrap();
    let help = stdout(&run(directory.path(), &["--help"]));
    assert!(
        help.contains("--explain <RULE>"),
        "the help must name the flag and its value: {help}"
    );
    // `--color` belongs to text diagnostics; explain output is never coloured,
    // so accepting it keeps the flag harmless instead of failing a scripted run.
    let colored = run(
        directory.path(),
        &["--explain", "V1002", "--color", "always"],
    );
    assert_eq!(colored.status.code(), Some(0), "{colored:?}");
    let plain = stdout(&run(directory.path(), &["--explain", "V1002"]));
    assert_eq!(stdout(&colored), plain);
    assert!(!plain.contains('\u{1b}'));
}
