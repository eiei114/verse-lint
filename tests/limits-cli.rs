//! Boundary and one-past-boundary coverage for every limit advertised in
//! `docs/cli.md` and `docs/parser-design.md`.
//!
//! Each limit is exercised at the documented boundary and one step past it. A
//! past-limit failure must be an execution error (exit 2, `complete: false`),
//! never a lint rule, and must never start a fix. Limits that need a heavy
//! input are either proven through a directly-invoked predicate (see the unit
//! tests in `src/`) or gated behind `VERSE_LINT_HEAVY_LIMIT_TESTS=1`, which is
//! documented in `docs/integration-testing.md`.
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

const MIB: usize = 1024 * 1024;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

fn json(root: &Path, args: &[&str]) -> (Output, Value) {
    let mut full = args.to_vec();
    full.push("--output-format=json");
    let out = run(root, &full);
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out, value)
}

fn lint_stdin(bytes: &[u8], args: &[&str]) -> (Output, Value) {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .arg("-")
        .args(["--output-format=json"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let out = child.wait_with_output().unwrap();
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out, value)
}

/// A past-limit run is an execution error, not a lint result: exit 2, an
/// incomplete report, no claimed diagnostics, and the expected message.
fn assert_execution_limit_failure(out: &Output, report: &Value, needle: &str) {
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["complete"], false, "{report}");
    assert_eq!(report["summary"]["filesChecked"], 0, "{report}");
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
    let messages = report["executionErrors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|error| error["message"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(
        messages.iter().any(|message| message.contains(needle)),
        "expected {needle:?} in {messages:?}"
    );
}

fn heavy_enabled() -> bool {
    std::env::var_os("VERSE_LINT_HEAVY_LIMIT_TESTS").is_some_and(|value| value == "1")
}

/// `docs/parser-design.md`: 8 MiB per source. Exactly 8 MiB is inspected, one
/// byte more is an execution error, and `--fix` writes nothing.
#[test]
fn source_byte_limit_boundary_is_enforced_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let at_limit = dir.path().join("at.verse");
    let mut bytes = vec![b'a'; 8 * MIB];
    bytes[0] = b'#';
    bytes[8 * MIB - 1] = b'\n';
    fs::write(&at_limit, &bytes).unwrap();

    let (out, report) = json(dir.path(), &["at.verse"]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["complete"], true);
    assert_eq!(report["summary"]["filesChecked"], 1);

    let past_limit = dir.path().join("past.verse");
    let mut oversized = vec![b'a'; 8 * MIB + 1];
    oversized[0] = b'#';
    fs::write(&past_limit, &oversized).unwrap();
    let before = fs::read(&past_limit).unwrap();

    let (out, report) = json(dir.path(), &["past.verse", "--fix"]);
    assert_execution_limit_failure(&out, &report, "8 MiB");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(
        fs::read(&past_limit).unwrap(),
        before,
        "--fix must not write"
    );
}

/// `docs/cli.md`: 64 MiB total successfully read input. The boundary predicate
/// is unit tested; this proves the whole 64 MiB aggregate end to end, so it is
/// gated behind `VERSE_LINT_HEAVY_LIMIT_TESTS=1`.
#[test]
fn total_read_limit_boundary_is_enforced_end_to_end() {
    if !heavy_enabled() {
        eprintln!("skipped: set VERSE_LINT_HEAVY_LIMIT_TESTS=1 (see docs/integration-testing.md)");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut file = vec![b'a'; 8 * MIB];
    file[0] = b'#';
    file[8 * MIB - 1] = b'\n';
    for index in 0..8 {
        fs::write(dir.path().join(format!("f{index}.verse")), &file).unwrap();
    }

    // Exactly 64 MiB total is a normal, complete inspection.
    let (out, report) = json(dir.path(), &["."]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["filesChecked"], 8);
    assert_eq!(report["summary"]["complete"], true);

    // A ninth file crosses the aggregate even though every file stays within
    // the per-source limit.
    let extra = dir.path().join("zz-extra.verse");
    fs::write(&extra, b"#\n").unwrap();
    let before = fs::read(&extra).unwrap();
    let (out, report) = json(dir.path(), &[".", "--fix"]);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["complete"], false);
    assert_eq!(
        report["summary"]["filesChanged"], 0,
        "no fix on limit failure"
    );
    assert!(
        report["executionErrors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["message"]
                .as_str()
                .unwrap()
                .contains("64 MiB total limit")),
        "{report}"
    );
    // The files inspected before the limit are still reported as checked.
    assert_eq!(report["summary"]["filesChecked"], 8, "{report}");
    assert_eq!(fs::read(&extra).unwrap(), before);
}

/// `docs/cli.md`: 128 directory levels. A file nested 127 directory levels below
/// the inspection root is still inspected; 128 nested levels crosses the bound.
/// The walk reports an execution error and keeps the files it already
/// discovered.
#[test]
fn directory_depth_limit_keeps_discovered_diagnostics() {
    let dir = tempfile::tempdir().unwrap();

    for (levels, expect_failure) in [(127usize, false), (128, true)] {
        let root = dir.path().join(format!("case-{levels}"));
        let mut nested = root.clone();
        for index in 1..=levels {
            nested = nested.join(format!("d{index}"));
        }
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("deep.verse"), "A := 1  \n").unwrap();
        fs::write(root.join("top.verse"), "B := 2  \n").unwrap();
        let before = fs::read(nested.join("deep.verse")).unwrap();

        let (out, report) = if expect_failure {
            json(&root, &[".", "--fix"])
        } else {
            json(&root, &["."])
        };
        if expect_failure {
            assert_eq!(out.status.code(), Some(2), "{report}");
            assert_eq!(report["summary"]["complete"], false);
            assert_eq!(
                report["summary"]["filesChanged"], 0,
                "no fix on limit failure"
            );
            assert!(
                report["executionErrors"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|error| error["message"]
                        .as_str()
                        .unwrap()
                        .contains("directory traversal resource limit"))
            );
            // Partial inspection: the already-discovered file keeps its finding.
            assert_eq!(report["summary"]["filesChecked"], 1, "{report}");
            assert_eq!(report["diagnostics"][0]["path"], "top.verse");
            assert_eq!(fs::read(nested.join("deep.verse")).unwrap(), before);
        } else {
            assert_eq!(out.status.code(), Some(1), "{report}");
            assert_eq!(report["summary"]["complete"], true);
            assert_eq!(report["summary"]["filesChecked"], 2);
        }
    }
}

/// `docs/cli.md`: at most 100 execution errors per walk. The cap stops the
/// walk, the discovered file still contributes its diagnostics, and the run is
/// an incomplete execution failure that starts no fix.
#[test]
fn walk_execution_error_cap_keeps_discovered_diagnostics_and_blocks_fixes() {
    let dir = tempfile::tempdir().unwrap();
    let good = dir.path().join("00-good.verse");
    fs::write(&good, b"A := 1  ").unwrap();
    let before = fs::read(&good).unwrap();

    let mut inputs = vec!["00-good.verse".to_string()];
    inputs.extend((0..100).map(|index| format!("absent-{index:03}.verse")));
    let mut args: Vec<&str> = inputs.iter().map(String::as_str).collect();
    args.push("--fix");

    let (out, report) = json(dir.path(), &args);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["complete"], false);
    assert_eq!(
        report["summary"]["filesChanged"], 0,
        "no fix on limit failure"
    );
    assert_eq!(report["executionErrors"].as_array().unwrap().len(), 100);
    // Documented partial-inspection contract: the file found before the cap is
    // still inspected and still reports.
    assert_eq!(report["summary"]["filesChecked"], 1);
    assert_eq!(report["diagnostics"][0]["path"], "00-good.verse");
    assert_eq!(report["diagnostics"][0]["ruleId"], "V1001");
    assert_eq!(report["diagnostics"][1]["ruleId"], "V1002");
    assert_eq!(fs::read(&good).unwrap(), before, "--fix must not write");
}

/// `docs/cli.md`: 256 KiB config and ignore file. Both bounds are inclusive;
/// one byte over is an execution error and starts no fix.
#[test]
fn config_and_ignore_size_limits_are_inclusive_at_256_kib() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a.verse");
    fs::write(&source, b"A := 1  \n").unwrap();
    let config = dir.path().join("verse.toml");

    fs::write(&config, exactly_sized_config(256 * 1024)).unwrap();
    assert_eq!(fs::metadata(&config).unwrap().len(), 256 * 1024);
    let (out, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["filesChecked"], 1);
    assert_eq!(report["summary"]["complete"], true);

    fs::write(&config, exactly_sized_config(256 * 1024 + 1)).unwrap();
    let before = fs::read(&source).unwrap();
    let (out, report) = json(dir.path(), &["a.verse", "--fix"]);
    assert_execution_limit_failure(&out, &report, "configuration exceeds 256 KiB");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(&source).unwrap(), before);
    fs::remove_file(&config).unwrap();

    let ignore = dir.path().join(".gitignore");
    fs::write(&ignore, exactly_sized_ignore(256 * 1024)).unwrap();
    assert_eq!(fs::metadata(&ignore).unwrap().len(), 256 * 1024);
    let (out, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    fs::write(&ignore, exactly_sized_ignore(256 * 1024 + 1)).unwrap();
    let (out, report) = json(dir.path(), &["a.verse", "--fix"]);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["complete"], false);
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(&source).unwrap(), before);
}

/// A valid `verse.toml` padded with comments to exactly `size` bytes.
fn exactly_sized_config(size: usize) -> Vec<u8> {
    let head = b"schema-version = 1\n";
    assert!(size > head.len());
    let mut bytes = head.to_vec();
    while bytes.len() < size {
        let room = size - bytes.len();
        if room < 3 {
            // Absorb the remainder into a trailing comment's text.
            bytes.push(b'#');
            bytes.extend(std::iter::repeat_n(b'a', size - bytes.len()));
            break;
        }
        let width = (room - 2).min(60);
        bytes.push(b'#');
        bytes.extend(std::iter::repeat_n(b'a', width));
        bytes.push(b'\n');
    }
    assert_eq!(bytes.len(), size);
    bytes
}

/// A valid `.gitignore` padded with comments to exactly `size` bytes.
fn exactly_sized_ignore(size: usize) -> Vec<u8> {
    let mut bytes = b"*.digest.verse\n".to_vec();
    while bytes.len() < size {
        let room = size - bytes.len();
        if room < 3 {
            bytes.push(b'#');
            bytes.extend(std::iter::repeat_n(b'a', size - bytes.len()));
            break;
        }
        let width = (room - 2).min(60);
        bytes.push(b'#');
        bytes.extend(std::iter::repeat_n(b'a', width));
        bytes.push(b'\n');
    }
    assert_eq!(bytes.len(), size);
    bytes
}

/// `docs/cli.md`: 256 exclude globs of up to 4,096 bytes each. Both bounds are
/// inclusive and one past either is an execution error.
#[test]
fn exclude_glob_boundaries_are_inclusive_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1  \n").unwrap();
    let config = dir.path().join("verse.toml");

    let globs = (0..256)
        .map(|index| format!("'{index}/**'"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(&config, format!("[files]\nexclude = [{globs}]\n")).unwrap();
    let (out, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    let globs = (0..257)
        .map(|index| format!("'{index}/**'"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(&config, format!("[files]\nexclude = [{globs}]\n")).unwrap();
    let (out, report) = json(dir.path(), &["a.verse", "--fix"]);
    assert_execution_limit_failure(&out, &report, "at most 256 exclude globs");
    assert_eq!(report["summary"]["filesChanged"], 0);

    let at_limit = "a".repeat(4096);
    fs::write(&config, format!("[files]\nexclude = ['{at_limit}']\n")).unwrap();
    let (out, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    let past_limit = "a".repeat(4097);
    fs::write(&config, format!("[files]\nexclude = ['{past_limit}']\n")).unwrap();
    let before = fs::read(dir.path().join("a.verse")).unwrap();
    let (out, report) = json(dir.path(), &["a.verse", "--fix"]);
    assert_execution_limit_failure(&out, &report, "without negation");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), before);
}

/// `docs/cli.md`: at most 10,000 diagnostics per invocation, including
/// suppressed findings. Exactly 10,000 is a normal lint result; the 10,001st
/// finding is an execution error that starts no fix.
#[test]
fn diagnostic_limit_boundary_includes_suppressed_findings() {
    let dir = tempfile::tempdir().unwrap();

    let at_limit = dir.path().join("at.verse");
    fs::write(&at_limit, " \n".repeat(10_000)).unwrap();
    let (out, report) = json(dir.path(), &["at.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["complete"], true);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 10_000);

    let past_limit = dir.path().join("past.verse");
    let mut body = " \n".repeat(10_000);
    body.push_str("A := 1  ");
    fs::write(&past_limit, &body).unwrap();
    let before = fs::read(&past_limit).unwrap();
    let (out, report) = json(dir.path(), &["past.verse", "--fix"]);
    assert_execution_limit_failure(&out, &report, "diagnostic limit exceeded");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(&past_limit).unwrap(), before);

    // Suppressed findings count toward the same bound even though they never
    // reach ordinary output.
    let suppressed = dir.path().join("suppressed.verse");
    let mut body = String::new();
    for _ in 0..10_000 {
        body.push_str("# verse-lint: disable-next-line V1001 -- test\n \n");
    }
    fs::write(&suppressed, &body).unwrap();
    let (out, report) = json(dir.path(), &["suppressed.verse"]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["complete"], true);
    assert_eq!(report["summary"]["suppressed"], 10_000);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
}

/// `docs/parser-design.md`: 250,000 tokens. Exactly 250,000 tokens scan; the
/// next token is a lexer execution error, not a rule and not a partial result.
#[test]
fn token_limit_boundary_is_enforced_end_to_end() {
    let dir = tempfile::tempdir().unwrap();

    // Two tokens per line: the line comment and the newline.
    let at_limit = dir.path().join("at.verse");
    fs::write(&at_limit, "# a\n".repeat(125_000)).unwrap();
    let (out, report) = json(dir.path(), &["at.verse"]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["complete"], true);
    assert_eq!(report["summary"]["filesChecked"], 1);

    let past_limit = dir.path().join("past.verse");
    fs::write(&past_limit, "# a\n".repeat(125_001)).unwrap();
    let before = fs::read(&past_limit).unwrap();
    let (out, report) = json(dir.path(), &["past.verse", "--fix"]);
    assert_execution_limit_failure(&out, &report, "250000 lexical tokens");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(&past_limit).unwrap(), before);
}

/// `docs/parser-design.md`: nesting 256. Exactly 256 levels parse; the 257th
/// is a lexer execution error.
#[test]
fn nesting_limit_boundary_is_enforced_end_to_end() {
    let at_limit = format!("{}1{}\n", "(".repeat(256), ")".repeat(256));
    let (out, report) = lint_stdin(at_limit.as_bytes(), &["--select", "V1001"]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    let past_limit = format!("{}1{}\n", "(".repeat(257), ")".repeat(257));
    let (out, report) = lint_stdin(past_limit.as_bytes(), &["--select", "V1001"]);
    assert_execution_limit_failure(&out, &report, "nesting exceeds 256 levels");
}

/// `docs/parser-design.md`: CST depth 512. A member-access chain reaches the
/// bound with a kilobyte of input, so both sides run end to end without a heavy
/// fixture.
#[test]
fn cst_depth_limit_boundary_is_enforced_end_to_end() {
    let at_limit = format!("A := X{}\n", ".Y".repeat(510));
    let (out, report) = lint_stdin(at_limit.as_bytes(), &["--select", "V1001"]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    let past_limit = format!("A := X{}\n", ".Y".repeat(511));
    let (out, report) = lint_stdin(past_limit.as_bytes(), &["--select", "V1001"]);
    assert_execution_limit_failure(&out, &report, "syntax tree resource limit");
}

/// `docs/parser-design.md`: one million fingerprint events. Reaching the bound
/// needs roughly 375 KB of interpolations, so the ordinary path proves the
/// shared guard predicate (`src/syntax.rs`) and this end-to-end run is gated.
#[test]
fn fingerprint_event_limit_boundary_is_enforced_end_to_end() {
    if !heavy_enabled() {
        eprintln!("skipped: set VERSE_LINT_HEAVY_LIMIT_TESTS=1 (see docs/integration-testing.md)");
        return;
    }
    let at_limit = format!("A := \"{}\"\n", "{X}".repeat(124_998));
    let (out, report) = lint_stdin(at_limit.as_bytes(), &["--select", "V1001"]);
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    let past_limit = format!("A := \"{}\"\n", "{X}".repeat(124_999));
    let (out, report) = lint_stdin(past_limit.as_bytes(), &["--select", "V1001"]);
    assert_execution_limit_failure(&out, &report, "syntax tree resource limit");
}

/// `docs/cli.md`: `max-line-length` must be 1..=1,000,000. Both ends of the
/// documented range are accepted and one past either end is a config error.
#[test]
fn max_line_length_boundaries_are_inclusive() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), b"A := 1\n").unwrap();
    let config = dir.path().join("verse.toml");

    for (value, expected_warnings) in [(1usize, 1usize), (1_000_000, 0)] {
        fs::write(
            &config,
            format!("[lint]\nselect = ['V2001']\nmax-line-length = {value}\n"),
        )
        .unwrap();
        let (out, report) = json(dir.path(), &["a.verse"]);
        assert_eq!(out.status.code(), Some(0), "{value}: {report}");
        assert_eq!(report["summary"]["complete"], true);
        assert_eq!(report["summary"]["warnings"], expected_warnings, "{value}");
    }

    for value in ["0", "1000001"] {
        fs::write(
            &config,
            format!("[lint]\nselect = ['V2001']\nmax-line-length = {value}\n"),
        )
        .unwrap();
        let (out, report) = json(dir.path(), &["a.verse", "--fix"]);
        assert_eq!(out.status.code(), Some(2), "{value}: {report}");
        assert_eq!(report["summary"]["complete"], false);
        assert_eq!(report["summary"]["filesChanged"], 0);
        assert!(
            report["executionErrors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|error| error["message"]
                    .as_str()
                    .unwrap()
                    .contains("max-line-length must be between 1 and 1000000"))
        );
    }
}
