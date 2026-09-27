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

#[test]
fn default_project_is_sorted_deduplicated_and_obeys_exclusions() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::create_dir(root.join("Saved")).unwrap();
    for name in [
        "z.verse",
        "a.verse",
        ".hidden.verse",
        "auto.digest.verse",
        "skip.verse",
        "config.verse",
        "Saved/x.verse",
    ] {
        fs::write(root.join(name), "A := 1 \n").unwrap();
    }
    fs::write(root.join(".gitignore"), "skip.verse\n").unwrap();
    fs::write(
        root.join("verse.toml"),
        "[files]\nexclude = ['config.verse']\n[format]\nfuture-setting = 123\n",
    )
    .unwrap();
    let (out, value) = json(root, &[]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(value["summary"]["filesChecked"], 2);
    assert_eq!(value["diagnostics"][0]["path"], "a.verse");
    assert_eq!(value["diagnostics"][1]["path"], "z.verse");
    let (_, duplicates) = json(root, &["a.verse", "./a.verse", "A.VERSE"]);
    #[cfg(windows)]
    assert_eq!(duplicates["summary"]["filesChecked"], 1);
    let (explicit, value) = json(root, &["skip.verse", ".hidden.verse"]);
    assert_eq!(explicit.status.code(), Some(1));
    assert_eq!(value["summary"]["filesChecked"], 2);
    let (excluded, _) = json(root, &["config.verse"]);
    assert_eq!(excluded.status.code(), Some(2));
}

#[test]
fn project_nested_under_saved_parent_is_not_itself_protected() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Saved").join("MyIsland");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("device.verse"), "A := 1 \n").unwrap();

    let (out, report) = json(&root, &["."]);
    assert_eq!(out.status.code(), Some(1), "{report}");
    assert_eq!(report["summary"]["filesChecked"], 1);
    assert_eq!(report["diagnostics"][0]["path"], "device.verse");
}

#[test]
fn partial_failures_keep_readable_diagnostics_but_never_claim_complete() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("good.verse"), "A := 1 \n").unwrap();
    fs::write(root.join("bad.verse"), "<# unfinished").unwrap();
    let (out, value) = json(root, &[".", "absent.verse"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(value["summary"]["filesChecked"], 1);
    assert_eq!(value["summary"]["errors"], 1);
    assert_eq!(value["summary"]["complete"], false);
    assert_eq!(value["executionErrors"].as_array().unwrap().len(), 2);
    assert_eq!(value["diagnostics"][0]["path"], "good.verse");
    assert_eq!(fs::read(root.join("good.verse")).unwrap(), b"A := 1 \n");
}

#[test]
fn selection_unknown_rules_and_empty_effective_selection_are_checked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("a.verse"), "A := 1 \n").unwrap();
    fs::write(
        root.join("verse.toml"),
        "[lint]\nselect=['V1001']\nignore=['V1001']\n",
    )
    .unwrap();
    let (out, _) = json(root, &[]);
    assert_eq!(out.status.code(), Some(2));
    // Empty CLI list replaces the config list (it is not unioned).
    let (out, _) = json(root, &["--ignore="]);
    assert_eq!(out.status.code(), Some(2)); // empty string is not a rule ID
    fs::write(root.join("verse.toml"), "[lint]\nselect=[]\n").unwrap();
    let (out, _) = json(root, &["--select", "V1001,V1001"]);
    assert_eq!(out.status.code(), Some(1));
    for args in [
        vec!["--select", "V1"],
        vec!["--ignore", "V9999"],
        vec!["--select", "V1001", "--ignore", "V1001"],
    ] {
        assert_eq!(json(root, &args).0.status.code(), Some(2));
    }
}

#[test]
fn config_search_git_boundary_explicit_override_and_show_config() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let project = root.join("project");
    let child = project.join("child");
    fs::create_dir_all(&child).unwrap();
    fs::create_dir(project.join(".git")).unwrap();
    fs::write(root.join("verse.toml"), "unknown=true\n").unwrap();
    fs::write(child.join("a.verse"), "A := 1\n").unwrap();
    assert!(run(&child, &["--show-config"]).status.success());
    fs::write(project.join("verse.toml"), "[lint]\nmax-line-length=99\n").unwrap();
    let out = run(&child, &["--show-config"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("max-line-length = 99")
    );
    fs::write(child.join("override.toml"), "[lint]\nmax-line-length=88\n").unwrap();
    let out = run(&child, &["--config", "override.toml", "--show-config"]);
    assert!(out.status.success());
    assert!(
        String::from_utf8(out.stdout)
            .unwrap()
            .contains("max-line-length = 88")
    );
    assert!(run(&child, &["--help"]).status.success());
}

#[test]
fn config_schema_and_sections_fail_closed_except_sibling_table() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("a.verse"), "A := 1\n").unwrap();
    for config in [
        "schema-version=2",
        "unknown=1",
        "[files]\nunknown=1",
        "[lint]\nunknown=1",
        "[lint]\nmax-line-length=0",
        "format=42",
        "[lint]\nselect=['BAD']",
    ] {
        fs::write(root.join("verse.toml"), config).unwrap();
        let (out, value) = json(root, &[]);
        assert_eq!(out.status.code(), Some(2), "{config}");
        assert_eq!(value["summary"]["complete"], false);
    }
    fs::write(
        root.join("verse.toml"),
        "[format]\nline-ending='future-value'\nunknown=true\n",
    )
    .unwrap();
    assert_eq!(json(root, &[]).0.status.code(), Some(0));
}

#[test]
fn json_envelopes_cover_cli_errors_and_are_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("a.verse"), "A := 1 \n").unwrap();
    let (a, value) = json(root, &[]);
    let (b, _) = json(root, &[]);
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(value["schemaVersion"], 1);
    for args in [
        vec!["--bad-flag"],
        vec!["-", "--fix"],
        vec!["--stdin-filepath", "virtual.verse"],
        vec!["--config", "absent.toml"],
        vec!["--color", "invalid"],
    ] {
        let (out, value) = json(root, &args);
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(value["summary"]["complete"], false);
        assert!(!value["executionErrors"].as_array().unwrap().is_empty());
    }
    let out = run(root, &["--output-format=xml"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
}

#[test]
fn zero_sources_unexpanded_globs_and_diagnostic_limit_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    assert_eq!(json(root, &[]).0.status.code(), Some(2));
    assert_eq!(json(root, &["*.verse"]).0.status.code(), Some(2));
    fs::write(root.join("many.verse"), "# safe\n \n".repeat(10_001)).unwrap();
    let (out, value) = json(root, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(value["summary"]["complete"], false);
    assert!(
        String::from_utf8(out.stderr)
            .unwrap()
            .contains("diagnostic limit")
    );
    assert!(value["diagnostics"].as_array().unwrap().len() <= 10_000);
}
