//! V1004 space-indent-width: one warning per structural line whose leading space
//! run is not a multiple of the configured `[lint] indent-width`.
//!
//! Every assertion here is about a configured width, because the rule has no
//! other source for one: the tool never infers a width from the file.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn json(root: &Path, args: &[&str]) -> (i32, Value) {
    let mut full = args.to_vec();
    full.push("--output-format=json");
    let out = run(root, &full);
    let report = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code().unwrap(), report)
}

/// One project holding `name` plus an optional configuration.
fn project(name: &str, bytes: &[u8], config: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(name), bytes).unwrap();
    if let Some(config) = config {
        fs::write(dir.path().join("verse.toml"), config).unwrap();
    }
    dir
}

/// Every V1004 finding as `(line, startColumn, endColumn, runLength)`.
///
/// The length is the byte length of the range, so a wrong span fails here even
/// when the start position is right.
fn findings(report: &Value) -> Vec<(u64, u64, u64, u64)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["ruleId"] == "V1004")
        .map(|diagnostic| {
            assert_eq!(diagnostic["severity"], "warning", "{report}");
            assert_eq!(diagnostic["fixable"], false, "{report}");
            assert_eq!(
                diagnostic["message"],
                "Indentation is not a multiple of the configured indent width",
                "{report}"
            );
            let start = &diagnostic["range"]["start"];
            let end = &diagnostic["range"]["end"];
            (
                start["line"].as_u64().unwrap(),
                start["column"].as_u64().unwrap(),
                end["column"].as_u64().unwrap(),
                end["byteOffset"].as_u64().unwrap() - start["byteOffset"].as_u64().unwrap(),
            )
        })
        .collect()
}

const WIDTH_2: &str = "[lint]\nindent-width = 2\n";

/// `f():void =` then a body line indented by `spaces` spaces, so exactly one
/// structural indentation run exists and its length is known.
fn indented(spaces: usize) -> String {
    format!("f():void =\n{}Print(\"x\")\n", " ".repeat(spaces))
}

#[test]
fn the_default_width_reports_half_and_one_and_a_half_levels_and_clears_multiples() {
    let dir = tempfile::tempdir().unwrap();
    for (spaces, expected) in [
        (2, vec![(2, 1, 3, 2)]),
        (6, vec![(2, 1, 7, 6)]),
        (4, vec![]),
        (8, vec![]),
    ] {
        fs::write(dir.path().join("main.verse"), indented(spaces)).unwrap();
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
        assert_eq!(code, 0, "a warning does not fail by default: {report}");
        assert_eq!(findings(&report), expected, "{spaces} spaces: {report}");
        assert_eq!(report["summary"]["warnings"], expected.len(), "{report}");
    }
}

#[test]
fn a_configured_width_of_two_flips_the_same_cases() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("verse.toml"), WIDTH_2).unwrap();
    for (spaces, expected) in [
        (2, vec![]),
        (4, vec![]),
        (6, vec![]),
        (8, vec![]),
        (3, vec![(2, 1, 4, 3)]),
    ] {
        fs::write(dir.path().join("main.verse"), indented(spaces)).unwrap();
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
        assert_eq!(code, 0, "{spaces} spaces: {report}");
        assert_eq!(findings(&report), expected, "{spaces} spaces: {report}");
    }
}

/// The same fixture run with and without the width proves the setting is read:
/// six spaces is wrong at width 4 and right at width 2.
#[test]
fn the_configured_width_is_the_only_source_of_the_expected_length() {
    let dir = project("main.verse", indented(6).as_bytes(), Some(WIDTH_2));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
    assert_eq!(code, 0, "{report}");
    assert!(
        findings(&report).is_empty(),
        "width 2 accepts six spaces: {report}"
    );

    // `--config` pointing elsewhere is the same configuration.
    fs::write(dir.path().join("wide.toml"), "[lint]\nindent-width = 4\n").unwrap();
    let (code, report) = json(
        dir.path(),
        &["main.verse", "--config", "wide.toml", "--select", "V1004"],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(2, 1, 7, 6)], "{report}");
}

#[test]
fn a_tab_run_belongs_to_v1003_and_never_double_reports_a_line() {
    for body in [
        "f():void =\n\tPrint(\"x\")\n",
        "f():void =\n \tPrint(\"x\")\n",
    ] {
        let dir = project("main.verse", body.as_bytes(), None);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1003,V1004"]);
        // V1003 is an error, so the run fails; V1004 stays silent about the line.
        assert_eq!(code, 1, "{body:?}: {report}");
        assert_eq!(findings(&report), Vec::new(), "{body:?}: {report}");
        let ids: Vec<_> = report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["ruleId"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["V1003"], "{body:?}: {report}");
    }
}

#[test]
fn blank_whitespace_comment_continuation_string_and_comment_runs_are_not_diagnosed() {
    for body in [
        // No code on the line, and the trailing space is V1001's business.
        "f():void =\n\n    Print(\"x\")\n",
        "f():void =\n  \n    Print(\"x\")\n",
        // A comment-only line's indentation is not a block statement.
        "f():void =\n  # note\n    Print(\"x\")\n",
        // A delimited continuation, whose alignment is a layout choice.
        "A := array{\n  1,\n    2\n}\n",
        // Literal bytes inside strings and comments are not indentation.
        "A := \"  x\"\n",
        "<#\n  text\n#>\n",
        // A line with no leading run at all cannot be reported.
        "A := 1\n",
    ] {
        let dir = project("main.verse", body.as_bytes(), None);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1003,V1004"]);
        assert_eq!(code, 0, "{body:?}: {report}");
        assert_eq!(findings(&report), Vec::new(), "{body:?}: {report}");
    }
}

#[test]
fn one_finding_per_offending_line_and_none_for_the_clean_lines_between_them() {
    let body =
        "f():void =\n  Print(\"a\")\n    Print(\"b\")\n      Print(\"c\")\n        Print(\"d\")\n";
    let dir = project("main.verse", body.as_bytes(), None);
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(2, 1, 3, 2), (4, 1, 7, 6)], "{report}");
    assert_eq!(report["summary"]["warnings"], 2, "{report}");
}

#[test]
fn invalid_indent_widths_exit_two_naming_the_key_with_or_without_the_rule_selected() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.verse"), indented(2)).unwrap();
    let config = dir.path().join("verse.toml");

    for (value, needle) in [
        ("0", "indent-width must be between 1 and 16"),
        ("17", "indent-width must be between 1 and 16"),
        ("-1", "indent-width"),
        ("\"4\"", "indent-width"),
        ("4.5", "indent-width"),
        ("true", "indent-width"),
    ] {
        fs::write(&config, format!("[lint]\nindent-width = {value}\n")).unwrap();
        // With V1004 selected and with a selection that never reads the width.
        for select in ["V1004", "V1001"] {
            let (code, report) = json(dir.path(), &["main.verse", "--select", select]);
            assert_eq!(code, 2, "{value} / {select}: {report}");
            assert_eq!(report["summary"]["complete"], false, "{value}: {report}");
            assert!(
                report["diagnostics"].as_array().unwrap().is_empty(),
                "{value}: a rejected configuration must not report: {report}"
            );
            let message = report["executionErrors"][0]["message"]
                .as_str()
                .unwrap_or_else(|| panic!("{value}: {report}"));
            assert!(message.contains(needle), "{value}: {message}");
        }
    }

    // The two accepted ends of the documented range are fine.
    for value in ["1", "16"] {
        fs::write(&config, format!("[lint]\nindent-width = {value}\n")).unwrap();
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
        assert_eq!(code, 0, "{value}: {report}");
        assert_eq!(report["summary"]["complete"], true, "{value}: {report}");
    }
    // And `--fix` never starts after such a refusal.
    fs::write(&config, "[lint]\nindent-width = 0\n").unwrap();
    let out = run(dir.path(), &["main.verse", "--fix"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(
        fs::read(dir.path().join("main.verse")).unwrap(),
        indented(2).as_bytes()
    );
}

#[test]
fn the_rule_is_off_by_default_and_reached_by_every_documented_selection() {
    let dir = project("main.verse", indented(2).as_bytes(), None);
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "off by default: {report}");

    for selection in ["V1004", "V1", "V"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert_eq!(findings(&report), [(2, 1, 3, 2)], "{selection}: {report}");
    }
    // The other families never bring it in.
    for selection in ["V2", "V3", "V1001,V1002,V1003"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert!(findings(&report).is_empty(), "{selection}: {report}");
    }

    let denied = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V1004",
            "--deny-warnings",
            "--output-format=json",
        ],
    );
    assert_eq!(denied.status.code(), Some(1), "{denied:?}");
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(findings(&denied), [(2, 1, 3, 2)], "{denied}");
}

#[test]
fn severity_override_and_ignore_follow_the_shared_plumbing() {
    let dir = project(
        "main.verse",
        indented(2).as_bytes(),
        Some("[lint]\nselect = ['V1004']\n[lint.severity]\nV1004 = 'error'\n"),
    );
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["diagnostics"][0]["severity"], "error", "{report}");
    assert_eq!(report["summary"]["errors"], 1, "{report}");
    // The SARIF driver metadata keeps the tool default, not the project policy.
    let sarif: Value =
        serde_json::from_slice(&run(dir.path(), &["main.verse", "--output-format=sarif"]).stdout)
            .unwrap();
    let descriptor = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V1004")
        .expect("V1004 driver entry");
    assert_eq!(descriptor["name"], "SpaceIndentWidth");
    assert_eq!(descriptor["defaultConfiguration"]["level"], "warning");
    assert_eq!(descriptor["defaultConfiguration"]["enabled"], false);
    assert_eq!(descriptor["properties"]["fixable"], false);
    assert_eq!(sarif["runs"][0]["results"][0]["level"], "error");

    // `--ignore V1004` and `[lint] ignore` both remove the rule.
    let dir = project("main.verse", indented(2).as_bytes(), None);
    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V1", "--ignore", "V1004"],
    );
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");
    assert_eq!(report["summary"]["suppressed"], 0, "{report}");

    let dir = project(
        "main.verse",
        indented(2).as_bytes(),
        Some("[lint]\nselect = ['V1001', 'V1004']\nignore = ['V1004']\n"),
    );
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");
}

#[test]
fn a_line_directive_suppresses_the_diagnosed_line_and_show_suppressed_lists_it() {
    let dir = project(
        "main.verse",
        b"f():void =\n  # verse-lint: disable-next-line V1004 -- generated snippet, width preserved\n  Print(\"x\")\n",
        None,
    );
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");
    assert_eq!(report["summary"]["suppressed"], 1, "{report}");

    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V1004", "--show-suppressed"],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(3, 1, 3, 2)], "{report}");
    assert_eq!(report["diagnostics"][0]["suppressed"], true, "{report}");

    // `disable-line` works on the same physical line as the indentation.
    let dir = project(
        "main.verse",
        b"f():void =\n  Print(\"x\") # verse-lint: disable-line V1004 -- keep width\n",
        None,
    );
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");
    assert_eq!(report["summary"]["suppressed"], 1, "{report}");
}

#[test]
fn text_json_sarif_and_github_agree_on_the_single_finding() {
    let dir = project("main.verse", indented(2).as_bytes(), None);
    let text = run(dir.path(), &["main.verse", "--select", "V1004"]);
    assert_eq!(
        String::from_utf8(text.stdout).unwrap(),
        "main.verse:2:1: warning V1004 Indentation is not a multiple of the configured indent width\n"
    );

    let (_, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
    let sarif: Value = serde_json::from_slice(
        &run(
            dir.path(),
            &["main.verse", "--select", "V1004", "--output-format=sarif"],
        )
        .stdout,
    )
    .unwrap();
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        report["diagnostics"].as_array().unwrap().len()
    );
    assert_eq!(results[0]["ruleId"], "V1004");
    assert_eq!(results[0]["level"], "warning");
    let region = &results[0]["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 2);
    assert_eq!(region["startColumn"], 1);
    assert_eq!(region["endColumn"], 3);
    assert_eq!(region["byteOffset"], 11);
    assert_eq!(region["byteLength"], 2);
    assert_eq!(sarif["runs"][0]["properties"]["summary"], report["summary"]);

    let github = String::from_utf8(
        run(
            dir.path(),
            &["main.verse", "--select", "V1004", "--output-format=github"],
        )
        .stdout,
    )
    .unwrap();
    assert_eq!(
        github,
        "::warning file=main.verse,line=2,col=1,endLine=2,endColumn=3,title=V1004 SpaceIndentWidth::Indentation is not a multiple of the configured indent width\n"
    );
}

/// The Detection section states its width-two example in prose, so it can drift
/// silently from the implementation. This pins that sentence to the binary: the
/// run length it names as reported must really be reported at that width, and no
/// multiple of two may be presented as the reported case.
#[test]
fn the_documented_width_two_example_names_a_length_the_rule_reports() {
    let rule = include_str!("../docs/rules/v1004.md");
    let sentence = rule
        .split("\n\n")
        .find(|block| block.contains("those cases flip"))
        .unwrap_or_else(|| panic!("the width-two sentence is missing: {rule}"));
    assert!(
        sentence.contains("three"),
        "the reported width-2 example must be a non-multiple of two: {sentence}"
    );
    assert!(
        !sentence.contains("six is reported"),
        "six is a multiple of two, so V1004 never reports it there: {sentence}"
    );

    // The same fixture the documented example describes, executed: three spaces
    // is the one reported length and the even runs stay clean.
    for (spaces, reported) in [(2, false), (3, true), (4, false), (6, false), (8, false)] {
        let dir = project("main.verse", indented(spaces).as_bytes(), Some(WIDTH_2));
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1004"]);
        assert_eq!(code, 0, "{spaces} spaces: {report}");
        assert_eq!(
            !findings(&report).is_empty(),
            reported,
            "{spaces} spaces at width 2: {report}"
        );
    }
}

/// The documents must state the rule, the configured-only width, and the refusal
/// to guess, because that stance is the whole reason the rule exists.
#[test]
fn the_documented_rule_states_that_the_width_is_never_inferred() {
    let rule = include_str!("../docs/rules/v1004.md");
    assert!(rule.contains("never inferred"), "{rule}");
    assert!(rule.contains("`[lint] indent-width`"), "{rule}");
    assert!(rule.contains("no safe fix"), "{rule}");
    let index = include_str!("../docs/rules.md");
    assert!(index.contains("](rules/v1004.md)"), "{index}");
    assert!(index.contains("| Off | warning | None |"), "{index}");
    let cli = include_str!("../docs/cli.md");
    assert!(cli.contains("### `[lint] indent-width`"), "{cli}");
    assert!(cli.contains("never inferred"), "{cli}");
    let readme = include_str!("../README.md");
    assert!(readme.contains("[V1004][v1004]"), "{readme}");
    assert!(readme.contains("never from the file's most"), "{readme}");
    assert!(
        readme.contains(
            "[v1004]: https://github.com/eiei114/verse-lint/blob/main/docs/rules/v1004.md"
        ),
        "{readme}"
    );
}
