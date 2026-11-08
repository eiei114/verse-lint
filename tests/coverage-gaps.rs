//! Independently self-authored practical shapes; no private source is redistributed.
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn practical_syntax_is_inspected_without_false_execution_failure() {
    for source in [
        "using { Demo.Helpers }\n",
        "using { Demo.Helpers.More }\n",
        "using { Helpers }\n",
        "shade := enum{\n    Light,\n    Dark\n}\n",
        "shade := enum{Light,Dark}\n",
        "F():void =\n    Label:string=\"hello\"\n",
        "F():void =\n    Count := 1\n    Label:string=\"{Count}\"\n",
        "Count:int=1\n",
        "Empty<public>:=class():\n    Value : int=1\n",
        "Check():void=\n    if:\n        Value:=1\n    then:\n        Print(\"ok\")\n",
        "Build():void=\n    Canvas:canvas=canvas:\n        Slots:=array:\n            canvas_slot:\n                ZOrder:={Z:=5}\n",
        "F():void=\n  if (set Values[Index] = Value):\n    Print(\"ok\")\n",
        "F():void=\n  for (Key -> Value : Values):\n    Print(Value)\n",
        "F():void =\n    for (I := 1..3):\n        Print(I)\n",
        // A string whose first content character is an unescaped `#` used to
        // lose its closing quote to the `line_comment` extra. Each shape below
        // is valid Verse and must be inspected, not refused.
        "A := \"# text\"\n",
        "A := \"#\"\n",
        "A := \"# text\" # trailing comment\n",
        "F():void =\n    L := \"a # b\"\n    Print(L)\n",
        "A := \"{Name}#tail\"\n",
        // Map literals in the documented positions.
        "A := map{\"x\" => 1}\n",
        "F():void =\n    M := map{\"x\" => 1, \"y\" => 2}\n    Print(M)\n",
        "F():void =\n    M := map{K => V}\n",
        "A := map{\"x\" => map{\"y\" => 1}}\n",
        "A := map{\"x\" => 1}[\"x\"]\n",
        // `map` stays usable as an ordinary name, which is why the rule head is
        // an identifier and not the reserved keyword `map`.
        "map := 1\n",
        "F():void =\n    map := 2\n    Print(map)\n",
        // Object construction is unchanged: `:=` initializers, not `=>` pairs.
        "F():void =\n    P := point{X := 1, Y := 2}\n    Print(P)\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .arg("-")
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
        assert_eq!(out.status.code(), Some(0), "{source}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(out.stderr.is_empty());
    }
}

#[test]
fn unsupported_range_headers_prevent_fixing_original_bytes() {
    for clause in [
        "I :=",
        "I := 1..",
        "I := ..3",
        ":= 1..3",
        "I := 1..3,",
        "I := Values",
        "I := 1..3, Twice := I * 2",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("range.verse");
        let source = format!("F():void =\n    for ({clause}):\n        Print(\"x\")  ");
        std::fs::write(&path, source.as_bytes()).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .args(["--fix", "--output-format=json"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{clause}: {out:?}");
        assert_eq!(std::fs::read(&path).unwrap(), source.as_bytes());
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["summary"]["complete"], false);
        assert_eq!(report["summary"]["filesChanged"], 0);
    }
}

#[test]
fn extreme_indentation_fails_closed_without_scanner_serialization_overflow() {
    let mut source = String::from("F():void =\n");
    for level in 1..=260 {
        source.push_str(&"    ".repeat(level));
        source.push_str("if (true):\n");
    }
    source.push_str(&"    ".repeat(261));
    source.push_str("Print(\"deep\")\n");

    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .arg("-")
        .arg("--output-format=json")
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
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert!(!String::from_utf8_lossy(&out.stderr).contains("panicked"));
}

/// The adjacent-statement guard's decision for inline binary function bodies is
/// part of the coverage contract, not an accident: a single same-line binary
/// body is refused with a message that names the construct, and the neighbours
/// that do have a complete CST are still inspected.
#[test]
fn inline_binary_function_bodies_stay_a_named_refusal() {
    for source in [
        "Add(X:int,Y:int):int = X+Y\n",
        "Sub(X:int,Y:int):int = X - Y\n",
        "F():int = 1 + 2\n",
        "F():int = X+Y;\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .arg("-")
            .arg("--output-format=json")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(|mut child| {
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(source.as_bytes())
                    .unwrap();
                child.wait_with_output().unwrap()
            })
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{source}: {out:?}");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["summary"]["complete"], false, "{source}");
        let message = report["executionErrors"][0]["message"].as_str().unwrap();
        assert!(
            message.contains("inline binary function bodies"),
            "{source}: {message}"
        );
    }

    // Complete CSTs keep working, so the refusal above is about the split — not
    // a blanket ban on binary expressions.
    for source in [
        "F():void =\n    X := 1 + 2\n",
        "F(X:int):int = X * Y\n",
        "F():bool = 1 < 2\n",
        "F():int = (X+Y)\n",
        "F():int = X\n(Y)\n",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .arg("-")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map(|mut child| {
                child
                    .stdin
                    .take()
                    .unwrap()
                    .write_all(source.as_bytes())
                    .unwrap();
                child.wait_with_output().unwrap()
            })
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{source}: {out:?}");
        assert!(out.stderr.is_empty(), "{source}: {out:?}");
    }
}

/// `<#>` is refused by name in the lexer, so a reader is told which construct the
/// strict lexer will not inspect instead of getting a generic recovery error.
#[test]
fn indented_hash_gt_comments_are_refused_by_name() {
    let source = "<#> digest\n    detail\nA := 1\n";
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .arg("-")
        .arg("--output-format=json")
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
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["summary"]["complete"], false);
    let message = report["executionErrors"][0]["message"].as_str().unwrap();
    assert!(message.contains("`<#>` indented comments"), "{message}");
}

/// Machine-readable output must cover a source that uses the constructs this
/// slice accepted, not only text output: JSON and SARIF have their own
/// serialization paths, and a file that fails as an execution error would
/// produce neither diagnostics nor a complete summary.
#[test]
fn newly_accepted_constructs_serialize_to_json_and_sarif() {
    let source = "A := \"# text\"\nB := map{\"x\" => 1}\n";
    for format in ["json", "sarif"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("accepted.verse");
        std::fs::write(&path, source).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .arg(&path)
            .arg(format!("--output-format={format}"))
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{format}: {out:?}");
        let document: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        match format {
            "json" => {
                assert_eq!(document["summary"]["complete"], true, "{document}");
                assert_eq!(document["summary"]["filesChecked"], 1, "{document}");
                assert_eq!(document["executionErrors"].as_array().unwrap().len(), 0);
                assert!(document["diagnostics"].as_array().unwrap().is_empty());
            }
            _ => {
                assert_eq!(document["version"], "2.1.0", "{document}");
                let run = &document["runs"][0];
                assert_eq!(run["invocations"][0]["executionSuccessful"], true);
                assert_eq!(run["invocations"][0]["exitCode"], 0);
                assert!(run["results"].as_array().unwrap().is_empty());
                // One driver rule entry per registered rule, derived from the
                // live registry: the document is real SARIF, not a stub.
                assert!(
                    !run["tool"]["driver"]["rules"]
                        .as_array()
                        .unwrap()
                        .is_empty()
                );
            }
        }
    }
}
