//! V1005 excess-final-blank-lines: one warning per file whose trailing trivia
//! holds more than one line terminator with only empty lines between.
//!
//! The rule is file-level like V1002, so a line directive cannot suppress it,
//! and it is read-only: `--fix` never touches it.
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

fn project(name: &str, bytes: &[u8]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join(name), bytes).unwrap();
    dir
}

/// Every V1005 finding as `(startLine, endLine, byteOffset, byteLength)`.
///
/// The length is the byte length of the range, so a span that only starts in the
/// right place still fails here.
fn findings(report: &Value) -> Vec<(u64, u64, u64, u64)> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|diagnostic| diagnostic["ruleId"] == "V1005")
        .map(|diagnostic| {
            assert_eq!(diagnostic["severity"], "warning", "{report}");
            assert_eq!(diagnostic["fixable"], false, "{report}");
            assert_eq!(
                diagnostic["message"], "File ends with excess blank lines",
                "{report}"
            );
            let start = &diagnostic["range"]["start"];
            let end = &diagnostic["range"]["end"];
            (
                start["line"].as_u64().unwrap(),
                end["line"].as_u64().unwrap(),
                start["byteOffset"].as_u64().unwrap(),
                end["byteOffset"].as_u64().unwrap() - start["byteOffset"].as_u64().unwrap(),
            )
        })
        .collect()
}

fn reported_ids(report: &Value) -> Vec<&str> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["ruleId"].as_str().unwrap())
        .collect()
}

/// The reported range is the excess tail: from the first byte of the first blank
/// line after the last content to end of file, so the extent of a long tail is
/// visible in one finding.
const ONE_BLANK_LF: &[u8] = b"A := 1\n\n";
const ONE_BLANK_CRLF: &[u8] = b"A := 1\r\n\r\n";
const MANY_BLANKS_LF: &[u8] = b"A := 1\n\n\n\n";

#[test]
fn one_terminator_is_clean_and_two_or_more_are_reported_once() {
    for (bytes, expected) in [
        // Exactly one final terminator is the clean termination V1002 asks for.
        (&b"A := 1\n"[..], vec![]),
        (b"A := 1\r\n", vec![]),
        // A single blank line: two terminators, reported over the blank line.
        (ONE_BLANK_LF, vec![(2, 3, 7, 1)]),
        (ONE_BLANK_CRLF, vec![(2, 3, 8, 2)]),
        // Any number of blank lines is still exactly one finding whose range
        // runs to end of file, so the extent is visible and the count is stable.
        (MANY_BLANKS_LF, vec![(2, 5, 7, 3)]),
        (b"A := 1\n\n\n\n\n", vec![(2, 6, 7, 4)]),
        (b"A := 1\r\n\r\n\r\n\r\n", vec![(2, 5, 8, 6)]),
    ] {
        let dir = project("main.verse", bytes);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
        assert_eq!(
            code, 0,
            "a warning does not fail by default: {bytes:?}: {report}"
        );
        assert_eq!(findings(&report), expected, "{bytes:?}: {report}");
        assert_eq!(report["summary"]["warnings"], expected.len(), "{bytes:?}");
    }
}

/// The tail is measured from the last non-trivia token, so a comment line is
/// content and the blank lines after it are the excess.
#[test]
fn a_comment_is_content_so_blank_lines_after_it_are_reported() {
    let dir = project("main.verse", b"A := 1\n# note\n\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(3, 4, 14, 1)], "{report}");

    // One terminator after the comment is clean.
    let dir = project("main.verse", b"A := 1\n# note\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");

    // A block comment as the last content token behaves the same way. It needs a
    // function body around it, because a bare `<# #>` after a statement is
    // outside the pinned grammar and refused before any rule runs.
    let dir = project(
        "main.verse",
        b"f():void =\n    Print(\"x\")\n<# trailer #>\n\n",
    );
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(4, 5, 40, 1)], "{report}");
}

/// Terminators inside a protected span belong to that token, never to the
/// trailing trivia, so blank lines inside a comment are not this rule's finding.
#[test]
fn blank_lines_inside_a_comment_or_string_are_never_reported() {
    for bytes in [
        // A blank line inside a block comment, one terminator after it: clean.
        &b"f():void =\n    Print(\"x\")\n<# x\n\ny #>\n"[..],
        // The same comment with no trailing blank line either.
        b"f():void =\n    Print(\"x\")\n<# x\n\ny #>",
        // A string is a single token, so its terminators are never counted.
        b"A := \"x\"\n",
        // A content-free file has no excess: shown clean by the rule under test.
        b"\n\n",
    ] {
        let dir = project("main.verse", bytes);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
        assert_eq!(code, 0, "{bytes:?}: {report}");
        assert!(findings(&report).is_empty(), "{bytes:?}: {report}");
    }

    // A whitespace-only trailing line belongs to V1001 alone, so the two rules
    // never describe one line. Each fixture ends without an excess blank line.
    for bytes in [
        &b"A := 1\n\n  \n"[..],
        b"A := 1\n  \n\n",
        b"A := 1\n  ",
        b"A := 1\n  \n",
    ] {
        let dir = project("main.verse", bytes);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005,V1001"]);
        assert_eq!(code, 1, "trailing whitespace is V1001's error: {bytes:?}");
        assert!(findings(&report).is_empty(), "{bytes:?}: {report}");
        assert!(
            reported_ids(&report).contains(&"V1001"),
            "the whitespace still belongs to V1001: {bytes:?}: {report}"
        );
    }

    // Blank lines inside a comment plus a real excess tail: the tail is reported
    // at the right place, not at the comment's interior.
    let dir = project(
        "main.verse",
        b"f():void =\n    Print(\"x\")\n<# x\n\ny #>\n\n",
    );
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(6, 7, 37, 1)], "{report}");
}

#[test]
fn empty_bom_only_and_whitespace_only_inputs_are_clean() {
    for bytes in [
        &b""[..],
        b"\xef\xbb\xbf",
        b"\n",
        b"\n\n",
        b"\r\n\r\n",
        b"   \n",
        b"  \n\n",
        b"\t\n\n",
    ] {
        let dir = project("main.verse", bytes);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
        assert_eq!(code, 0, "{bytes:?}: {report}");
        assert!(
            findings(&report).is_empty(),
            "no content means no excess: {bytes:?}: {report}"
        );
    }
}

/// A BOM shifts every byte offset by three while the reported lines stay the
/// same, so the position conversion has to exclude it.
#[test]
fn a_bom_prefixed_file_is_measured_after_the_bom() {
    let dir = project("main.verse", b"\xef\xbb\xbfA := 1\n\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(findings(&report), [(2, 3, 10, 1)], "{report}");

    let dir = project("main.verse", b"\xef\xbb\xbfA := 1\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");
}

#[test]
fn the_rule_is_off_by_default_and_reached_by_every_documented_selection() {
    let dir = project("main.verse", ONE_BLANK_LF);
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "off by default: {report}");

    for selection in ["V1005", "V1", "V"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert_eq!(findings(&report), [(2, 3, 7, 1)], "{selection}: {report}");
    }
    // The other families never bring it in.
    for selection in ["V2", "V3", "V1001,V1002,V1003,V1004"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert!(findings(&report).is_empty(), "{selection}: {report}");
    }

    let denied = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V1005",
            "--deny-warnings",
            "--output-format=json",
        ],
    );
    assert_eq!(denied.status.code(), Some(1), "{denied:?}");
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(findings(&denied), [(2, 3, 7, 1)]);
}

#[test]
fn fix_leaves_the_file_unchanged_and_the_finding_still_reported() {
    let dir = project("main.verse", ONE_BLANK_LF);
    let out = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V1005",
            "--fix",
            "--output-format=json",
        ],
    );
    // A warning that survives a fix run exits 0, and the bytes did not move.
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        fs::read(dir.path().join("main.verse")).unwrap(),
        ONE_BLANK_LF
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(findings(&report), [(2, 3, 7, 1)], "{report}");
    assert_eq!(report["summary"]["filesChanged"], 0, "{report}");

    // The preview mode states the same thing: no planned edit and no diff.
    let preview = run(
        dir.path(),
        &["main.verse", "--select", "V1005", "--fix", "--diff"],
    );
    assert_eq!(preview.status.code(), Some(0), "{preview:?}");
    assert_eq!(String::from_utf8(preview.stdout).unwrap(), "");
    assert_eq!(
        fs::read(dir.path().join("main.verse")).unwrap(),
        ONE_BLANK_LF
    );

    // The default family has nothing fixable here either, so the file stays put
    // even when V1001 and V1002 are the ones that could write.
    let dir = project("main.verse", b"A := 1\n\n");
    let out = run(dir.path(), &["main.verse", "--fix", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(
        fs::read(dir.path().join("main.verse")).unwrap(),
        b"A := 1\n\n"
    );
}

/// The rule is file-level, so a line directive naming it is refused with the
/// same wording V1002 uses and exit 2, whether or not V1005 is selected.
#[test]
fn line_suppression_is_forbidden_for_the_file_level_rule() {
    // `disable-next-line` alone, and `disable-line` on a line that has the code
    // its mode requires, so each reaches the file-level check rather than failing
    // earlier for a different reason.
    for source in [
        &b"# verse-lint: disable-next-line V1005 -- deliberate\r\nA := 1\n\n"[..],
        b"A := 1 # verse-lint: disable-line V1005 -- deliberate\n\n",
    ] {
        let dir = project("main.verse", source);
        for selection in ["V1005", "V1001"] {
            let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
            assert_eq!(code, 2, "{source:?} / {selection}: {report}");
            assert_eq!(report["summary"]["complete"], false, "{source:?}: {report}");
            assert!(findings(&report).is_empty(), "{source:?}: {report}");
            let message = report["executionErrors"][0]["message"].as_str().unwrap();
            assert!(
                message.contains("file-level V1005 cannot be suppressed by a line directive"),
                "{source:?}: {message}"
            );
            assert!(
                message.contains("use config ignore"),
                "{source:?}: {message}"
            );
        }
    }

    // The documented alternative removes the rule for the whole project.
    let dir = project("main.verse", ONE_BLANK_LF);
    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V1", "--ignore", "V1005"],
    );
    assert_eq!(code, 0, "{report}");
    assert!(findings(&report).is_empty(), "{report}");
    assert_eq!(report["summary"]["suppressed"], 0, "{report}");
}

#[test]
fn severity_override_and_sarif_metadata_follow_the_shared_plumbing() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.verse"), ONE_BLANK_LF).unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V1005']\n[lint.severity]\nV1005 = 'error'\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["diagnostics"][0]["severity"], "error", "{report}");
    assert_eq!(report["summary"]["errors"], 1, "{report}");
    assert_eq!(report["summary"]["warnings"], 0, "{report}");

    let sarif: Value =
        serde_json::from_slice(&run(dir.path(), &["main.verse", "--output-format=sarif"]).stdout)
            .unwrap();
    let descriptor = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V1005")
        .expect("V1005 driver entry");
    assert_eq!(descriptor["name"], "ExcessFinalBlankLines");
    assert_eq!(descriptor["defaultConfiguration"]["level"], "warning");
    assert_eq!(descriptor["defaultConfiguration"]["enabled"], false);
    assert_eq!(descriptor["properties"]["fixable"], false);
    assert_eq!(sarif["runs"][0]["results"][0]["level"], "error");
    let index = sarif["runs"][0]["results"][0]["ruleIndex"]
        .as_u64()
        .unwrap() as usize;
    assert_eq!(
        sarif["runs"][0]["tool"]["driver"]["rules"][index]["id"],
        "V1005"
    );
}

#[test]
fn text_json_and_sarif_agree_on_the_single_finding() {
    let dir = project("main.verse", ONE_BLANK_LF);
    let text = run(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(
        String::from_utf8(text.stdout).unwrap(),
        "main.verse:2:1: warning V1005 File ends with excess blank lines\n"
    );

    let (_, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        1,
        "{report}"
    );
    assert_eq!(report["summary"]["filesChecked"], 1, "{report}");
    assert_eq!(report["summary"]["complete"], true, "{report}");

    let sarif: Value = serde_json::from_slice(
        &run(
            dir.path(),
            &["main.verse", "--select", "V1005", "--output-format=sarif"],
        )
        .stdout,
    )
    .unwrap();
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(results.len(), 1, "{sarif}");
    assert_eq!(results[0]["ruleId"], "V1005");
    assert_eq!(results[0]["level"], "warning");
    let region = &results[0]["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 2);
    assert_eq!(region["startColumn"], 1);
    assert_eq!(region["endLine"], 3);
    assert_eq!(region["endColumn"], 1);
    assert_eq!(region["byteOffset"], 7);
    assert_eq!(region["byteLength"], 1);
    assert_eq!(sarif["runs"][0]["properties"]["summary"], report["summary"]);
}

#[test]
fn an_unanalysable_file_gets_an_execution_error_and_no_v1005() {
    for body in [
        // Unclosed delimiter: the lexer refuses the whole source.
        &b"A := (1\n\n"[..],
        // Unterminated block comment: the parse refuses the whole source.
        b"A := 1\n<# unterminated #>\n\n",
    ] {
        let dir = project("main.verse", body);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V1005"]);
        assert_eq!(code, 2, "{body:?}: {report}");
        assert!(findings(&report).is_empty(), "{body:?}: {report}");
        assert_eq!(report["summary"]["filesChecked"], 0, "{body:?}: {report}");
        assert_eq!(
            report["executionErrors"].as_array().unwrap().len(),
            1,
            "{body:?}"
        );
    }
}

/// The documents have to name the rule, its off-by-default warning contract and
/// its file-level suppression refusal, because those are the whole slice.
#[test]
fn the_documents_state_the_rule_contract() {
    let rule = include_str!("../docs/rules/v1005.md");
    assert!(rule.contains("**not** selected by default"), "{rule}");
    assert!(rule.contains("**no safe fix**"), "{rule}");
    assert!(rule.contains("file-level"), "{rule}");
    let index = include_str!("../docs/rules.md");
    assert!(index.contains("](rules/v1005.md)"), "{index}");
    assert!(index.contains("| Off | warning | None |"), "{index}");
    let cli = include_str!("../docs/cli.md");
    assert!(cli.contains("V1005"), "{cli}");
    // Comments and code are content: the anchor is the last non-trivia token,
    // and the docs must not claim a comment is excluded from it.
    for document in [rule, cli] {
        assert!(document.contains("non-trivia token"), "{document}");
        assert!(
            !document.contains("neither trivia nor a comment"),
            "{document}"
        );
    }
    let readme = include_str!("../README.md");
    assert!(readme.contains("[V1005][v1005]"), "{readme}");
    assert!(
        readme.contains(
            "[v1005]: https://github.com/eiei114/verse-lint/blob/main/docs/rules/v1005.md"
        ),
        "{readme}"
    );
}
