//! `--output-format github` emits GitHub Actions workflow commands and nothing
//! else. Every case drives the real binary, because the contract is about what
//! lands on stdout, how it is escaped and which exit code accompanies it.
//!
//! Message escaping is proven at the unit level in `src/github.rs`: rule
//! messages come from the static registry and never contain a newline, so the
//! only way to exercise the message table end to end would be to invent a
//! diagnostic the linter cannot produce. Property escaping is exercised here
//! with a real virtual stdin path, which can hold `,`, `:` and a newline.
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

fn stdout(root: &Path, args: &[&str]) -> (i32, String) {
    let out = run(root, args);
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
    )
}

fn json(root: &Path, args: &[&str]) -> (i32, Value) {
    let out = run(root, &[args, &["--output-format=json"]].concat());
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code().unwrap(), value)
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("verse.toml"), "schema-version = 1\n").unwrap();
    for (name, body) in files {
        fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

/// The registry name of every rule the fixtures can report, keyed by ID.
fn rule_name(id: &str) -> &'static str {
    match id {
        "V1001" => "TrailingWhitespace",
        "V1002" => "MissingFinalNewline",
        "V1003" => "TabIndentation",
        "V2001" => "LineTooLong",
        "V2002" => "BareTodoComment",
        other => panic!("add {other} to this fixture table"),
    }
}

/// The exact annotation JSON says the binary must print, derived from the JSON
/// payload so a region that drifts in one format fails the comparison.
fn expected_annotation(diagnostic: &Value) -> String {
    let command = if diagnostic["suppressed"] == true {
        "notice"
    } else {
        match diagnostic["severity"].as_str().unwrap() {
            "error" => "error",
            "warning" => "warning",
            other => panic!("unexpected severity {other}"),
        }
    };
    let start = &diagnostic["range"]["start"];
    let end = &diagnostic["range"]["end"];
    format!(
        "::{command} file={},line={},col={},endLine={},endColumn={},title={} {}::{}",
        diagnostic["path"].as_str().unwrap(),
        start["line"],
        start["column"],
        end["line"],
        end["column"],
        diagnostic["ruleId"].as_str().unwrap(),
        rule_name(diagnostic["ruleId"].as_str().unwrap()),
        diagnostic["message"].as_str().unwrap(),
    )
}

/// One active error, one active warning, one suppressed finding: enough that a
/// wrong command level, ordering or count is visible.
const MIXED: &str = concat!(
    "A := 1  \n",
    "# verse-lint: disable-next-line V1001 -- kept for the review job\n",
    "B := 2  \n",
    "# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
);

#[test]
fn a_nested_configuration_root_keeps_config_relative_paths() {
    // GitHub resolves `file=` against the repository root, so a nested config
    // root cannot line up. The format must still report the same path as text
    // and JSON — a documented limitation, not a silent rewrite, and never an
    // absolute path (which GitHub would reject outright).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir_all(root.join("Content/Sub")).unwrap();
    fs::write(root.join("Content/verse.toml"), "schema-version = 1\n").unwrap();
    fs::write(root.join("Content/Sub/device.verse"), "A := 1  \n").unwrap();

    // Run with an explicit nested configuration root. Upward discovery would
    // take the CWD as the root, so `--config` is the way a nested root arises.
    let args = ["Content", "--config", "Content/verse.toml"];
    let (code, out) = stdout(root, &[&args[..], &["--output-format=github"]].concat());
    assert_eq!(code, 1);
    assert!(
        out.starts_with("::error file=Sub/device.verse,line=1,"),
        "a nested config root keeps the reported path relative to it: {out}"
    );
    assert!(!out.contains("Content/Sub/device.verse"), "{out}");
    assert!(
        !out.contains(":\\") && !out.contains(":/") && !out.contains("file=/"),
        "an absolute path would be unusable in an annotation: {out}"
    );

    // Text, JSON and SARIF are unchanged: the same config-relative path.
    let (_, text) = stdout(root, &args);
    assert!(text.starts_with("Sub/device.verse:"), "{text}");
    let (_, report) = json(root, &args);
    assert_eq!(report["diagnostics"][0]["path"], "Sub/device.verse");
    let sarif: Value = serde_json::from_slice(
        &run(root, &[&args[..], &["--output-format=sarif"]].concat()).stdout,
    )
    .unwrap();
    assert_eq!(
        sarif["runs"][0]["artifacts"][0]["location"]["uri"], "Sub/device.verse",
        "SARIF keeps the config-relative artifact path too"
    );
}

#[test]
fn a_clean_run_prints_nothing_and_exits_zero() {
    let dir = project(&[("clean.verse", "A := 1\n")]);
    let (code, out) = stdout(dir.path(), &[".", "--output-format=github"]);
    assert_eq!(code, 0);
    assert_eq!(out, "", "a clean run must not print a banner or summary");
}

#[test]
fn every_annotation_line_carries_the_documented_region_and_level() {
    let dir = project(&[("a.verse", MIXED)]);
    let args = [
        "a.verse",
        "--select",
        "V1001,V2001",
        "--show-suppressed",
        "--output-format=github",
    ];
    let (code, out) = stdout(dir.path(), &args);
    assert_eq!(code, 1);
    let (_, report) = json(dir.path(), &args[..4]);

    let lines: Vec<&str> = out.lines().collect();
    let expected: Vec<String> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(expected_annotation)
        .collect();
    assert_eq!(
        lines, expected,
        "stdout must be exactly one annotation per reported diagnostic, in JSON order"
    );

    // Pin the shape itself, so agreeing with JSON cannot hide a format change.
    assert_eq!(
        out,
        "::error file=a.verse,line=1,col=7,endLine=1,endColumn=9,title=V1001 TrailingWhitespace::Trailing whitespace\n\
         ::notice file=a.verse,line=3,col=7,endLine=3,endColumn=9,title=V1001 TrailingWhitespace::Trailing whitespace\n\
         ::warning file=a.verse,line=4,col=121,endLine=4,endColumn=132,title=V2001 LineTooLong::Line exceeds configured maximum length\n"
    );

    // Every line is a workflow command this slice is allowed to emit.
    for line in &lines {
        assert!(
            ["::error ", "::warning ", "::notice "]
                .iter()
                .any(|prefix| line.starts_with(prefix)),
            "unexpected stdout line: {line}"
        );
        for forbidden in ["::group::", "::endgroup::", "::set-output", "::add-matcher"] {
            assert!(
                !line.contains(forbidden),
                "{forbidden} is not part of this format: {line}"
            );
        }
    }
}

#[test]
fn region_numbers_match_sarif_and_text_ordering_for_the_same_input() {
    let dir = project(&[
        ("a.verse", "# TODO later\nA := 1  \n"),
        ("b.verse", "B := 2  \n"),
    ]);
    let args = [".", "--select", "V1001,V2002"];
    let (_, github) = stdout(
        dir.path(),
        &[&args[..], &["--output-format=github"]].concat(),
    );
    let (_, report) = json(dir.path(), &args);
    let sarif: Value = serde_json::from_slice(
        &run(
            dir.path(),
            &[&args[..], &["--output-format=sarif"]].concat(),
        )
        .stdout,
    )
    .unwrap();
    let (_, text) = stdout(dir.path(), &args);

    let diagnostics = report["diagnostics"].as_array().unwrap();
    let lines: Vec<&str> = github.lines().collect();
    assert_eq!(lines.len(), diagnostics.len());
    assert_eq!(lines.len(), text.lines().count());
    for (index, diagnostic) in diagnostics.iter().enumerate() {
        let line = lines[index];
        let start = &diagnostic["range"]["start"];
        let end = &diagnostic["range"]["end"];
        assert!(
            line.starts_with(&format!(
                "::{} file={},line={},col={},endLine={},endColumn={},",
                if diagnostic["severity"] == "error" {
                    "error"
                } else {
                    "warning"
                },
                diagnostic["path"].as_str().unwrap(),
                start["line"],
                start["column"],
                end["line"],
                end["column"]
            )),
            "{line} must repeat the JSON region"
        );
        // The same source order as text output, which prints path then position.
        let text_path = text.lines().nth(index).unwrap();
        assert!(
            text_path.starts_with(diagnostic["path"].as_str().unwrap()),
            "{index}: text and github must agree on order"
        );
    }

    let sarif_results = sarif["runs"][0]["results"].as_array().unwrap();
    assert_eq!(sarif_results.len(), diagnostics.len());
    for (result, diagnostic) in sarif_results.iter().zip(diagnostics) {
        let region = &result["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], diagnostic["range"]["start"]["line"]);
        assert_eq!(
            region["startColumn"],
            diagnostic["range"]["start"]["column"]
        );
        assert_eq!(region["endLine"], diagnostic["range"]["end"]["line"]);
        assert_eq!(region["endColumn"], diagnostic["range"]["end"]["column"]);
    }
}

#[test]
fn suppressed_findings_are_notices_and_absent_without_the_flag() {
    let dir = project(&[("a.verse", MIXED)]);
    let (code, without) = stdout(
        dir.path(),
        &["a.verse", "--select", "V1001", "--output-format=github"],
    );
    assert_eq!(code, 1);
    assert!(
        !without.contains("::notice"),
        "a suppressed finding must stay out of a default run: {without}"
    );
    assert_eq!(
        without.lines().count(),
        1,
        "only the active finding is annotated: {without}"
    );

    let (code, with) = stdout(
        dir.path(),
        &[
            "a.verse",
            "--select",
            "V1001",
            "--show-suppressed",
            "--output-format=github",
        ],
    );
    assert_eq!(code, 1, "the flag must not change the exit code");
    assert!(with.contains("::notice file=a.verse,line=3,"), "{with}");
    assert_eq!(
        with.lines().filter(|l| l.starts_with("::notice ")).count(),
        1
    );

    // A run whose only finding is suppressed stays clean and still notices it.
    let dir = project(&[(
        "a.verse",
        "# verse-lint: disable-next-line V1001 -- intentional\nA := 1  \n",
    )]);
    let (code, out) = stdout(
        dir.path(),
        &["a.verse", "--show-suppressed", "--output-format=github"],
    );
    assert_eq!(code, 0, "a notice must not fail a check run: {out}");
    assert!(out.starts_with("::notice "), "{out}");
}

#[test]
fn a_property_value_with_a_comma_colon_and_newline_stays_one_value() {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args([
            "-",
            "--stdin-filepath",
            "日, a:b\nc%.verse",
            "--output-format=github",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all("A := 1  \n".as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout,
        "::error file=日%2C a%3Ab%0Ac%25.verse,line=1,col=7,endLine=1,endColumn=9,title=V1001 TrailingWhitespace::Trailing whitespace\n",
        "a comma, colon or newline must not split the property list"
    );
    // One physical line: an unescaped LF would split the annotation in two.
    assert_eq!(stdout.lines().count(), 1);
}

#[test]
fn a_real_path_with_a_comma_is_escaped_and_still_identifies_the_file() {
    // NTFS cannot store `:` in a name, so the comma is the character a real
    // project path can carry.
    let dir = project(&[("a, b.verse", "A := 1  \n")]);
    let (code, out) = stdout(dir.path(), &[".", "--output-format=github"]);
    assert_eq!(code, 1);
    assert!(
        out.starts_with("::error file=a%2C b.verse,line=1,"),
        "{out}"
    );
    // The same path is reported unescaped by JSON, which needs no escaping.
    let (_, report) = json(dir.path(), &[]);
    assert_eq!(report["diagnostics"][0]["path"], "a, b.verse");
}

#[test]
fn execution_errors_stay_on_stderr_with_exit_two_and_leave_stdout_alone() {
    let dir = project(&[("bad.verse", "A := 1  \n"), ("ok.verse", "B := 2\n")]);
    let (code, out) = stdout(
        dir.path(),
        &[
            ".",
            "missing.verse",
            "--select",
            "V1001",
            "--output-format=github",
        ],
    );
    let text_run = run(dir.path(), &[".", "missing.verse", "--select", "V1001"]);
    let stderr = String::from_utf8(text_run.stderr).unwrap();
    assert_eq!(code, 2, "a partial failure is exit 2");
    assert_eq!(
        out,
        "::error file=bad.verse,line=1,col=7,endLine=1,endColumn=9,title=V1001 TrailingWhitespace::Trailing whitespace\n",
        "diagnostics from readable files survive; the failure itself is not an annotation"
    );
    assert!(!out.to_lowercase().contains("missing"), "{out}");
    // Execution errors keep the text-format stderr shape; the format only
    // changes stdout, so a failure is never a workflow command.
    assert!(
        stderr.starts_with("verse-lint: ") && stderr.contains("missing.verse"),
        "execution errors must keep their text-format stderr wording: {stderr}"
    );

    // Argument and configuration failures never fabricate an annotation either.
    for args in [
        vec!["--output-format=github", "-", "--fix"],
        vec!["--output-format=github", "--config", "absent.toml"],
        vec!["--output-format=github", "--show-config"],
    ] {
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(
            out.stdout.is_empty(),
            "{args:?} must not print a workflow command: {:?}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(!out.stderr.is_empty(), "{args:?}");
    }
}

#[test]
fn colors_never_apply_and_the_format_is_deterministic() {
    let dir = project(&[("a.verse", MIXED)]);
    let base = ["a.verse", "--select", "V1001,V2001"];
    let plain = stdout(
        dir.path(),
        &[&base[..], &["--output-format=github"]].concat(),
    );
    for color in ["auto", "always", "never"] {
        let colored = stdout(
            dir.path(),
            &[&base[..], &["--output-format=github", "--color", color]].concat(),
        );
        assert_eq!(colored, plain, "--color={color} must change nothing");
        assert!(
            !colored.1.contains('\u{1b}'),
            "--color={color} emitted ANSI"
        );
    }
    assert!(!plain.1.contains('\u{1b}'));
    let again = stdout(
        dir.path(),
        &[&base[..], &["--output-format=github"]].concat(),
    );
    assert_eq!(again.1, plain.1, "the same input must print the same bytes");
}

#[test]
fn warnings_alone_exit_zero_and_deny_warnings_raises_one() {
    let dir = project(&[(
        "a.verse",
        "# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )]);
    let base = ["a.verse", "--select", "V2001", "--output-format=github"];
    let (code, out) = stdout(dir.path(), &base);
    assert_eq!(code, 0);
    assert!(out.starts_with("::warning "), "{out}");
    let (code, out) = stdout(dir.path(), &[&base[..], &["--deny-warnings"]].concat());
    assert_eq!(code, 1, "deny-warnings changes the exit code only: {out}");
    assert!(out.starts_with("::warning "), "{out}");
}

#[test]
fn fix_runs_report_saved_source_regions_as_annotations() {
    let dir = project(&[("a.verse", "A := 1  \nB := 2 \n")]);
    let (code, out) = stdout(dir.path(), &["a.verse", "--fix", "--output-format=github"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out, "", "a clean fix leaves no annotation");
    assert_eq!(
        fs::read(dir.path().join("a.verse")).unwrap(),
        b"A := 1\nB := 2\n"
    );
}

#[test]
fn only_a_repository_root_configuration_root_aligns_annotations() {
    // Pins the docs/cli.md guidance: the working directory does not decide the
    // annotation path, the configuration root does.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join(".git")).unwrap();
    fs::create_dir_all(root.join("Content/Sub")).unwrap();
    fs::write(root.join("Content/verse.toml"), "schema-version = 1\n").unwrap();
    fs::write(root.join("Content/Sub/device.verse"), "A := 1  \n").unwrap();
    let content = root.join("Content");

    // Moving the job into the nested configuration root does not help: the path
    // is still relative to `Content`, which GitHub cannot resolve.
    let (code, out) = stdout(&content, &["Sub", "--output-format=github"]);
    assert_eq!(code, 1);
    assert!(out.starts_with("::error file=Sub/device.verse,"), "{out}");

    // From the checkout root without `--config`, the upward search starts at the
    // CWD and never finds the nested file, so the CWD is the root and aligns.
    let (code, out) = stdout(root, &["Content", "--output-format=github"]);
    assert_eq!(code, 1);
    assert!(
        out.starts_with("::error file=Content/Sub/device.verse,"),
        "{out}"
    );

    // A configuration at the checkout root aligns from any working directory.
    fs::remove_file(root.join("Content/verse.toml")).unwrap();
    fs::write(root.join("verse.toml"), "schema-version = 1\n").unwrap();
    let (code, out) = stdout(&content, &[".", "--output-format=github"]);
    assert_eq!(code, 1);
    assert!(
        out.starts_with("::error file=Content/Sub/device.verse,"),
        "{out}"
    );
}
