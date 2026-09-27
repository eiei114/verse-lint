use serde_json::Value;
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
};
fn stdin(source: &str, extra: &[&str]) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args(["-", "--output-format=sarif"])
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
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (out.status.code().unwrap(), v)
}
#[test]
fn unicode_stdin_has_regions_but_no_fabricated_file_uri_or_edits() {
    let source = "\u{feff}A := \"日😀e\u{301}\"  \r\n";
    let (code, v) = stdin(
        source,
        &[
            "--stdin-filepath",
            "C:/private/日本語.verse",
            "--color=always",
        ],
    );
    assert_eq!(code, 1);
    assert_eq!(v["version"], "2.1.0");
    let run = &v["runs"][0];
    assert_eq!(run["columnKind"], "unicodeCodePoints");
    assert_eq!(run["tool"]["driver"]["rules"].as_array().unwrap().len(), 5);
    let result = &run["results"][0];
    assert_eq!(result["ruleId"], "V1001");
    assert!(result.get("fixes").is_none());
    let location = &result["locations"][0]["physicalLocation"];
    assert!(location["artifactLocation"].get("uri").is_none());
    assert_eq!(location["artifactLocation"]["index"], 0);
    assert_eq!(location["region"]["startColumn"], 12);
    assert_eq!(location["region"]["endColumn"], 14);
    assert_eq!(location["region"]["byteOffset"], source.find("  ").unwrap());
    assert_eq!(location["region"]["byteLength"], 2);
    assert!(run["artifacts"][0].get("location").is_none());
    assert!(run.get("originalUriBaseIds").is_none());
    assert_eq!(run["invocations"][0]["executionSuccessful"], true);
    assert_eq!(run["invocations"][0]["exitCode"], 1);
}
#[test]
fn project_uris_are_encoded_and_counts_match_json_and_text() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("日 space #%.verse"), "A := 1  \n# TODO later\n").unwrap();
    let run = |format| {
        Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(root)
            .args([".", "--select=V1001,V2002", "--output-format", format])
            .output()
            .unwrap()
    };
    let a = run("sarif");
    let b = run("sarif");
    assert_eq!(a.stdout, b.stdout);
    let v: Value = serde_json::from_slice(&a.stdout).unwrap();
    let j: Value = serde_json::from_slice(&run("json").stdout).unwrap();
    let sarif = &v["runs"][0];
    assert_eq!(
        sarif["results"].as_array().unwrap().len(),
        j["diagnostics"].as_array().unwrap().len()
    );
    assert_eq!(
        String::from_utf8(run("text").stdout)
            .unwrap()
            .lines()
            .count(),
        2
    );
    let location = &sarif["artifacts"][0]["location"];
    assert_eq!(location["uri"], "%E6%97%A5%20space%20%23%25.verse");
    assert_eq!(location["uriBaseId"], "%SRCROOT%");
    assert!(
        sarif["originalUriBaseIds"]["%SRCROOT%"]["uri"]
            .as_str()
            .unwrap()
            .starts_with("file:///")
    );
    assert_eq!(sarif["properties"]["summary"], j["summary"]);
    assert_eq!(sarif["results"][1]["level"], "warning");
}
#[test]
fn execution_failures_are_notifications_and_partial_findings_survive() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), "A:=1  \n").unwrap();
    for args in [
        vec![".", "absent.verse"],
        vec!["--unknown"],
        vec!["--config", "missing.toml"],
        vec!["-", "--fix"],
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .args(&args)
            .arg("--output-format=sarif")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2));
        let v: Value = serde_json::from_slice(&out.stdout).unwrap();
        let run = &v["runs"][0];
        assert_eq!(run["invocations"][0]["executionSuccessful"], false);
        assert!(
            !run["invocations"][0]["toolExecutionNotifications"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        if args[0] == "." {
            assert_eq!(run["results"].as_array().unwrap().len(), 1);
        }
    }
    let (code, v) = stdin("<# incomplete", &[]);
    assert_eq!(code, 2);
    assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
}
#[test]
fn warnings_are_successful_execution_even_when_denied_and_suppression_has_no_results() {
    let (code, v) = stdin("# TODO later\n", &["--select=V2002", "--deny-warnings"]);
    assert_eq!(code, 1);
    assert_eq!(v["runs"][0]["invocations"][0]["executionSuccessful"], true);
    let (code, v) = stdin(
        "# verse-lint: disable-next-line V1001 -- test\nA:=1  \n",
        &[],
    );
    assert_eq!(code, 0);
    assert_eq!(v["runs"][0]["properties"]["summary"]["suppressed"], 1);
    assert!(v["runs"][0]["results"].as_array().unwrap().is_empty());
}
