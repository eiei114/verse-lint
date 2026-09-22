use std::{
    io::Write,
    process::{Command, Output, Stdio},
};

fn stdin(bytes: &[u8], extra: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .arg("-")
        .args(["--select", "V1001"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn trailing_whitespace_has_stable_text_and_exit_codes() {
    let out = stdin(b"A := 1  \n", &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "<stdin>:1:7: error V1001 Trailing whitespace\n"
    );
    assert!(out.stderr.is_empty());
    assert!(stdin(b"A := 1\n", &[]).status.success());
}

#[test]
fn strings_comments_and_interpolation_are_protected() {
    for source in [
        "A := \"spaces   \"\n",
        "# trailing comment spaces   \n",
        "<# nested <# comment #>   \n end #>\nA := 1\n",
        "A := \"{Foo(\"inside   \")}\"\n",
        "",
        "\u{feff}",
    ] {
        let out = stdin(source.as_bytes(), &[]);
        assert!(
            out.status.success(),
            "{source:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn pinned_shared_corpus_is_accepted_without_formatter_runtime() {
    for source in [
        include_bytes!("fixtures/device.input.verse").as_slice(),
        include_bytes!("fixtures/device.expected.verse").as_slice(),
    ] {
        let out = stdin(source, &[]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.stdout.is_empty());
    }
}

#[test]
fn trailing_trivia_variants_and_protected_counterexamples() {
    for (source, count) in [
        ("A := 1 \n", 1),
        ("A := 1\t\n", 1),
        ("A := 1 \t\r\n", 1),
        ("A := 1  ", 1),
        (" \n", 1),
        ("A := ' ' \n", 1),
        ("<# comment #> \n", 1),
        ("# comment   ", 0),
        ("<# protected   \n more #>\n", 0),
        ("A := \"\\t\"\n", 0),
        ("A := 1 \nB := 2 \n", 2),
    ] {
        let out = stdin(source.as_bytes(), &["--output-format=json"]);
        assert_eq!(
            out.status.code(),
            Some(u8::from(count > 0).into()),
            "{source:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            value["diagnostics"].as_array().unwrap().len(),
            count,
            "{source:?}"
        );
    }
}

#[test]
fn virtual_stdin_uses_parent_config_and_is_not_a_real_write_target() {
    use std::process::{Command, Stdio};
    let dir = tempfile::tempdir().unwrap();
    let child_dir = dir.path().join("nested");
    std::fs::create_dir(&child_dir).unwrap();
    std::fs::write(
        child_dir.join("verse.toml"),
        "[lint]\nselect=['V1001']\nignore=['V1001']\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args([
            "-",
            "--stdin-filepath",
            "nested/virtual.verse",
            "--output-format=json",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"A := 1\n").unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("no active lint rules")
    );
    assert!(!child_dir.join("virtual.verse").exists());
}

#[test]
fn json_range_counts_bytes_and_unicode_scalars_without_bom() {
    let source = "\u{feff}A := \"日😀e\u{301}\"  \r\n";
    let out = stdin(
        source.as_bytes(),
        &["--output-format", "json", "--color", "always"],
    );
    assert_eq!(out.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let diagnostic = &value["diagnostics"][0];
    assert_eq!(
        diagnostic["range"]["start"]["byteOffset"],
        source.find("  ").unwrap()
    );
    assert_eq!(diagnostic["range"]["start"]["column"], 12);
    assert_eq!(diagnostic["range"]["end"]["column"], 14);
    assert_eq!(diagnostic["fixable"], true);
    assert_eq!(value["summary"]["filesChecked"], 1);
    assert_eq!(value["summary"]["complete"], true);
    assert!(!out.stdout.contains(&27));
}

#[test]
fn malformed_input_is_execution_failure_not_a_lint_rule() {
    for source in [
        &b"A := \"unfinished"[..],
        b"<# never closed",
        b"A := 1\r\nB := 2\n",
        b"\xff\xfe",
    ] {
        let out = stdin(source, &["--output-format=json"]);
        assert_eq!(out.status.code(), Some(2));
        let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(value["summary"]["complete"], false);
        assert!(value["diagnostics"].as_array().unwrap().is_empty());
        assert!(!value["executionErrors"].as_array().unwrap().is_empty());
    }
}

#[test]
fn real_file_and_readonly_inspection_do_not_modify_bytes_or_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("日本語 space 😀.verse");
    std::fs::write(&path, "A := 1 \r\n").unwrap();
    let time = std::fs::metadata(&path).unwrap().modified().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .starts_with("日本語 space 😀.verse:1:7:")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"A := 1 \r\n");
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), time);
}
