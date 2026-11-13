//! `[lint.severity]` promotes or demotes an existing rule for one project
//! without inventing rules, and without coupling severity to selection.
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

fn json(root: &Path, args: &[&str]) -> (i32, Value) {
    let mut full = args.to_vec();
    full.push("--output-format=json");
    let out = run(root, &full);
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code().expect("exit code"), value)
}

fn project(config: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("verse.toml"), config).unwrap();
    for (name, body) in files {
        fs::write(dir.path().join(name), body).unwrap();
    }
    dir
}

/// One warning-only finding: no trailing whitespace, no missing final newline.
const LONG_LINE: &str = "# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n";
const TRAILING: &str = "A := 1  \n";

#[test]
fn promoting_a_warning_to_error_fails_the_run_without_deny_warnings() {
    let dir = project(
        "[lint]\nselect = ['V2001']\n[lint.severity]\nV2001 = 'error'\n",
        &[("long.verse", LONG_LINE)],
    );
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["diagnostics"][0]["ruleId"], "V2001");
    assert_eq!(report["diagnostics"][0]["severity"], "error");
    assert_eq!(report["summary"]["errors"], 1);
    assert_eq!(report["summary"]["warnings"], 0);

    // Without the override the same project is a clean warning-only run.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V2001']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["warnings"], 1);
    assert_eq!(report["summary"]["errors"], 0);
}

#[test]
fn demoting_an_error_to_warning_keeps_exit_zero_unless_deny_warnings() {
    let dir = project(
        "[lint.severity]\nV1001 = 'warning'\n",
        &[("a.verse", TRAILING)],
    );
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["diagnostics"][0]["severity"], "warning");
    assert_eq!(report["summary"]["warnings"], 1);
    assert_eq!(report["summary"]["errors"], 0);
    // --deny-warnings promotes the remaining warning without changing severity.
    let (code, report) = json(dir.path(), &["--deny-warnings"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(report["diagnostics"][0]["severity"], "warning");
    assert_eq!(report["summary"]["warnings"], 1);
}

#[test]
fn text_json_and_sarif_report_the_effective_severity_for_one_rule_only() {
    let dir = project(
        "[lint.severity]\nV1001 = 'warning'\n",
        &[("a.verse", TRAILING)],
    );
    let text = String::from_utf8(run(dir.path(), &["a.verse"]).stdout).unwrap();
    assert_eq!(
        text, "a.verse:1:7: warning V1001 Trailing whitespace\n",
        "text shape must stay path:line:column: severity rule-id message"
    );
    let (code, report) = json(dir.path(), &["a.verse"]);
    assert_eq!(code, 0);
    assert_eq!(report["diagnostics"][0]["severity"], "warning");

    let sarif: Value =
        serde_json::from_slice(&run(dir.path(), &["a.verse", "--output-format=sarif"]).stdout)
            .unwrap();
    let run_node = &sarif["runs"][0];
    let result = &run_node["results"][0];
    assert_eq!(result["ruleId"], "V1001");
    assert_eq!(
        result["level"], "warning",
        "results carry the project's effective level"
    );
    // Driver metadata documents the tool default, not this project's override.
    let driver = run_node["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V1001")
        .unwrap();
    assert_eq!(driver["defaultConfiguration"]["level"], "error");
    assert_eq!(
        run_node["properties"]["summary"]["warnings"], 1,
        "SARIF summary counts agree with JSON"
    );
}

#[test]
fn overriding_an_unselected_rule_does_not_activate_it() {
    let dir = project(
        "[lint]\nselect = ['V1001']\n[lint.severity]\nV2001 = 'error'\n",
        &[("long.verse", LONG_LINE), ("a.verse", "A := 1\n")],
    );
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 0, "{report}");
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(report["summary"]["filesChecked"], 2);
}

#[test]
fn invalid_severity_configuration_fails_closed_and_names_the_offending_key() {
    for (config, mentioned) in [
        ("[lint.severity]\nV9999 = 'error'\n", "V9999"),
        ("[lint.severity]\nV200 = 'error'\n", "V200"),
        ("[lint.severity]\nV2001 = 'Error'\n", "V2001"),
        ("[lint.severity]\nV2001 = ''\n", "V2001"),
        ("[lint.severity]\nV2001 = 'notice'\n", "V2001"),
        ("[lint.severity]\nV2001 = 3\n", "V2001"),
        ("[lint.severity]\nV2001 = true\n", "V2001"),
    ] {
        let dir = project(config, &[("a.verse", "A := 1\n")]);
        let (code, report) = json(dir.path(), &[]);
        assert_eq!(code, 2, "{config}: {report}");
        assert_eq!(report["summary"]["complete"], false, "{config}");
        assert!(
            report["diagnostics"].as_array().unwrap().is_empty(),
            "a configuration error must not print diagnostics: {config}"
        );
        let message = report["executionErrors"][0]["message"].as_str().unwrap();
        assert!(
            message.contains(mentioned),
            "message must name {mentioned}: {message}"
        );
        assert!(
            message.to_lowercase().contains("severity") || message.contains("rule ID"),
            "message must describe the severity configuration: {message}"
        );
    }
}

#[test]
fn show_config_renders_an_ordered_effective_severity_map() {
    let dir = project(
        "[lint]\nselect = ['V1002']\n[lint.severity]\nV2001 = 'error'\nV1003 = 'warning'\n",
        &[("a.verse", "A := 1\n")],
    );
    let out = run(dir.path(), &["--show-config"]);
    assert!(out.status.success(), "{out:?}");
    let shown = String::from_utf8(out.stdout.clone()).unwrap();
    let severity_block = shown
        .split("[lint.severity]")
        .nth(1)
        .expect("show-config must render the effective severity map");
    let rendered: Vec<_> = severity_block
        .lines()
        .filter(|line| line.contains(" = "))
        .map(|line| line.trim().to_owned())
        .collect();
    // Every registered rule appears, in stable rule-ID order, and the override
    // is already applied instead of being echoed as raw configuration.
    assert_eq!(
        rendered,
        [
            "V1001 = \"error\"",
            "V1002 = \"error\"",
            "V1003 = \"warning\"",
            "V1004 = \"warning\"",
            "V2001 = \"error\"",
            "V2002 = \"warning\"",
            "V2003 = \"warning\"",
            "V3001 = \"warning\"",
            "V3002 = \"warning\"",
            "V3003 = \"warning\"",
        ]
    );
    // Same configuration, same bytes: no timestamps or filesystem ordering.
    let again = run(dir.path(), &["--show-config"]);
    assert_eq!(out.stdout, again.stdout);

    // Reversing the TOML source order must not change the rendered order.
    let reversed = project(
        "[lint]\nselect = ['V1002']\n[lint.severity]\nV1003 = 'warning'\nV2001 = 'error'\n",
        &[("a.verse", "A := 1\n")],
    );
    let reordered = String::from_utf8(run(reversed.path(), &["--show-config"]).stdout).unwrap();
    assert_eq!(
        reordered
            .split("[lint.severity]")
            .nth(1)
            .expect("severity block"),
        severity_block
    );
}

#[test]
fn severity_override_does_not_change_suppression_or_fix_behavior() {
    let dir = project(
        "[lint.severity]\nV1001 = 'warning'\n",
        &[(
            "a.verse",
            "# verse-lint: disable-next-line V1001 -- intentional\nA := 1  \n",
        )],
    );
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["suppressed"], 1);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());

    // The promoted/demoted severity must not disable the safe fix either.
    let fixed = project(
        "[lint.severity]\nV1001 = 'warning'\n",
        &[("a.verse", TRAILING)],
    );
    let out = run(fixed.path(), &[".", "--fix", "--output-format=json"]);
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(report["summary"]["filesChanged"], 1);
    assert_eq!(fs::read(fixed.path().join("a.verse")).unwrap(), b"A := 1\n");
}
