//! V2003 mixed-line-endings: one file-consistency warning at the first terminator
//! whose kind differs from the file's first terminator.
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

/// Every reported diagnostic as (line, column, endColumn, byteOffset, byteLength).
fn ranges(report: &Value) -> Vec<(u64, u64, u64, u64, u64)> {
    let diagnostics = report["diagnostics"].as_array().unwrap();
    for diagnostic in diagnostics {
        assert_eq!(diagnostic["ruleId"], "V2003", "{report}");
        assert_eq!(diagnostic["severity"], "warning", "{report}");
        assert_eq!(diagnostic["fixable"], false, "{report}");
        assert_eq!(
            diagnostic["message"], "File mixes CRLF and LF line endings",
            "{report}"
        );
    }
    diagnostics
        .iter()
        .map(|diagnostic| {
            let start = &diagnostic["range"]["start"];
            let end = &diagnostic["range"]["end"];
            (
                start["line"].as_u64().unwrap(),
                start["column"].as_u64().unwrap(),
                end["column"].as_u64().unwrap(),
                start["byteOffset"].as_u64().unwrap(),
                end["byteOffset"].as_u64().unwrap() - start["byteOffset"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// The one finding a mixed file produces: the mismatching terminator, not the
/// line's code.
const MIXED_LF_AFTER_CRLF: &[u8] = b"A := 1\r\nB := 2\n";
const MIXED_CRLF_AFTER_LF: &[u8] = b"A := 1\nB := 2\r\n";

#[test]
fn one_terminator_is_reported_for_a_mixed_file_in_either_direction() {
    let dir = project("main.verse", MIXED_LF_AFTER_CRLF);
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "a warning does not fail by default: {report}");
    // `A := 1\r\n` is 8 bytes, so the bare LF terminating line 2 is the 15th.
    assert_eq!(ranges(&report), [(2, 7, 1, 14, 1)]);
    assert_eq!(report["summary"]["warnings"], 1);
    assert_eq!(report["summary"]["errors"], 0);
    assert_eq!(report["summary"]["filesChecked"], 1);

    // The first terminator is the reference, so the CRLF comes second here.
    let dir = project("main.verse", MIXED_CRLF_AFTER_LF);
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report), [(2, 7, 1, 13, 2)]);
}

#[test]
fn a_large_mixed_file_still_produces_exactly_one_finding() {
    // Alternating terminators per line, so a per-line rule would flood this file.
    let mut bytes = Vec::new();
    for index in 0..500 {
        bytes.extend_from_slice(format!("A{index} := {index}").as_bytes());
        bytes.extend_from_slice(if index % 2 == 0 { b"\n" } else { b"\r\n" });
    }
    let dir = project("main.verse", &bytes);
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report).len(), 1, "{report}");
    // The first terminator is the LF ending line 1, so line 2's CRLF is reported.
    assert_eq!(ranges(&report), [(2, 8, 1, 15, 2)]);

    // Byte-level property: a terminator inside a block comment counts like one
    // between statements, and a string that spans lines is refused earlier.
    let dir = project("main.verse", b"<# note\r\nsecond #>\nA := 1\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report), [(2, 10, 1, 18, 1)]);
}

#[test]
fn consistent_empty_terminator_free_and_bom_prefixed_files_are_clean() {
    for bytes in [
        &b"A := 1\nB := 2\n"[..],
        b"A := 1\r\nB := 2\r\n",
        b"",
        b"\xef\xbb\xbf",
        b"A := 1",
        b"\r\n",
        b"\n",
        // A single CRLF terminator is one style, not a mixture.
        b"A := 1\r\n",
        b"\xef\xbb\xbfA := 1\r\nB := 2\r\n",
        b"\xef\xbb\xbfA := 1\nB := 2\n",
    ] {
        let dir = project("main.verse", bytes);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
        assert_eq!(code, 0, "{bytes:?}: {report}");
        assert!(
            report["diagnostics"].as_array().unwrap().is_empty(),
            "{bytes:?}: {report}"
        );
    }
}

#[test]
fn a_crlf_only_file_with_one_trailing_lf_is_still_mixed() {
    // Every line is CRLF except the last, which is a bare LF: the file mixes.
    let dir = project("main.verse", b"A := 1\r\nB := 2\r\nC := 3\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report), [(3, 7, 1, 22, 1)]);

    // The mirror case: an LF-only file whose final terminator is a CRLF.
    let dir = project("main.verse", b"A := 1\nB := 2\nC := 3\r\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report), [(3, 7, 1, 20, 2)]);
}

#[test]
fn selection_default_and_warning_semantics_follow_the_shared_plumbing() {
    let dir = project("main.verse", MIXED_LF_AFTER_CRLF);
    // Off by default: the default family never mentions line endings.
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert!(ranges(&report).is_empty(), "{report}");

    for selection in ["V2003", "V2", "V"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert_eq!(ranges(&report), [(2, 7, 1, 14, 1)], "{selection}");
    }
    // The V1 family and the V3 semantic family never bring this rule in.
    for selection in ["V1", "V3", "V1001,V2001,V3001,V3002"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert!(ranges(&report).is_empty(), "{selection}: {report}");
    }

    let denied = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V2003",
            "--deny-warnings",
            "--output-format=json",
        ],
    );
    assert_eq!(denied.status.code(), Some(1), "{denied:?}");
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(ranges(&denied), [(2, 7, 1, 14, 1)]);
}

#[test]
fn severity_override_promotes_the_warning_to_a_hard_failure() {
    let dir = project("main.verse", MIXED_LF_AFTER_CRLF);
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V2003']\n[lint.severity]\nV2003 = 'error'\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["summary"]["errors"], 1);
    assert_eq!(report["summary"]["warnings"], 0);
    assert_eq!(report["diagnostics"][0]["severity"], "error", "{report}");
    // The tool default in SARIF driver metadata stays `warning`.
    let sarif = run(dir.path(), &["main.verse", "--output-format=sarif"]);
    let sarif: Value = serde_json::from_slice(&sarif.stdout).unwrap();
    let descriptor = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V2003")
        .expect("V2003 driver entry");
    assert_eq!(descriptor["defaultConfiguration"]["level"], "warning");
    assert_eq!(descriptor["defaultConfiguration"]["enabled"], false);
    assert_eq!(descriptor["name"], "MixedLineEndings");
    assert_eq!(descriptor["properties"]["fixable"], false);
    assert_eq!(sarif["runs"][0]["results"][0]["level"], "error");
}

#[test]
fn line_directive_and_ignore_suppress_the_rule_without_changing_detection() {
    // The diagnosed line is the one the mismatching terminator ends.
    let dir = project(
        "main.verse",
        b"A := 1\r\n# verse-lint: disable-next-line V2003 -- normalised at the source\r\nB := 2\n",
    );
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(code, 0, "{report}");
    assert!(ranges(&report).is_empty(), "{report}");
    assert_eq!(report["summary"]["suppressed"], 1, "{report}");

    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V2003", "--show-suppressed"],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report), [(3, 7, 1, 81, 1)]);
    assert_eq!(report["diagnostics"][0]["suppressed"], true, "{report}");

    // `--ignore V2003` removes the whole rule, directive or not.
    let dir = project("main.verse", MIXED_LF_AFTER_CRLF);
    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V", "--ignore", "V2003"],
    );
    assert_eq!(code, 0, "{report}");
    assert!(ranges(&report).is_empty(), "{report}");

    // The config-level path stays falsifiable only when V2003 is inside the
    // active set: `ignore` subtracts from a selection, so selecting just the
    // default V1 family and ignoring V2003 would pass even if `[lint] ignore`
    // did nothing. Select V1001 (which has nothing to report here) alongside
    // V2003 and run the same fixture with and without the ignore entry.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V1001', 'V2003']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(ranges(&report), [(2, 7, 1, 14, 1)], "{report}");
    assert_eq!(report["summary"]["warnings"], 1, "{report}");

    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V1001', 'V2003']\nignore = ['V2003']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert!(ranges(&report).is_empty(), "{report}");
    // Configuration exclusion is selection, not a suppression directive.
    assert_eq!(report["summary"]["suppressed"], 0, "{report}");
    assert_eq!(report["summary"]["filesChecked"], 1, "{report}");
}

#[test]
fn an_unanalysable_file_gets_an_execution_error_and_no_v2003() {
    // Mixed endings plus source this run cannot analyze: the ordinary coverage
    // failure is reported and the line-ending rule stays silent about the file.
    for body in [
        &b"A := \"# verse-lint: invalid\"\r\nB := 1\n"[..],
        b"A := 1\r\n<# unterminated #>B := 2\n",
    ] {
        let dir = project("main.verse", body);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
        assert_eq!(code, 2, "{body:?}: {report}");
        assert!(ranges(&report).is_empty(), "{body:?}: {report}");
        assert_eq!(report["summary"]["complete"], false, "{body:?}");
        assert_eq!(report["summary"]["filesChecked"], 0, "{body:?}");
        assert_eq!(
            report["executionErrors"].as_array().unwrap().len(),
            1,
            "{body:?}"
        );
    }
}

#[test]
fn text_json_and_sarif_agree_on_the_single_finding() {
    let dir = project("main.verse", MIXED_LF_AFTER_CRLF);
    let text = run(dir.path(), &["main.verse", "--select", "V2003"]);
    assert_eq!(
        String::from_utf8(text.stdout).unwrap(),
        "main.verse:2:7: warning V2003 File mixes CRLF and LF line endings\n"
    );
    let (_, report) = json(dir.path(), &["main.verse", "--select", "V2003"]);
    let sarif: Value = serde_json::from_slice(
        &run(
            dir.path(),
            &["main.verse", "--select", "V2003", "--output-format=sarif"],
        )
        .stdout,
    )
    .unwrap();
    let results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        report["diagnostics"].as_array().unwrap().len()
    );
    assert_eq!(results[0]["ruleId"], "V2003");
    assert_eq!(results[0]["level"], "warning");
    let region = &results[0]["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 2);
    assert_eq!(region["startColumn"], 7);
    assert_eq!(region["endLine"], 3);
    assert_eq!(region["endColumn"], 1);
    assert_eq!(region["byteOffset"], 14);
    assert_eq!(region["byteLength"], 1);
    assert_eq!(sarif["runs"][0]["properties"]["summary"], report["summary"]);
}
