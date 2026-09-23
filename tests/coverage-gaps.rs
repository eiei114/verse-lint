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
