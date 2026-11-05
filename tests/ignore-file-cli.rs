//! `verse-lint.ignore`: one bounded ignore list at the configuration root.
//!
//! Every test here drives the real binary, because the contract is about what
//! discovery actually skips and what the user is told, not about a helper's
//! return value. The rules the file must not change — gitignore handling, hidden
//! entries, generated-file protection, explicit-argument behaviour — are asserted
//! alongside the new behaviour so a regression in either shows up as a failure.
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

fn json(root: &Path, args: &[&str]) -> (Output, Value) {
    let mut full = args.to_vec();
    full.push("--output-format=json");
    let out = run(root, &full);
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out, value)
}

fn text(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

fn checked(report: &Value) -> Vec<String> {
    let mut paths: Vec<String> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|diagnostic| diagnostic["path"].as_str().unwrap().to_string())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

/// A project with one trailing-whitespace finding per file, plus an ignore file.
fn project(ignore: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, body) in files {
        let path = dir.path().join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, body).unwrap();
    }
    fs::write(dir.path().join("verse-lint.ignore"), ignore).unwrap();
    dir
}

#[test]
fn ignore_file_excludes_matching_files_from_directory_discovery() {
    let dir = project(
        "# generated output\nVendor/**\nContent/Generated/*.verse\n",
        &[
            ("Content/device.verse", "A := 1 \n"),
            ("Vendor/lib.verse", "B := 1 \n"),
            ("Content/Generated/gen.verse", "C := 1 \n"),
        ],
    );
    let (out, report) = json(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["Content/device.verse"]);
}

/// Neither source overrides the other: the union is applied, and a file excluded
/// only by the ignore file is skipped exactly like a config-excluded one.
#[test]
fn config_exclude_and_ignore_file_combine_as_a_union() {
    let dir = project(
        "ignored.verse\n",
        &[
            ("kept.verse", "A := 1 \n"),
            ("configured.verse", "B := 1 \n"),
            ("ignored.verse", "C := 1 \n"),
        ],
    );
    fs::write(
        dir.path().join("verse.toml"),
        "[files]\nexclude = ['configured.verse']\n",
    )
    .unwrap();
    let (out, report) = json(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["kept.verse"]);

    // The union still applies when the ignore file changes: a pattern from each
    // source excludes its own file, and neither source is shadowed by the other.
    fs::write(
        dir.path().join("verse-lint.ignore"),
        "ignored.verse\nkept.verse\n",
    )
    .unwrap();
    let (out, report) = json(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["filesChecked"], 0, "{report}");
    assert!(
        report["executionErrors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["message"]
                .as_str()
                .unwrap()
                .contains("no eligible .verse files found")),
        "both sources excluded every file: {report}"
    );
}

/// Comments, blank lines and surrounding whitespace are handled; a `#` that is
/// not leading is part of the pattern, so it never truncates an entry.
#[test]
fn comments_blanks_whitespace_and_inner_hashes_are_handled() {
    let dir = project(
        "\n   \n# a comment\n\t# an indented comment\n  Vendor/**  \n",
        &[
            ("Vendor/lib.verse", "A := 1 \n"),
            ("kept.verse", "B := 1 \n"),
        ],
    );
    let (out, report) = json(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["kept.verse"]);

    // A `#` inside a name is literal, so the pattern still matches that file and
    // nothing else.
    fs::write(dir.path().join("build#1.verse"), "C := 1 \n").unwrap();
    fs::remove_dir_all(dir.path().join("Vendor")).unwrap();
    fs::write(dir.path().join("verse-lint.ignore"), "build#1.verse\n").unwrap();
    let (out, report) = json(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["kept.verse"]);
}

/// Every rejected form exits 2 with the offending line number, and no source is
/// inspected, so stdout carries no lint finding.
#[test]
fn every_rejected_entry_exits_2_and_names_its_line() {
    let cases = [
        ("kept.verse\n!negated.verse\n", "negation"),
        ("kept.verse\n/absolute/**\n", "an absolute path"),
        ("kept.verse\nC:/drive/**\n", "a drive letter or colon"),
        ("kept.verse\nVendor\\**\n", "a backslash"),
    ];
    for (ignore, reason) in cases {
        let dir = project(ignore, &[("a.verse", "A := 1 \n")]);
        let (out, report) = json(dir.path(), &[]);
        assert_eq!(out.status.code(), Some(2), "{ignore:?}: {report}");
        assert_eq!(report["summary"]["complete"], false, "{ignore:?}");
        assert_eq!(report["summary"]["filesChecked"], 0, "{ignore:?}");
        assert!(report["diagnostics"].as_array().unwrap().is_empty());
        let messages: Vec<&str> = report["executionErrors"]
            .as_array()
            .unwrap()
            .iter()
            .map(|error| error["message"].as_str().unwrap())
            .collect();
        assert!(
            messages.iter().any(|message| message.contains(":2:")),
            "{ignore:?}: the line number must be reported: {messages:?}"
        );
        assert!(
            messages.iter().any(|message| message.contains(reason)),
            "{ignore:?}: the reason must be named: {messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("verse-lint.ignore")),
            "{ignore:?}: the path must be named: {messages:?}"
        );
    }

    // An over-long entry is rejected at its own line too.
    let over_long = format!("kept.verse\n{}\n", "a".repeat(4_097));
    let dir = project(&over_long, &[("a.verse", "A := 1 \n")]);
    let (out, report) = json(dir.path(), &[]);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert!(
        report["executionErrors"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["message"].as_str().unwrap().contains(":2:")),
        "{report}"
    );
}

/// Both documented caps are inclusive: 256 patterns and 256 KiB are applied, one
/// past either exits 2 without inspecting a source or starting a fix.
#[test]
fn pattern_and_size_caps_are_inclusive_and_boundaries_are_enforced() {
    let dir = project("", &[("a.verse", "A := 1 \n")]);
    let ignore = dir.path().join("verse-lint.ignore");
    let at_limit = (0..256)
        .map(|index| format!("{index}/**"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&ignore, format!("{at_limit}\n")).unwrap();
    let (out, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    fs::write(&ignore, format!("{at_limit}\n256/**\n")).unwrap();
    let before = fs::read(dir.path().join("a.verse")).unwrap();
    let (out, report) = json(dir.path(), &[".", "--fix"]);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), before);
    assert!(
        stderr(&out).contains("at most 256 ignore patterns"),
        "{report}"
    );

    // Exactly 256 KiB of comments is accepted; one byte more is not.
    let padded = |size: usize| {
        let mut bytes = b"# ignore file\n".to_vec();
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
    };
    fs::write(&ignore, padded(256 * 1024)).unwrap();
    assert_eq!(fs::metadata(&ignore).unwrap().len(), 256 * 1024);
    let (out, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["complete"], true);

    fs::write(&ignore, padded(256 * 1024 + 1)).unwrap();
    let (out, report) = json(dir.path(), &[".", "--fix"]);
    assert_eq!(out.status.code(), Some(2), "{report}");
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(fs::read(dir.path().join("a.verse")).unwrap(), before);
    assert!(stderr(&out).contains("exceeds 256 KiB"), "{report}");
}

/// `--no-ignore-file` restores the pre-existing behaviour in the same directory,
/// and a broken file cannot fail a run that declined to use it.
#[test]
fn no_ignore_file_restores_the_previous_behaviour() {
    let dir = project(
        "Vendor/**\n",
        &[
            ("Vendor/lib.verse", "A := 1 \n"),
            ("kept.verse", "B := 1 \n"),
        ],
    );
    let (_, report) = json(dir.path(), &[]);
    assert_eq!(checked(&report), ["kept.verse"]);

    let (out, report) = json(dir.path(), &["--no-ignore-file"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["Vendor/lib.verse", "kept.verse"]);

    // A file that would be rejected is never opened, so it cannot fail the run.
    fs::write(dir.path().join("verse-lint.ignore"), "!nope.verse\n").unwrap();
    assert_eq!(json(dir.path(), &[]).0.status.code(), Some(2));
    let (out, report) = json(dir.path(), &["--no-ignore-file"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["Vendor/lib.verse", "kept.verse"]);
}

/// An explicit file argument still bypasses the ignore file, while the
/// non-bypassable protections keep applying.
#[test]
fn explicit_arguments_bypass_the_ignore_file_but_not_generated_protections() {
    let dir = project(
        "explicit.verse\nSaved/**\n",
        &[
            ("explicit.verse", "A := 1 \n"),
            ("Saved/x.verse", "B := 1 \n"),
            ("gen.digest.verse", "C := 1 \n"),
        ],
    );
    let (out, report) = json(dir.path(), &["explicit.verse"]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(checked(&report), ["explicit.verse"]);

    // Generated-file protection is not bypassable, but it comes from discovery
    // protections rather than this file, so it stays an error for an explicit arg.
    for path in ["Saved/x.verse", "gen.digest.verse"] {
        let (out, report) = json(dir.path(), &[path]);
        assert_eq!(out.status.code(), Some(2), "{path}: {report}");
    }
}

/// `--verbose` names the source that excluded each path, so a surprising skip is
/// attributable: config, ignore file and gitignore stay distinguishable.
#[test]
fn verbose_names_the_excluding_source() {
    let dir = project(
        "ignored.verse\n",
        &[
            ("kept.verse", "A := 1 \n"),
            ("ignored.verse", "B := 1 \n"),
            ("configured.verse", "C := 1 \n"),
            ("gitignored.verse", "D := 1 \n"),
        ],
    );
    fs::write(dir.path().join(".gitignore"), "gitignored.verse\n").unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "[files]\nexclude = ['configured.verse']\n",
    )
    .unwrap();
    let out = run(dir.path(), &[".", "--verbose"]);
    assert_eq!(out.status.code(), Some(1));
    let explained = stderr(&out);
    assert!(
        explained.contains("ignored.verse: excluded (verse-lint.ignore exclude)"),
        "{explained}"
    );
    assert!(
        explained.contains("configured.verse: excluded (verse.toml exclude)"),
        "{explained}"
    );
    assert!(
        explained.contains("gitignored.verse: excluded (.gitignore)"),
        "{explained}"
    );
    assert!(
        explained.contains("1 eligible .verse file(s)"),
        "{explained}"
    );
    assert!(text(&out).contains("kept.verse:1:7:"), "{}", text(&out));
}

/// `--show-config` reports presence, path and the merged effective count
/// deterministically, and distinguishes "disabled" from "not found".
#[test]
fn show_config_reports_the_ignore_file_deterministically() {
    let dir = project("Vendor/**\nGenerated/**\n", &[("a.verse", "A := 1 \n")]);
    fs::write(
        dir.path().join("verse.toml"),
        "[files]\nexclude = ['configured.verse']\n",
    )
    .unwrap();

    let out = run(dir.path(), &["--show-config"]);
    assert!(out.status.success(), "{}", stderr(&out));
    let shown = text(&out);
    let line = shown
        .lines()
        .find(|line| line.starts_with("# ignore file: "))
        .unwrap_or_else(|| panic!("no ignore-file line: {shown}"));
    assert!(line.contains("verse-lint.ignore"), "{shown}");
    assert!(line.contains("2 pattern(s)"), "{shown}");
    assert!(line.contains("3 effective"), "{shown}");
    // Same configuration, same bytes: no timestamps or filesystem ordering.
    assert_eq!(out.stdout, run(dir.path(), &["--show-config"]).stdout);

    let out = run(dir.path(), &["--show-config", "--no-ignore-file"]);
    let shown = text(&out);
    assert!(shown.contains("disabled by --no-ignore-file"), "{shown}");
    assert!(shown.contains("1 effective"), "{shown}");

    fs::remove_file(dir.path().join("verse-lint.ignore")).unwrap();
    let shown = text(&run(dir.path(), &["--show-config"]));
    assert!(shown.contains("# ignore file: none at"), "{shown}");
    assert!(shown.contains("0 pattern(s)"), "{shown}");
}

/// Only the configuration root is read: a file in a subdirectory or above the
/// root has no effect, so one location is the whole contract.
#[test]
fn only_the_configuration_root_ignore_file_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    let nested = root.join("Content");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.join("verse.toml"), "schema-version = 1\n").unwrap();
    fs::write(dir.path().join("verse-lint.ignore"), "root-above.verse\n").unwrap();
    fs::write(nested.join("verse-lint.ignore"), "nested.verse\n").unwrap();
    fs::write(root.join("root-above.verse"), "A := 1 \n").unwrap();
    fs::write(nested.join("nested.verse"), "B := 1 \n").unwrap();
    fs::write(root.join("kept.verse"), "C := 1 \n").unwrap();

    let (out, report) = json(&root, &["."]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(
        checked(&report),
        ["Content/nested.verse", "kept.verse", "root-above.verse"],
        "only <config root>/verse-lint.ignore may exclude anything: {report}"
    );
}
