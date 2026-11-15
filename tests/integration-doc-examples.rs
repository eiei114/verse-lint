//! `docs/integration.md` tells a reader how to wire the binary into an editor,
//! a commit hook and a CI job. That document is only useful if its recipes are
//! literally runnable, so this module executes the parts a test can drive:
//!
//! - the VS Code `tasks.json` block is parsed as JSON, its problem-matcher
//!   regular expression is compiled, and the *real* text output of the binary is
//!   matched against it. The captured file/line/column/severity/code/message
//!   must equal what the same run reports in JSON, so a documented pattern that
//!   drifts from the output contract fails here instead of silently showing
//!   nothing in an editor. One case uses a `C:/...` absolute path, which is the
//!   case a naive `(\S+):(\d+)` pattern gets wrong.
//! - the stdin command is executed, including the claim that `--fix` refuses
//!   stdin with exit 2.
//! - the documented exit codes are observed on a clean run, an error, a denied
//!   warning and an incomplete inspection.
//! - the documented PowerShell pre-commit hook is extracted and run against a
//!   temporary Git repository, because a hook script that only *looks* right
//!   blocks nothing.
//!
//! It never claims a language server or an extension exists; the document states
//! that there is none, and that statement is checked too.
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
};

const DOC: &str = include_str!("../docs/integration.md");
const README: &str = include_str!("../README.md");
const CLI_DOC: &str = include_str!("../docs/cli.md");

/// Every fenced block in `document` tagged with `language`, in order.
fn fenced_blocks(document: &str, language: &str) -> Vec<String> {
    let opening = format!("```{language}");
    let mut blocks = Vec::new();
    let mut lines = document.lines();
    while let Some(line) = lines.next() {
        if line.trim() != opening {
            continue;
        }
        let mut body = Vec::new();
        for line in lines.by_ref() {
            if line.trim() == "```" {
                break;
            }
            body.push(line);
        }
        blocks.push(body.join("\n"));
    }
    blocks
}

/// The one fenced block that contains `needle`; its absence is a failure the
/// document cannot recover from.
fn block_with(document: &str, language: &str, needle: &str) -> String {
    fenced_blocks(document, language)
        .into_iter()
        .find(|block| block.contains(needle))
        .unwrap_or_else(|| {
            panic!("docs/integration.md has no ```{language} block containing {needle:?}")
        })
}

/// The documented `.vscode/tasks.json`, parsed. This also proves the snippet is
/// valid JSON a reader can paste.
fn documented_task() -> serde_json::Value {
    let block = block_with(DOC, "json", "problemMatcher");
    serde_json::from_str(&block).unwrap_or_else(|e| {
        panic!("the tasks.json block in docs/integration.md is not valid JSON: {e}\n{block}")
    })
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_verse-lint")
}

fn run(root: &Path, args: &[String]) -> Output {
    Command::new(binary())
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

/// Run the binary with `input` on stdin, the way a hook or an editor pipe does.
fn run_stdin(root: &Path, args: &[&str], input: &str) -> Output {
    use std::io::Write;
    let mut child = Command::new(binary())
        .current_dir(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout_text(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

fn json_report(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)))
}

/// A project whose `verse.toml` selects one bundled policy rule, plus a fixture
/// that produces one error and one warning so both severity words appear.
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "schema-version = 1\n[lint]\nselect = [\"V1001\", \"V2002\"]\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("Content")).unwrap();
    fs::write(
        dir.path().join("Content/device.verse"),
        "# TODO revisit\r\ndevice := class(creative_device):\r\n    OnBegin<override>()<suspends>:void =   \r\n        Print(\"ready\")\r\n",
    )
    .unwrap();
    dir
}

/// The documented matcher, read out of the document rather than duplicated here.
///
/// The expression is compiled with the `regex` crate as a portable check that it
/// is a valid regular expression; VS Code itself uses a JavaScript `RegExp`. The
/// documented pattern deliberately stays inside the syntax both engines accept
/// (no lookaround, no backreferences), so a compile here is meaningful.
struct DocumentedPattern {
    regex: regex::Regex,
    file: usize,
    line: usize,
    column: usize,
    severity: usize,
    code: usize,
    message: usize,
    args: Vec<String>,
}

fn documented_pattern() -> DocumentedPattern {
    let task = documented_task();
    let matcher = &task["tasks"][0]["problemMatcher"];
    let pattern = &matcher["pattern"];
    let regexp = pattern["regexp"]
        .as_str()
        .expect("the problem-matcher pattern has a regexp string");
    let index = |key: &str| {
        pattern[key]
            .as_u64()
            .unwrap_or_else(|| panic!("the documented pattern has no `{key}` capture index"))
            as usize
    };
    let args: Vec<String> = task["tasks"][0]["args"]
        .as_array()
        .expect("the documented task has an args array")
        .iter()
        .map(|arg| {
            arg.as_str()
                .expect("each documented argument is a string")
                .to_owned()
        })
        .collect();
    DocumentedPattern {
        regex: regex::Regex::new(regexp)
            .unwrap_or_else(|e| panic!("{regexp:?} does not compile: {e}")),
        file: index("file"),
        line: index("line"),
        column: index("column"),
        severity: index("severity"),
        code: index("code"),
        message: index("message"),
        args,
    }
}

/// One text line matched against the documented pattern, compared with the JSON
/// diagnostic that describes the same finding.
fn assert_line_matches(pattern: &DocumentedPattern, line: &str, diagnostic: &serde_json::Value) {
    let captures = pattern.regex.captures(line).unwrap_or_else(|| {
        panic!("the documented problem-matcher pattern does not match {line:?}")
    });
    let group = |index: usize| {
        captures
            .get(index)
            .unwrap_or_else(|| panic!("the documented pattern has no group {index}"))
            .as_str()
    };
    let start = &diagnostic["range"]["start"];
    assert_eq!(
        group(pattern.file),
        diagnostic["path"].as_str().unwrap(),
        "{line:?}"
    );
    assert_eq!(
        group(pattern.line),
        start["line"].as_u64().unwrap().to_string(),
        "{line:?}"
    );
    assert_eq!(
        group(pattern.column),
        start["column"].as_u64().unwrap().to_string(),
        "{line:?}"
    );
    assert_eq!(
        group(pattern.severity),
        diagnostic["severity"].as_str().unwrap(),
        "{line:?}"
    );
    assert_eq!(
        group(pattern.code),
        diagnostic["ruleId"].as_str().unwrap(),
        "{line:?}"
    );
    assert_eq!(
        group(pattern.message),
        diagnostic["message"].as_str().unwrap(),
        "{line:?}"
    );
}

/// The documented task arguments, with the JSON format swapped in for the
/// comparison run. The text run uses the documented arguments verbatim.
fn with_json_format(args: &[String]) -> Vec<String> {
    let mut args = args.to_vec();
    let index = args
        .iter()
        .position(|arg| arg == "--output-format")
        .expect("the documented task names --output-format");
    args[index + 1] = "json".into();
    args
}

#[test]
fn documented_problem_matcher_matches_real_relative_paths() {
    let pattern = documented_pattern();
    let dir = project();
    let text = run(dir.path(), &pattern.args);
    let json = run(dir.path(), &with_json_format(&pattern.args));
    let report = json_report(&json);

    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        2,
        "the fixture must produce one error and one warning so both documented \
         severity words are exercised: {report}"
    );
    let text_stdout = stdout_text(&text);
    let lines: Vec<&str> = text_stdout.lines().collect();
    assert_eq!(
        lines.len(),
        diagnostics.len(),
        "text output must carry one documented line per diagnostic: {text_stdout:?}"
    );
    for (line, diagnostic) in lines.iter().zip(diagnostics) {
        assert_line_matches(&pattern, line, diagnostic);
    }
    let severities: Vec<&str> = diagnostics
        .iter()
        .map(|diagnostic| diagnostic["severity"].as_str().unwrap())
        .collect();
    for severity in ["error", "warning"] {
        assert!(
            severities.contains(&severity),
            "both documented severity words must occur, so the pattern's severity \
             group is not accidentally only ever one of them: {report}"
        );
    }
    // The documented exit code for reported violations.
    assert_eq!(text.status.code(), Some(1), "{text_stdout}");
}

/// A drive letter contains a colon, so `C:/Users/you/Content/device.verse:1:14:`
/// is the case a pattern without a path class either rejects or splits wrong.
/// A virtual stdin path is the portable way to obtain exactly that label.
#[test]
fn documented_problem_matcher_matches_a_windows_drive_path() {
    let pattern = documented_pattern();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("verse.toml"), "schema-version = 1\n").unwrap();
    const LABEL: &str = "C:/Users/you/Content/device.verse";
    let buffer = "value := 1   \r\n";

    let text = run_stdin(
        dir.path(),
        &[
            "-",
            "--stdin-filepath",
            LABEL,
            "--output-format",
            "text",
            "--color",
            "never",
        ],
        buffer,
    );
    let json = run_stdin(
        dir.path(),
        &["-", "--stdin-filepath", LABEL, "--output-format", "json"],
        buffer,
    );
    let report = json_report(&json);
    let diagnostics = report["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics.len(),
        1,
        "the buffer must produce exactly the trailing-whitespace finding: {report}"
    );
    assert_eq!(
        diagnostics[0]["path"], LABEL,
        "stdin reports its virtual path, which is this label: {report}"
    );

    let text = stdout_text(&text);
    let line = text.lines().next().expect("the run reports one line");
    assert!(
        line.starts_with("C:/Users/you/Content/device.verse:"),
        "the drive letter must stay at the start of the matched line: {line:?}"
    );
    assert_line_matches(&pattern, line, &diagnostics[0]);
}

/// The stdin recipe and its two documented consequences: the virtual path picks
/// up the project's configuration, and `--fix` refuses stdin with exit 2.
#[test]
fn documented_stdin_recipe_uses_the_virtual_path_and_never_fixes() {
    let dir = project();
    // Only V2001 is selected here, so a long line is reported and the buffer's
    // trailing whitespace is not: that proves the configuration was discovered
    // from the virtual path rather than from defaults.
    fs::write(
        dir.path().join("verse.toml"),
        "schema-version = 1\n[lint]\nselect = [\"V2001\"]\n",
    )
    .unwrap();
    const LABEL: &str = "Content/device.verse";
    let long = format!("value := {}\r\n       \r\n", "x".repeat(130));

    let out = run_stdin(
        dir.path(),
        &[
            "-",
            "--stdin-filepath",
            LABEL,
            "--output-format",
            "text",
            "--color",
            "never",
        ],
        &long,
    );
    let stdout = stdout_text(&out);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(
        stdout.starts_with("Content/device.verse:1:"),
        "the documented command must label the finding with the virtual path: {stdout:?}"
    );
    assert!(
        stdout.contains("warning V2001"),
        "the project's selection must apply to a stdin run: {stdout:?}"
    );
    assert!(
        !stdout.contains("V1001"),
        "V1001 is not selected here, so the buffer proves config discovery: {stdout:?}"
    );

    let fixed = run_stdin(dir.path(), &["-", "--fix"], "value := 1   \r\n");
    assert_eq!(
        fixed.status.code(),
        Some(2),
        "the document states `--fix` with stdin exits 2: {:?}",
        stdout_text(&fixed)
    );
    assert!(
        String::from_utf8_lossy(&fixed.stderr).contains("cannot be used with stdin"),
        "--fix must refuse stdin with the documented message: {:?}",
        String::from_utf8_lossy(&fixed.stderr)
    );
}

/// The exit-code table, observed rather than quoted: 0 clean, 1 violations (and
/// 1 for denied warnings), 2 incomplete.
#[test]
fn documented_exit_codes_are_the_ones_the_binary_returns() {
    for needle in ["| `0` |", "| `1` |", "| `2` |"] {
        assert!(
            DOC.contains(needle),
            "docs/integration.md must keep the exit-code table row {needle:?}"
        );
    }
    let dir = project();
    fs::write(dir.path().join("Content/clean.verse"), "value := 42\r\n").unwrap();

    // The clean file on its own: exit 0 and no finding line for it.
    let clean = run(dir.path(), &["Content/clean.verse".to_owned()]);
    assert!(
        !stdout_text(&clean).contains("clean.verse"),
        "a clean file prints no findings: {:?}",
        stdout_text(&clean)
    );
    assert_eq!(
        clean.status.code(),
        Some(0),
        "exit 0 means no selected errors"
    );

    let error = run(dir.path(), &["Content/device.verse".to_owned()]);
    assert_eq!(
        error.status.code(),
        Some(1),
        "exit 1 means reported violations"
    );

    // A warning-only run is 0, and `--deny-warnings` promotes it to 1 without
    // changing severity — exactly the documented option for a hook.
    fs::write(
        dir.path().join("verse.toml"),
        "schema-version = 1\n[lint]\nselect = [\"V2001\"]\n",
    )
    .unwrap();
    let long = format!("value := {}\r\n", "x".repeat(130));
    fs::write(dir.path().join("Content/long.verse"), &long).unwrap();
    let warning = run(dir.path(), &["Content/long.verse".to_owned()]);
    assert_eq!(
        warning.status.code(),
        Some(0),
        "a warning alone is not a failure"
    );
    let denied = run(
        dir.path(),
        &[
            "Content/long.verse".to_owned(),
            "--deny-warnings".to_owned(),
        ],
    );
    assert_eq!(
        denied.status.code(),
        Some(1),
        "--deny-warnings must turn the same warning into exit 1"
    );

    // 2 is "incomplete", not "more violations": the path was never inspected.
    let missing = run(dir.path(), &["Content/absent.verse".to_owned()]);
    assert_eq!(missing.status.code(), Some(2), "an I/O failure exits 2");
    let stderr = String::from_utf8_lossy(&missing.stderr);
    assert!(
        stderr.contains("verse-lint:"),
        "exit 2 must explain itself on stderr: {stderr:?}"
    );
}

/// The document has to say what is not there: no language server, no extension,
/// and no automatic `--fix` from a hook. It also has to be reachable from the
/// two places a reader starts at.
#[test]
fn the_document_states_the_limits_and_is_linked_from_readme_and_cli() {
    assert!(
        DOC.contains("there is no language server and no editor extension"),
        "the document must state that no LSP or extension exists"
    );
    assert!(
        DOC.contains("**Stdin never fixes.**"),
        "the document must state that stdin never fixes"
    );
    assert!(
        DOC.contains("A hook or a task never rewrites your files unless you add `--fix` yourself."),
        "the document must state that a hook never fixes on its own"
    );
    assert!(
        CLI_DOC.contains("](integration.md)"),
        "docs/cli.md must link the integration guide"
    );
    assert!(
        README.contains("docs/integration.md"),
        "README.md must link the integration guide with the absolute URL crates.io needs"
    );
    // The documented workflow snippet must not introduce a code-scanning upload.
    let workflow = block_with(DOC, "yaml", "verse-lint.exe");
    for prohibited in ["upload-sarif", "security-events", "codeql-action"] {
        assert!(
            !workflow.contains(prohibited),
            "the documented CI recipe must not upload SARIF: found {prohibited:?}\n{workflow}"
        );
    }
    // The JSON snippet a reader copies is real JSON and reads the documented key.
    let snippet = block_with(DOC, "sh", "jq -r");
    assert!(
        snippet.contains(".summary"),
        "the JSON recipe must read `summary`, as the output contract defines it"
    );
    // The POSIX hook has to lint the *staged* bytes through stdin and exit with
    // the recorded status; a hook that lints the worktree would report lines the
    // commit does not contain.
    let sh_hook = block_with(DOC, "sh", "git diff --cached");
    for needle in [
        "--stdin-filepath",
        "git show \":$file\"",
        "--diff-filter=ACMR",
        "exit \"$status\"",
    ] {
        assert!(
            sh_hook.contains(needle),
            "the documented sh hook must keep {needle:?}"
        );
    }
    assert!(
        !sh_hook.contains("--fix"),
        "the documented sh hook must never rewrite the source it lints"
    );
}

/// The pre-commit hook from the document, extracted and run against a temporary
/// repository. A hook that cannot block is worse than no hook, so this asserts
/// 0 for a clean staged file, 1 for a staged error, 1 for a denied warning and
/// 2 when the tool is missing.
#[cfg(windows)]
#[test]
fn documented_powershell_hook_blocks_the_staged_findings_it_must() {
    // The hook needs both `pwsh` and `git`; without them the documented setup is
    // not installable and there is nothing to run.
    for tool in ["pwsh", "git"] {
        if Command::new(tool).arg("--version").output().is_err() {
            eprintln!("skipped: {tool} is unavailable, so the documented hook cannot run");
            return;
        }
    }
    let script = block_with(DOC, "powershell", "VERSE_LINT_DENY_WARNINGS");
    for needle in [
        "$PSVersionTable.PSVersion.Major -lt 7",
        "$OutputEncoding = [System.Text.UTF8Encoding]::new($false)",
        "git cat-file -e",
    ] {
        assert!(
            script.contains(needle),
            "the documented PowerShell hook must keep {needle:?}: it is what makes \
             the hook read the staged bytes instead of a re-encoded line list"
        );
    }
    // The documented shim must run PowerShell 7, which the script's version
    // guard requires; `powershell` (5.1) pipes native output line by line and
    // would hide a missing final newline.
    let shim = block_with(DOC, "sh", "pre-commit.ps1");
    assert!(
        shim.contains("pwsh"),
        "the documented shim must invoke pwsh, not Windows PowerShell: {shim}"
    );
    let dir = tempfile::tempdir().unwrap();
    let hook = dir.path().join("pre-commit.ps1");
    fs::write(&hook, format!("{script}\n")).unwrap();
    let project = dir.path().join("project");
    fs::create_dir(&project).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .current_dir(&project)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    };
    git(&["init", "--quiet"]);
    fs::write(
        project.join("verse.toml"),
        "schema-version = 1\n[lint]\nselect = [\"V1001\", \"V2001\"]\n",
    )
    .unwrap();
    fs::write(project.join("clean.verse"), "value := 1\r\n").unwrap();
    git(&["add", "verse.toml", "clean.verse"]);

    let run_hook = |binary: &str, deny: bool| {
        let mut command = Command::new("pwsh");
        command
            .current_dir(&project)
            .args(["-NoProfile", "-File"])
            .arg(&hook)
            .env("VERSE_LINT", binary)
            .env_remove("VERSE_LINT_DENY_WARNINGS");
        if deny {
            command.env("VERSE_LINT_DENY_WARNINGS", "1");
        }
        command.output().unwrap()
    };

    let clean = run_hook(binary(), false);
    assert_eq!(
        clean.status.code(),
        Some(0),
        "a clean staged file must commit: {}{}",
        stdout_text(&clean),
        String::from_utf8_lossy(&clean.stderr)
    );

    fs::write(project.join("bad.verse"), "value := 1   \r\n").unwrap();
    git(&["add", "bad.verse"]);
    let blocked = run_hook(binary(), false);
    assert_eq!(
        blocked.status.code(),
        Some(1),
        "a staged V1001 finding must block the commit: {}{}",
        stdout_text(&blocked),
        String::from_utf8_lossy(&blocked.stderr)
    );
    assert!(
        stdout_text(&blocked).contains("commit blocked: verse-lint exited 1"),
        "the hook must name the documented exit code: {:?}",
        stdout_text(&blocked)
    );

    // A warning-only staged file passes by default and fails under the
    // documented option.
    fs::write(project.join("bad.verse"), "value := 1\r\n").unwrap();
    let long = format!("value := {}\r\n", "x".repeat(130));
    fs::write(project.join("long.verse"), &long).unwrap();
    git(&["add", "bad.verse", "long.verse"]);
    let warned = run_hook(binary(), false);
    assert_eq!(
        warned.status.code(),
        Some(0),
        "a warning alone must not block a commit: {}{}",
        stdout_text(&warned),
        String::from_utf8_lossy(&warned.stderr)
    );
    let denied = run_hook(binary(), true);
    assert_eq!(
        denied.status.code(),
        Some(1),
        "VERSE_LINT_DENY_WARNINGS must block the same warning: {}{}",
        stdout_text(&denied),
        String::from_utf8_lossy(&denied.stderr)
    );

    // A missing binary is an incomplete inspection (2), never a silent pass.
    let absent = run_hook("C:/nowhere/verse-lint.exe", false);
    assert_eq!(
        absent.status.code(),
        Some(2),
        "a missing tool must exit 2 rather than commit: {}{}",
        stdout_text(&absent),
        String::from_utf8_lossy(&absent.stderr)
    );
    // A staged entry whose blob no longer exists is not a clean file either: a
    // hook that pipes nothing must fail, not pass. `--cacheinfo` puts a dangling
    // object ID in the index without touching the worktree.
    git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        "100644,0000000000000000000000000000000000000001,ghost.verse",
    ]);
    let unreadable = run_hook(binary(), false);
    assert_eq!(
        unreadable.status.code(),
        Some(2),
        "a staged file that cannot be read must exit 2: {}{}",
        stdout_text(&unreadable),
        String::from_utf8_lossy(&unreadable.stderr)
    );
}
