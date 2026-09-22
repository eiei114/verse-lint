//! Independently self-authored minimal shapes. Refusal is coverage debt, not a
//! claim of compiler-invalid Verse. No private source is redistributed.
use std::{
    io::Write,
    process::{Command, Stdio},
};

#[test]
fn practical_syntax_gaps_fail_closed_until_parser_support_is_verified() {
    for source in [
        "using { Demo.Helpers }\n",
        "shade := enum{\n    Light,\n    Dark\n}\n",
        "F():void =\n    Label:string=\"hello\"\n",
        "F():void =\n    Count := 1\n    Label:string=\"{Count}\"\n",
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
        assert_eq!(out.status.code(), Some(2), "{source}: {out:?}");
        assert!(out.stdout.is_empty());
        assert!(
            String::from_utf8(out.stderr)
                .unwrap()
                .contains("unsupported")
        );
    }
}
