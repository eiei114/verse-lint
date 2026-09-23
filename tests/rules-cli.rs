use serde_json::Value;
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn lint(source: &str, selected: &str, extra: &[&str]) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_verse-lint"));
    command
        .current_dir(dir.path())
        .args(["-", "--output-format=json"]);
    if !selected.is_empty() {
        command.args(["--select", selected]);
    }
    let mut child = command
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let value = serde_json::from_slice(&out.stdout).unwrap();
    (out.status.code().unwrap(), value)
}

#[test]
fn defaults_are_three_errors_and_policy_warnings_are_opt_in() {
    let (code, v) = lint("f():void =  \n\tPrint(\"x\")  ", "", &[]);
    assert_eq!(code, 1, "{v}");
    let ids: Vec<_> = v["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["ruleId"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["V1001", "V1003", "V1001", "V1002"]);
    let policy = format!("# TODO {}\n", "x".repeat(121));
    assert_eq!(lint(&policy, "", &[]).1["summary"]["warnings"], 0);
    let (code, v) = lint(&policy, "V2001,V2002", &["--deny-warnings"]);
    assert_eq!(code, 1);
    assert_eq!(v["summary"]["warnings"], 2);
}

#[test]
fn todo_pathological_comment_is_bounded_and_hash_start_string_fails_as_coverage_not_directive() {
    let source = format!("# {}\n", "TODO( ".repeat(10001));
    let (code, v) = lint(&source, "V2002", &[]);
    assert_eq!(code, 2);
    assert!(
        v["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("diagnostic limit")
    );
    let (code, v) = lint("A := \"# verse-lint: invalid\"\n", "V1001", &[]);
    assert_eq!(code, 2);
    assert!(
        v["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("unsupported or incomplete Verse syntax")
    );
}

#[test]
fn final_newline_cases() {
    for (source, count) in [
        ("", 0),
        ("\u{feff}", 0),
        ("A := 1\n", 0),
        ("A := 1\r\n", 0),
        ("A := 1", 1),
        ("# comment", 1),
        ("<# block #>", 1),
        ("\u{feff}A := \"日😀\"", 1),
        (" ", 1),
        ("A := 1\nB := 2", 1),
    ] {
        let (code, v) = lint(source, "V1002", &[]);
        assert_eq!(code, i32::from(count > 0), "{source:?}: {v}");
        assert_eq!(v["diagnostics"].as_array().unwrap().len(), count);
        if count > 0 {
            assert_eq!(
                v["diagnostics"][0]["range"]["start"]["byteOffset"],
                source.len()
            );
        }
    }
}

#[test]
fn tab_indent_is_not_literal_comment_or_delimited_continuation() {
    for (source, count) in [
        ("f():void =\n\tPrint(\"x\")\n", 1),
        ("f():void =\n \tPrint(\"x\")\n", 1),
        ("f():void =\n    Print(\"x\")\n", 0),
        ("A := \"\t\"\n", 0),
        ("# \tcomment\n", 0),
        ("<# \n\tcomment\n#>\n", 0),
        ("A := array{\n\t1,\n\t2\n}\n", 0),
        ("\t\n", 0),
        ("f():void =\n\tPrint(\"x\")\n\tPrint(\"y\")\n", 2),
    ] {
        let (code, v) = lint(source, "V1003", &[]);
        assert_eq!(code, i32::from(count > 0), "{source:?}: {v}");
        assert_eq!(v["diagnostics"].as_array().unwrap().len(), count);
        for d in v["diagnostics"].as_array().unwrap() {
            assert_eq!(d["fixable"], false);
        }
    }
}

#[test]
fn line_length_counts_unicode_and_four_column_tab_stops() {
    for (source, count) in [
        (format!("#{}\n", "a".repeat(118)), 0),
        (format!("#{}\n", "a".repeat(119)), 0),
        (format!("#{}\n", "a".repeat(120)), 1),
        (format!("#{}\r\n", "日".repeat(119)), 0),
        (format!("\u{feff}#{}\n", "😀".repeat(120)), 1),
        (format!("#{}\n", "e\u{301}".repeat(60)), 1),
        (format!("#\t{}\n", "a".repeat(116)), 0),
        (format!("#\t{}\n", "a".repeat(117)), 1),
        (format!("A := \"https://{}\"\n", "x".repeat(120)), 1),
    ] {
        let (code, v) = lint(&source, "V2001", &[]);
        assert_eq!(code, 0, "{source:?}: {v}");
        assert_eq!(v["summary"]["warnings"], count);
        assert_eq!(
            lint(&source, "V2001", &["--deny-warnings"]).0,
            i32::from(count > 0)
        );
    }
}

#[test]
fn mixed_line_endings_keep_trailing_whitespace_and_length_detection_line_local() {
    let source = "A := 1  \r\nB := 2  \n";
    let (code, report) = lint(source, "V1001", &[]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["summary"]["errors"], 2);

    // CR belongs to the first CRLF terminator, not to the line's measured body.
    let source = format!("#{}\r\n# ok\n", "a".repeat(119));
    let (code, report) = lint(&source, "V2001", &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["warnings"], 0);
}

#[test]
fn todo_markers_have_explicit_boundaries_and_nonempty_same_line_reference() {
    for (source, count) in [
        ("# TODO do this\n", 1),
        ("# TODO(PROJ-1): do this\n", 0),
        ("# TODO()\n", 1),
        ("# TODO(  )\n", 1),
        ("# TODOS NOTODO _TODO TODO_\n", 0),
        ("# todo Todo\n", 0),
        ("A := \"TODO\"\n", 0),
        ("<# TODO one\n TODO two #>\n", 2),
        ("# TODO(x) TODO(y) TODO\n", 1),
        ("<# TODO(\nx) #>\n", 1),
        ("# 日本TODO語 TODO(参照)\n", 0),
    ] {
        let (code, v) = lint(source, "V2002", &[]);
        assert_eq!(code, 0, "{source:?}: {v}");
        assert_eq!(v["summary"]["warnings"], count, "{source:?}: {v}");
    }
}

#[test]
fn exact_directives_suppress_only_named_physical_line_diagnostics() {
    let source =
        "# verse-lint: disable-next-line V2002 -- tracked elsewhere\n# TODO one\n# TODO two\n";
    let (_, v) = lint(source, "V2002", &[]);
    assert_eq!(v["summary"]["suppressed"], 1);
    assert_eq!(v["summary"]["warnings"], 1);
    assert_eq!(v["diagnostics"][0]["range"]["start"]["line"], 3);
    let source = "f():void =\n\tPrint(\"x\") # verse-lint: disable-line V1003 -- tab intentionally retained\n";
    let (code, v) = lint(source, "V1003", &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["summary"]["suppressed"], 1);
    let source =
        "# verse-lint: disable-next-line V2002 -- TODO reason is not a finding\n\n# TODO stays\n";
    let (_, v) = lint(source, "V2002", &[]);
    assert_eq!(v["summary"]["suppressed"], 0);
    assert_eq!(v["summary"]["warnings"], 1);
}

#[test]
fn malformed_directives_fail_even_for_unselected_rules_and_never_suppress_engine_errors() {
    for directive in [
        "disable-next-line V9999 -- reason",
        "disable-next-line V2002",
        "disable-next-line V2002 -- ",
        "disable V2002 -- reason",
        "disable-next-line V1002 -- reason",
        "disable-next-line V2002, -- reason",
        "disable-line V2002 -- no code before comment",
        "disable-next-line all -- reason",
    ] {
        let (code, v) = lint(
            &format!("# verse-lint: {directive}\nA := 1\n"),
            "V1001",
            &[],
        );
        assert_eq!(code, 2, "{directive}: {v}");
        assert_eq!(v["summary"]["complete"], false);
    }
    assert_eq!(
        lint(
            "# verse-lint: disable-next-line V1001 -- not syntax\nA := \"bad",
            "V1001",
            &[]
        )
        .0,
        2
    );
    for source in [
        "A := \"verse-lint: invalid\"\n",
        "<# verse-lint: invalid #>\n",
        "# verse-lint is a useful tool\n",
    ] {
        assert_eq!(lint(source, "V1001", &[]).0, 0);
    }
}

#[test]
fn multi_rule_suppression_is_counted_and_effective_selection_honors_cli_overrides() {
    let source = "f():void =\n    # verse-lint: disable-next-line V1001,V1003 -- intentional test\n\tPrint(\"x\")  \n";
    let (code, v) = lint(source, "V1001,V1003", &[]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["summary"]["suppressed"], 2);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect=['V2001','V2002']\nignore=['V2002']\nmax-line-length=8\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("a.verse"), "# TODO xxxxxxxxx\n").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args([".", "--ignore", "V2001", "--output-format=json"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(out.status.success(), "{v}");
    assert_eq!(v["diagnostics"][0]["ruleId"], "V2002");
    assert_eq!(v["diagnostics"].as_array().unwrap().len(), 1);
    let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args([".", "--output-format=json"])
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["diagnostics"][0]["range"]["start"]["column"], 9);
}
