//! Original ambiguous comment example, not copied from a private project.
use std::{fs, process::Command};

const MISMATCH: &str = "F():void =\n    if:\n        A := First[] <# keep\n        comment #> B := Second[]\n    then:\n        Print(\"yes\")\n";

#[test]
fn mismatched_block_comment_fails_before_inspection_or_fix() {
    let source = MISMATCH;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("comment.verse");
    fs::write(&path, source).unwrap();
    for fix in [false, true] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_verse-lint"));
        command
            .current_dir(dir.path())
            .arg("--output-format=json")
            .arg(&path);
        if fix {
            command.arg("--fix");
        }
        let out = command.output().unwrap();
        assert_eq!(out.status.code(), Some(2), "fix={fix}: {out:?}");
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(report["summary"]["complete"], false);
        assert_eq!(report["summary"]["filesChanged"], 0);
        assert!(String::from_utf8_lossy(&out.stdout).contains("block comment boundary"));
        assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
    }
}

#[test]
fn matched_comments_and_opaque_literals_keep_bytes_with_bom_and_crlf() {
    for prefix in [
        "<# outer <# inner #> end #>\n",
        "<# one #><# two #>\n",
        "<# multi\nline #>\n",
        "Label := \"keep <# text #>\"\n",
        "Count := 1\nLabel := \"keep <# text #> {Count}\"\n",
        "# <# not a block #>\n",
    ] {
        for bom in ["", "\u{feff}"] {
            for eol in ["\n", "\r\n"] {
                let source = format!("{bom}{prefix}Value := 1  \n").replace('\n', eol);
                let expected = format!("{bom}{prefix}Value := 1\n").replace('\n', eol);
                let dir = tempfile::tempdir().unwrap();
                let path = dir.path().join("positive.verse");
                fs::write(&path, &source).unwrap();
                for pass in 0..2 {
                    let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
                        .current_dir(dir.path())
                        .args(["--fix", "--output-format=json"])
                        .arg(&path)
                        .output()
                        .unwrap();
                    assert_eq!(out.status.code(), Some(0), "{source:?}: {out:?}");
                    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
                    assert_eq!(report["summary"]["complete"], true);
                    assert_eq!(
                        report["summary"]["filesChanged"],
                        if pass == 0 { 1 } else { 0 }
                    );
                    assert_eq!(fs::read(&path).unwrap(), expected.as_bytes());
                }
            }
        }
    }
}

#[test]
fn earliest_mismatch_position_is_deterministic_with_bom_and_crlf() {
    let source =
        format!("\u{feff}{MISMATCH}{}", MISMATCH.replace("F()", "G()")).replace('\n', "\r\n");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("multiple.verse");
    fs::write(&path, &source).unwrap();
    let mut previous = None;
    for _ in 0..8 {
        let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
            .current_dir(dir.path())
            .arg("--output-format=json")
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        assert!(
            String::from_utf8_lossy(&out.stdout)
                .contains("3:22: unsupported block comment boundary"),
            "{out:?}"
        );
        if let Some(ref bytes) = previous {
            assert_eq!(&out.stdout, bytes);
        }
        previous = Some(out.stdout);
    }
    assert_eq!(fs::read(&path).unwrap(), source.as_bytes());
}

#[test]
fn later_mismatch_prevents_earlier_file_fix() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("a.verse");
    let later = dir.path().join("z.verse");
    fs::write(&first, "Value := 1  \n").unwrap();
    fs::write(&later, MISMATCH).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args(["--fix", "--output-format=json"])
        .arg(&first)
        .arg(&later)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["summary"]["filesChanged"], 0);
    assert_eq!(report["summary"]["complete"], false);
    assert_eq!(fs::read(&first).unwrap(), b"Value := 1  \n");
    assert_eq!(fs::read(&later).unwrap(), MISMATCH.as_bytes());
}
