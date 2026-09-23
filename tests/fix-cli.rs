use serde_json::Value;
use std::{fs, path::Path, process::Command};
fn run(root: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(root)
        .args(args)
        .arg("--output-format=json")
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or_else(|e| panic!("{e}: {out:?}"));
    (out.status.code().unwrap(), v)
}
#[test]
fn fixes_only_two_safe_rules_and_preserves_bom_crlf_protected_bytes_and_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("日本語 😀.verse");
    fs::write(&p, "\u{feff}A:=\"日😀\"  \r\n# comment  ").unwrap();
    let (code, v) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(v["summary"]["filesChanged"], 1);
    assert_eq!(
        fs::read(&p).unwrap(),
        "\u{feff}A:=\"日😀\"\r\n# comment  \r\n".as_bytes()
    );
    let mtime = fs::metadata(&p).unwrap().modified().unwrap();
    let (_, v) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(v["summary"]["filesChanged"], 0);
    assert_eq!(fs::metadata(&p).unwrap().modified().unwrap(), mtime);
}
#[test]
fn remaining_tab_and_policy_diagnostics_refer_to_saved_source() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("a.verse");
    fs::write(&p, "f():void =  \n\tPrint(\"x\")  ").unwrap();
    let (code, v) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 1, "{v}");
    assert_eq!(v["summary"]["filesChanged"], 1);
    assert_eq!(v["diagnostics"][0]["ruleId"], "V1003");
    assert_eq!(v["diagnostics"][0]["range"]["start"]["byteOffset"], 11);
    assert_eq!(fs::read(&p).unwrap(), b"f():void =\n\tPrint(\"x\")\n");
}
#[test]
fn all_file_analysis_prevents_any_fix_on_syntax_directive_or_limit_failure() {
    for bad in [
        "<# not closed".to_string(),
        "# verse-lint: disable-next-line V9999 -- nope\n".into(),
        "# safe\n \n".repeat(10001),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.verse");
        fs::write(&p, b"A := 1  ").unwrap();
        fs::write(dir.path().join("z.verse"), bad).unwrap();
        let (code, v) = run(dir.path(), &[".", "--fix"]);
        assert_eq!(code, 2, "{v}");
        assert_eq!(v["summary"]["filesChanged"], 0);
        assert_eq!(v["summary"]["complete"], false);
        assert_eq!(fs::read(p).unwrap(), b"A := 1  ");
    }
}

#[test]
fn aggregate_input_limit_prevents_inspection_success_and_every_fix() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("00-first.verse");
    fs::write(&first, b"A := 1  \n").unwrap();
    for index in 0..8 {
        let path = dir.path().join(format!("{:02}-large.verse", index + 1));
        fs::File::create(path)
            .unwrap()
            .set_len(8 * 1024 * 1024)
            .unwrap();
    }

    for args in [&["."][..], &[".", "--fix"][..]] {
        let (code, report) = run(dir.path(), args);
        assert_eq!(code, 2, "{report}");
        assert_eq!(report["summary"]["complete"], false);
        assert!(
            report["executionErrors"]
                .as_array()
                .unwrap()
                .iter()
                .any(|error| error["message"]
                    .as_str()
                    .unwrap()
                    .contains("64 MiB total limit"))
        );
        assert_eq!(fs::read(&first).unwrap(), b"A := 1  \n");
    }
}

#[test]
fn oversized_config_and_gitignore_prevent_every_fix() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a.verse");
    fs::write(&source, b"A := 1  \n").unwrap();
    let oversized = vec![b'#'; 256 * 1024 + 1];

    let config = dir.path().join("verse.toml");
    fs::write(&config, &oversized).unwrap();
    let (code, report) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 2, "{report}");
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
                .contains("configuration exceeds 256 KiB"))
    );
    assert_eq!(fs::read(&source).unwrap(), b"A := 1  \n");

    fs::remove_file(config).unwrap();
    fs::write(dir.path().join(".gitignore"), &oversized).unwrap();
    let (code, report) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 2, "{report}");
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
                .contains(".gitignore exceeds 256 KiB"))
    );
    assert_eq!(fs::read(&source).unwrap(), b"A := 1  \n");
}

#[test]
fn too_many_exclude_globs_prevent_every_fix() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("a.verse");
    fs::write(&source, b"A := 1  \n").unwrap();
    let globs = (0..257)
        .map(|index| format!("'pattern-{index}/**'"))
        .collect::<Vec<_>>()
        .join(", ");
    fs::write(
        dir.path().join("verse.toml"),
        format!("[files]\nexclude = [{globs}]\n"),
    )
    .unwrap();

    let (code, report) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 2, "{report}");
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
                .contains("at most 256 exclude globs"))
    );
    assert_eq!(fs::read(&source).unwrap(), b"A := 1  \n");
}

#[test]
fn eof_and_suppressed_fix_cases_converge() {
    for (input, expected) in [
        ("A:=1  ", "A:=1\n"),
        ("# comment  ", "# comment  \n"),
        ("<# block #>", "<# block #>\n"),
        ("\u{feff}", "\u{feff}"),
        ("", ""),
        ("   ", "\n"),
        (
            "# verse-lint: disable-next-line V1001 -- keep trailing spaces\nA:=1  ",
            "# verse-lint: disable-next-line V1001 -- keep trailing spaces\nA:=1  \n",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.verse");
        fs::write(&p, input).unwrap();
        let (code, v) = run(dir.path(), &[".", "--fix"]);
        assert_eq!(code, 0, "{input:?}: {v}");
        assert_eq!(fs::read(&p).unwrap(), expected.as_bytes());
        assert_eq!(
            run(dir.path(), &[".", "--fix"]).1["summary"]["filesChanged"],
            0
        );
    }
}

#[test]
fn fix_preserves_mixed_line_endings_and_uses_last_seen_style_at_eof() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mixed.verse");
    fs::write(&path, b"A := 1  \r\nB := 2\nC := 3").unwrap();
    let (code, report) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(fs::read(&path).unwrap(), b"A := 1\r\nB := 2\nC := 3\n");
    let (code, report) = run(dir.path(), &[".", "--fix"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["filesChanged"], 0);
}
#[cfg(windows)]
#[test]
fn readonly_hardlink_and_share_lock_preflight_preserve_all_originals() {
    use std::os::windows::fs::OpenOptionsExt;
    for mode in ["readonly", "hardlink", "lock"] {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.verse");
        let z = dir.path().join("z.verse");
        fs::write(&a, b"A:=1  ").unwrap();
        fs::write(&z, b"Z:=2  ").unwrap();
        let permissions = fs::metadata(&z).unwrap().permissions();
        let mut lock = None;
        match mode {
            "readonly" => {
                let mut p = permissions.clone();
                p.set_readonly(true);
                fs::set_permissions(&z, p).unwrap();
            }
            "hardlink" => fs::hard_link(&z, dir.path().join("alias.verse")).unwrap(),
            "lock" => {
                lock = Some(
                    fs::OpenOptions::new()
                        .read(true)
                        .share_mode(1)
                        .open(&z)
                        .unwrap(),
                );
            }
            _ => unreachable!(),
        }
        let (code, v) = run(dir.path(), &["a.verse", "z.verse", "--fix"]);
        drop(lock);
        fs::set_permissions(&z, permissions).unwrap();
        assert_eq!(code, 2, "{mode}: {v}");
        assert_eq!(v["summary"]["filesChanged"], 0);
        assert_eq!(fs::read(&a).unwrap(), b"A:=1  ");
        assert_eq!(fs::read(&z).unwrap(), b"Z:=2  ");
    }
}

#[cfg(windows)]
#[test]
fn cli_fix_preserves_custom_dacl_creation_time_and_ntfs_stream() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("security.verse");
    fs::write(&path, b"A:=1  ").unwrap();
    let stream = format!("{}:verse-test", path.display());
    fs::write(&stream, b"private stream").unwrap();
    let acl = |set: bool| {
        let script = if set {
            "$ErrorActionPreference='Stop'; $a=Get-Acl -LiteralPath $env:VERSE_FILE; $a.SetAccessRuleProtection($true,$true); Set-Acl -LiteralPath $env:VERSE_FILE -AclObject $a; (Get-Acl -LiteralPath $env:VERSE_FILE).Sddl"
        } else {
            "$ErrorActionPreference='Stop'; (Get-Acl -LiteralPath $env:VERSE_FILE).Sddl"
        };
        let out = Command::new("pwsh")
            .args(["-NoProfile", "-Command", script])
            .env("VERSE_FILE", &path)
            .output()
            .unwrap();
        assert!(out.status.success(), "{out:?}");
        out.stdout
    };
    let before_acl = acl(true);
    let before_created = fs::metadata(&path).unwrap().created().unwrap();
    let (code, v) = run(dir.path(), &["security.verse", "--fix"]);
    assert_eq!(code, 0, "{v}");
    assert_eq!(acl(false), before_acl);
    assert_eq!(
        fs::metadata(&path).unwrap().created().unwrap(),
        before_created
    );
    assert_eq!(fs::read(stream).unwrap(), b"private stream");
    assert_eq!(fs::read(&path).unwrap(), b"A:=1\n");
}
