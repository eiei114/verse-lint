//! `--fix --diff` contract: the preview runs the same analysis a write run runs,
//! renders a unified diff on stdout, and changes no byte on disk.
//!
//! Every case reads the file back after the run, because "writes nothing" is the
//! one promise a preview cannot express in its own output: a printing bug and a
//! write bug look identical unless the bytes are compared.
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
        .expect("launch verse-lint")
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).unwrap_or_else(|e| panic!("{e}: {out:?}"))
}

/// The JSON envelope of one run, so a preview's text output can be checked
/// against the machine report the write path produces.
fn json_run(root: &Path, args: &[&str]) -> (i32, Value) {
    let mut args = args.to_vec();
    args.push("--output-format=json");
    let out = run(root, &args);
    let value = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (out.status.code().unwrap(), value)
}

#[test]
fn preview_changes_no_bytes_and_renders_both_fix_kinds() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.verse");
    let original = b"A := 1  \nB := 2  ";
    fs::write(&path, original).unwrap();

    let out = run(dir.path(), &["a.verse", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(fs::read(&path).unwrap(), original);
    // The trailing-whitespace edit and the final-newline insert merge into one
    // hunk here, because only one unchanged line separates them. The marker is
    // what makes the newline-only row visible at all.
    assert_eq!(
        stdout(&out),
        "--- a/a.verse\n+++ b/a.verse\n@@ -1,2 +1,2 @@\n-A := 1  \n+A := 1\n-B := 2  \n\\ No newline at end of file\n+B := 2\n"
    );
    assert!(stderr(&out).is_empty(), "{out:?}");
}

#[test]
fn preview_reports_the_candidate_diagnostics_a_write_run_keeps() {
    let write_dir = tempfile::tempdir().unwrap();
    let preview_dir = tempfile::tempdir().unwrap();
    // A tab-indented line is not fixable, so one finding survives the plan and
    // its position must follow the candidate source in both modes.
    let source = "f():void =  \n\tPrint(\"x\")  ";
    fs::write(write_dir.path().join("a.verse"), source).unwrap();
    fs::write(preview_dir.path().join("a.verse"), source).unwrap();

    let (write_code, written) = json_run(write_dir.path(), &["a.verse", "--fix"]);
    assert_eq!(write_code, 1, "{written}");
    assert_eq!(written["summary"]["filesChanged"], 1, "{written}");
    assert_eq!(
        fs::read(write_dir.path().join("a.verse")).unwrap(),
        b"f():void =\n\tPrint(\"x\")\n"
    );

    let out = run(preview_dir.path(), &["a.verse", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert_eq!(
        fs::read(preview_dir.path().join("a.verse")).unwrap(),
        source.as_bytes()
    );
    // The tab line loses its trailing spaces in the candidate, so the remaining
    // tab finding sits at the same place the written file reports.
    let expected = &written["diagnostics"][0];
    assert!(
        expected["ruleId"] == "V1003",
        "the surviving finding is the unfixable tab rule: {expected}"
    );
    let line = format!(
        "a.verse:{}:{}: {} {} ",
        expected["range"]["start"]["line"].as_u64().unwrap(),
        expected["range"]["start"]["column"].as_u64().unwrap(),
        expected["severity"].as_str().unwrap(),
        expected["ruleId"].as_str().unwrap(),
    );
    assert!(stderr(&out).contains(&line), "{expected}\n{out:?}");
}

#[test]
fn text_diagnostics_go_to_stderr_while_stdout_is_only_the_diff() {
    let dir = tempfile::tempdir().unwrap();
    // V1003 (tab indentation) is not fixable, so it survives the fix plan.
    fs::write(dir.path().join("a.verse"), "A := 1  \n\tB := 2\n").unwrap();

    let out = run(dir.path(), &["a.verse", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let diff = stdout(&out);
    assert!(diff.starts_with("--- a/a.verse\n+++ b/a.verse\n"), "{diff}");
    assert!(
        !diff.contains("V1003"),
        "a diagnostic leaked into stdout: {diff}"
    );
    for line in diff.lines() {
        assert!(
            line.starts_with("--- ")
                || line.starts_with("+++ ")
                || line.starts_with("@@ ")
                || line.starts_with(' ')
                || line.starts_with('-')
                || line.starts_with('+')
                || line == "\\ No newline at end of file",
            "not a diff line: {line:?}"
        );
    }
    let err = stderr(&out);
    assert!(
        err.contains("a.verse:2:1: error V1003 Tab in structural indentation"),
        "the remaining finding must still be reported: {err}"
    );
    assert_eq!(
        fs::read(dir.path().join("a.verse")).unwrap(),
        b"A := 1  \n\tB := 2\n"
    );
}

#[test]
fn a_clean_tree_prints_no_diff_and_still_exits_zero() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1\n").unwrap();
    let out = run(dir.path(), &["a.verse", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stdout.is_empty(), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
}

#[test]
fn one_hunk_header_per_changed_file_in_path_order() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("b.verse"), b"B := 1  ").unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1  ").unwrap();
    fs::write(dir.path().join("c.verse"), b"C := 1\n").unwrap();
    let out = run(dir.path(), &[".", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let diff = stdout(&out);
    assert_eq!(
        diff.matches("--- a/").count(),
        2,
        "one header per changed file: {diff}"
    );
    let a = diff.find("--- a/a.verse").expect("a.verse is previewed");
    let b = diff.find("--- a/b.verse").expect("b.verse is previewed");
    assert!(a < b, "discovery order, not filesystem order: {diff}");
    assert!(
        diff.contains("@@ -1,1 +1,1 @@\n-A := 1  \n\\ No newline"),
        "{diff}"
    );
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), b"A := 1  ");
    assert_eq!(fs::read(dir.path().join("b.verse")).unwrap(), b"B := 1  ");
}

#[test]
fn crlf_and_bom_sources_render_readably_and_keep_their_bytes_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crlf.verse");
    let original = "\u{feff}A:=\"日😀\"  \r\n# comment  ";
    fs::write(&path, original).unwrap();

    let out = run(dir.path(), &["crlf.verse", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let diff = stdout(&out);
    assert!(
        diff.contains("@@ -1,2 +1,2 @@"),
        "one hunk for the two changed lines: {diff}"
    );
    // The removal shows the trailing spaces but neither the BOM nor the CR, and
    // the final-newline insertion is the last changed row.
    assert!(diff.contains("\n-A:=\"日😀\"  \n+A:=\"日😀\"\n"), "{diff}");
    assert!(
        diff.contains("\n-# comment  \n\\ No newline at end of file\n+# comment  \n"),
        "{diff}"
    );
    assert!(!diff.contains('\r'), "raw CR reached the preview: {diff:?}");
    assert!(
        !diff.contains('\u{feff}'),
        "raw BOM reached the preview: {diff:?}"
    );
    assert_eq!(fs::read(&path).unwrap(), original.as_bytes());

    // A real write still preserves both, which is the contract the preview
    // claims to describe.
    let written = run(dir.path(), &["crlf.verse", "--fix"]);
    assert_eq!(written.status.code(), Some(0), "{written:?}");
    assert_eq!(
        fs::read(&path).unwrap(),
        "\u{feff}A:=\"日😀\"\r\n# comment  \r\n".as_bytes()
    );
}

#[cfg(windows)]
#[test]
fn a_read_only_target_previews_the_change_a_real_fix_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let locked = dir.path().join("locked.verse");
    fs::write(&locked, b"Z := 2  ").unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1  ").unwrap();
    let permissions = fs::metadata(&locked).unwrap().permissions();
    let mut readonly = permissions.clone();
    readonly.set_readonly(true);
    fs::set_permissions(&locked, readonly).unwrap();

    let out = run(dir.path(), &["a.verse", "locked.verse", "--fix", "--diff"]);
    let restored = fs::set_permissions(&locked, permissions.clone());
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    restored.unwrap();
    let diff = stdout(&out);
    assert!(diff.contains("--- a/a.verse"), "{diff}");
    assert!(diff.contains("--- a/locked.verse"), "{diff}");
    assert_eq!(fs::read(&locked).unwrap(), b"Z := 2  ");
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), b"A := 1  ");

    // The same run without `--diff` refuses the batch and leaves both originals.
    let mut readonly = fs::metadata(&locked).unwrap().permissions();
    readonly.set_readonly(true);
    fs::set_permissions(&locked, readonly).unwrap();
    let out = run(dir.path(), &["a.verse", "locked.verse", "--fix"]);
    let restored = fs::set_permissions(&locked, permissions);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    restored.unwrap();
    assert_eq!(fs::read(&locked).unwrap(), b"Z := 2  ");
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), b"A := 1  ");
}

#[test]
fn every_documented_rejected_combination_exits_two_and_changes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.verse");
    fs::write(&path, b"A := 1  ").unwrap();
    for args in [
        vec!["a.verse", "--diff"],
        vec!["-", "--fix", "--diff"],
        vec!["a.verse", "--fix", "--diff", "--output-format=json"],
        vec!["a.verse", "--fix", "--diff", "--output-format=sarif"],
        vec!["a.verse", "--fix", "--diff", "--output-format=github"],
        vec!["a.verse", "--fix", "--diff", "--show-config"],
    ] {
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
        assert!(!out.stderr.is_empty(), "{args:?} was silent");
        assert_eq!(fs::read(&path).unwrap(), b"A := 1  ", "{args:?}");
    }
    // The refusals are about the combination, not about the flags: naming the
    // default text format explicitly is still the accepted spelling.
    let out = run(
        dir.path(),
        &["a.verse", "--fix", "--diff", "--output-format=text"],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(fs::read(&path).unwrap(), b"A := 1  ");
}

#[test]
fn the_diff_without_fix_message_names_the_missing_flag() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1  ").unwrap();
    let out = run(dir.path(), &["a.verse", "--diff"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let err = stderr(&out);
    assert!(err.contains("--diff"), "{err}");
    assert!(err.contains("--fix"), "{err}");
    assert!(out.stdout.is_empty(), "{out:?}");
}

#[test]
fn incomplete_inspection_still_exits_two_and_prints_no_diff() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1  ").unwrap();
    fs::write(dir.path().join("z.verse"), b"<# not closed").unwrap();
    let out = run(dir.path(), &["a.verse", "z.verse", "--fix", "--diff"]);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(
        out.stdout.is_empty(),
        "a failed plan must not print a partial diff: {out:?}"
    );
    assert!(stderr(&out).contains("z.verse"), "{out:?}");
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), b"A := 1  ");
}

#[cfg(windows)]
#[test]
fn repeated_previews_leave_the_file_and_its_mtime_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.verse");
    fs::write(&path, b"A := 1  ").unwrap();
    let before = fs::metadata(&path).unwrap().modified().unwrap();
    for _ in 0..3 {
        let out = run(dir.path(), &["a.verse", "--fix", "--diff"]);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        assert_eq!(stdout(&out).matches("@@ -").count(), 1, "{out:?}");
    }
    assert_eq!(fs::read(&path).unwrap(), b"A := 1  ");
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), before);
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        1,
        "a preview must leave no staging, backup or recovery file"
    );
}

#[test]
fn help_lists_the_preview_flag_with_its_requirement() {
    let out = run(Path::new("."), &["--help"]);
    assert!(out.status.success(), "{out:?}");
    let help = stdout(&out);
    // Clap wraps the description onto its own line, so the flag line alone does
    // not carry the requirement: read the flag and the text after it.
    let start = help
        .find("\n      --diff")
        .unwrap_or_else(|| panic!("missing --diff: {help}"));
    let block = &help[start..];
    let block = &block[..block
        .find("\n      --select")
        .expect("the next option ends it")];
    assert!(
        block.contains("--fix"),
        "the help must say the flag belongs to --fix: {block}"
    );
}
