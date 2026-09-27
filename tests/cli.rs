use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .args(args)
        .output()
        .expect("launch verse-lint")
}

#[test]
fn help_succeeds_without_reading_input() {
    let out = cli(&["--help"]);
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    for flag in ["--fix", "--select", "--ignore", "--output-format"] {
        assert!(help.contains(flag), "missing {flag}");
    }
}

#[test]
fn version_is_the_package_version() {
    let out = cli(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        concat!("verse-lint ", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn invalid_flags_fail_without_diagnostics() {
    for args in [
        vec!["--unknown"],
        vec!["--output-format", "xml"],
        vec!["-", "--fix"],
    ] {
        let out = cli(&args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty());
        assert!(!out.stderr.is_empty());
    }
}

#[test]
fn nonexistent_file_is_not_success() {
    let out = cli(&["does-not-exist.verse"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}
