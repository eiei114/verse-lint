//! `--dump-scopes` CLI contract: versioned deterministic JSON, no rules, and
//! each documented conflict refusing with exit 2.
use serde_json::Value;
use std::{
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

fn run(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_verse-lint"));
    command
        .current_dir(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match stdin {
        Some(bytes) => {
            let mut child = command.stdin(Stdio::piped()).spawn().unwrap();
            child.stdin.take().unwrap().write_all(bytes).unwrap();
            child.wait_with_output().unwrap()
        }
        None => command.output().unwrap(),
    }
}

fn dump(source: &str) -> (i32, Value) {
    let dir = tempfile::tempdir().unwrap();
    let out = run(dir.path(), &["-", "--dump-scopes"], Some(source.as_bytes()));
    let code = out.status.code().unwrap();
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stdout)));
    (code, value)
}

#[test]
fn dump_emits_versioned_json_and_exits_zero_without_running_rules() {
    // The source carries a trailing-whitespace error and a missing final
    // newline: a lint run would exit 1. The dump must still exit 0 and must not
    // mention any rule ID or diagnostic shape.
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("noise.verse");
    std::fs::write(&path, b"F():void =  \n    Print(\"x\")").unwrap();
    let out = run(directory.path(), &["noise.verse", "--dump-scopes"], None);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(out.stderr.is_empty(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    for absent in ["diagnostics", "ruleId", "V1001", "summary"] {
        assert!(!text.contains(absent), "dump must not run rules: {absent}");
    }
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["schemaVersion"], 1);
    let file = &value["files"][0];
    assert_eq!(file["path"], "noise.verse");
    assert_eq!(file["isComplete"], true);
    assert!(file["scopes"].as_array().unwrap().len() > 1);
}

#[test]
fn dump_document_has_the_documented_shape() {
    let (code, value) = dump("F(X:int):void =\n    Value := X\n    Print(Value)\n");
    assert_eq!(code, 0, "{value}");
    assert_eq!(value["schemaVersion"], 1);
    let file = &value["files"][0];
    assert_eq!(file["path"], "<stdin>");
    assert_eq!(file["isComplete"], true);
    assert_eq!(file["skipped"], serde_json::json!({}));

    let scopes = file["scopes"].as_array().unwrap();
    assert_eq!(scopes[0]["kind"], "file");
    assert_eq!(scopes[0]["parent"], Value::Null);
    assert_eq!(scopes[1]["kind"], "function");
    assert_eq!(scopes[1]["parent"], 0);

    let bindings = file["bindings"].as_array().unwrap();
    let by_name = |name: &str| {
        bindings
            .iter()
            .find(|b| b["name"] == name)
            .unwrap_or_else(|| panic!("no binding {name}"))
    };
    assert_eq!(by_name("F")["kind"], "function");
    assert_eq!(by_name("X")["kind"], "parameter");
    assert_eq!(by_name("Value")["kind"], "constant");
    // Ranges are half-open byte offsets: `Value` starts at its declaration.
    assert_eq!(by_name("Value")["range"]["start"], 20);
    assert_eq!(by_name("Value")["range"]["end"], 25);

    let references = file["references"].as_array().unwrap();
    let value_reference = references
        .iter()
        .find(|r| r["name"] == "Value" && r["range"]["start"] == 41)
        .expect("reference to Value");
    // `resolved` is an index into `bindings`, not a copy of the binding.
    let resolved = value_reference["resolved"].as_u64().unwrap() as usize;
    assert_eq!(bindings[resolved]["name"], "Value");
    // An unresolvable external call target is explicitly null, never guessed.
    let print = references.iter().find(|r| r["name"] == "Print").unwrap();
    assert_eq!(print["resolved"], Value::Null);
}

#[test]
fn dump_is_deterministic_for_the_same_input() {
    let source = "using { Demo.Helpers }\nF(X:int):void =\n    Value := X\n    Print(Value)\n";
    let (_, first) = dump(source);
    let (_, second) = dump(source);
    assert_eq!(first, second);
}

#[test]
fn dump_reports_incomplete_graph_with_skipped_kind_counts() {
    let (code, value) = dump("F():void =\n    P := point{X := 1}\n    Print(P)\n");
    assert_eq!(code, 0, "{value}");
    let file = &value["files"][0];
    assert_eq!(file["isComplete"], false);
    assert_eq!(file["skipped"]["field_initializer"], 1);
}

#[test]
fn dump_refuses_each_documented_conflict_with_exit_two() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("a.verse"), b"A := 1\n").unwrap();
    for args in [
        vec!["a.verse", "--dump-scopes", "--fix"],
        vec!["a.verse", "--dump-scopes", "--show-config"],
    ] {
        let out = run(directory.path(), &args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?} wrote stdout");
        assert!(!out.stderr.is_empty(), "{args:?} was silent");
    }
    // `text` is the default setting the dump is documented to accept, so naming
    // it explicitly is not a conflict.
    let out = run(
        directory.path(),
        &["a.verse", "--dump-scopes", "--output-format=text"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    // A requested machine format is still reported through the machine
    // envelope, exactly as every other execution error already is; the conflict
    // must not be swallowed into a scope graph.
    for format in ["json", "sarif"] {
        let flag = format!("--output-format={format}");
        let args = vec!["a.verse", "--dump-scopes", flag.as_str()];
        let out = run(directory.path(), &args, None);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        let text = String::from_utf8(out.stdout).unwrap();
        assert!(!text.contains("schemaVersion\": 1,\n  \"files"));
        assert!(!text.contains("\"scopes\""), "{format} emitted a graph");
    }
    // The accepted default spelling still works, so the refusals above are
    // about the conflict rather than about the flag itself.
    let out = run(
        directory.path(),
        &["a.verse", "--dump-scopes", "--color=never"],
        None,
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
}

#[test]
fn dump_fails_closed_on_inputs_that_today_fail_coverage_validation() {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in [
        ("markup.verse", "A := <p>text</p>\n"),
        ("unicode.verse", "得点 := 1\n"),
        ("incomplete.verse", "A:int=\n"),
        ("inline-binary.verse", "Add(X:int,Y:int):int = X+Y\n"),
        ("digest.verse", "<#> digest\n    detail\nA := 1\n"),
    ] {
        std::fs::write(directory.path().join(name), source).unwrap();
        let out = run(directory.path(), &[name, "--dump-scopes"], None);
        assert_eq!(out.status.code(), Some(2), "{name}: {out:?}");
        // No partial graph is printed for a file the tool refuses to inspect.
        assert!(out.stdout.is_empty(), "{name} printed a graph");
        assert!(!out.stderr.is_empty(), "{name} was silent");
    }
}

#[test]
fn dump_respects_the_per_source_byte_limit() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("huge.verse");
    let mut bytes = vec![b'#'];
    bytes.extend(std::iter::repeat_n(b'x', 8 * 1024 * 1024));
    std::fs::write(&path, &bytes).unwrap();
    let out = run(directory.path(), &["huge.verse", "--dump-scopes"], None);
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("8 MiB"),
        "{out:?}"
    );
}

#[test]
fn dump_does_not_change_lint_exit_codes_or_output() {
    // The same file, with and without the flag, must not influence each other.
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("trailing.verse"), b"A := 1  \n").unwrap();
    let lint = run(directory.path(), &["trailing.verse"], None);
    assert_eq!(lint.status.code(), Some(1), "{lint:?}");
    assert_eq!(
        String::from_utf8(lint.stdout).unwrap(),
        "trailing.verse:1:7: error V1001 Trailing whitespace\n"
    );
    let dump = run(directory.path(), &["trailing.verse", "--dump-scopes"], None);
    assert_eq!(dump.status.code(), Some(0));
    // A later lint run is unaffected by the dump.
    let again = run(directory.path(), &["trailing.verse"], None);
    assert_eq!(again.status.code(), Some(1));
}

#[test]
fn dump_discovers_the_same_files_as_a_lint_run_including_the_ignore_file() {
    // `--dump-scopes` shares discovery with a lint run, so `verse-lint.ignore`
    // excludes a file from the dump and `--no-ignore-file` brings it back.
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(root.join("verse.toml"), "").unwrap();
    std::fs::write(root.join("verse-lint.ignore"), "Generated/**\n").unwrap();
    std::fs::create_dir(root.join("Generated")).unwrap();
    std::fs::write(root.join("kept.verse"), "F():void = {}\n").unwrap();
    std::fs::write(root.join("Generated/skipped.verse"), "G():void = {}\n").unwrap();
    let paths = |args: &[&str]| -> Vec<String> {
        let out = run(root, args, None);
        assert_eq!(out.status.code(), Some(0), "{out:?}");
        let value: Value = serde_json::from_slice(&out.stdout).unwrap();
        value["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|file| file["path"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(paths(&["--dump-scopes"]), ["kept.verse"]);
    assert_eq!(
        paths(&["--dump-scopes", "--no-ignore-file"]),
        ["Generated/skipped.verse", "kept.verse"]
    );
}
