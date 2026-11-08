//! V3003 variable-shadowing: an inner lexical declaration that reuses an outer
//! name. Every test here is an acceptance bullet from the issue, executed through
//! the real CLI so the rule is proven end to end — selection, the `[lint]
//! shadow-allow` list, the incomplete-graph no-op, suppression, severity
//! override, `--ignore`, `--show-suppressed` and JSON/SARIF output. The
//! false-positive cases are the point of the design, so they assert that *no*
//! diagnostic is produced rather than that one is merely absent from a list.
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
    let report = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    (out.status.code().unwrap(), report)
}

fn project(source: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("main.verse"), source).unwrap();
    dir
}

/// V3003 findings from a JSON report, as `(line, column, endColumn)`, after
/// asserting the documented per-finding contract. Other rules' findings are
/// filtered out, so a test that selects the whole registry still compares only
/// what this rule produced.
fn positions(report: &Value) -> Vec<(u64, u64, u64)> {
    let diagnostics = report["diagnostics"].as_array().unwrap();
    let mut found = Vec::new();
    for diagnostic in diagnostics.iter().filter(|d| d["ruleId"] == "V3003") {
        assert_eq!(diagnostic["severity"], "warning", "{report}");
        assert_eq!(diagnostic["fixable"], false, "{report}");
        assert_eq!(
            diagnostic["message"], "Declaration shadows an outer binding",
            "the message must be the fixed, name-free string: {report}"
        );
        found.push((
            diagnostic["range"]["start"]["line"].as_u64().unwrap(),
            diagnostic["range"]["start"]["column"].as_u64().unwrap(),
            diagnostic["range"]["end"]["column"].as_u64().unwrap(),
        ));
    }
    found
}

#[test]
fn an_inner_block_local_shadowing_an_outer_local_is_one_warning_on_the_inner_name() {
    let dir = project(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "        Print(Value)\n",
        "    Print(Value)\n",
    ));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "warnings do not fail by default: {report}");
    // Exactly one finding, on the inner declaration's five-character name.
    assert_eq!(positions(&report), [(4, 9, 14)]);
    assert_eq!(report["summary"]["warnings"], 1);
    assert_eq!(report["summary"]["errors"], 0);
    assert_eq!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|diagnostic| diagnostic["ruleId"] == "V3003")
            .count(),
        1,
        "{report}"
    );
}

#[test]
fn a_local_shadowing_a_parameter_is_reported_and_the_idiomatic_construct_binding_is_not() {
    let dir = project("F(Param:int):void =\n    Param := 1\n    Print(Param)\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 5, 10)]);

    // `for (Param : ...)` / `if (Param := ...)` rebinding the same function's
    // own parameter is the idiomatic Verse shape and must stay silent.
    for source in [
        concat!(
            "F(Param:int):void =\n",
            "    Values := array{1}\n",
            "    for (Param : Values):\n",
            "        Print(Param)\n",
        ),
        concat!(
            "F(Param:int):void =\n",
            "    if (Param := Get()):\n",
            "        Print(Param)\n",
        ),
    ] {
        fs::write(dir.path().join("main.verse"), source).unwrap();
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
        assert_eq!(code, 0, "{source}: {report}");
        assert!(positions(&report).is_empty(), "{source}: {report}");
    }

    // The exemption is about the immediate function's own parameter only, so a
    // construct binding that captures a *local* is still reported.
    fs::write(
        dir.path().join("main.verse"),
        concat!(
            "F():void =\n",
            "    Value := 1\n",
            "    for (Value : Values):\n",
            "        Print(Value)\n",
        ),
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(3, 10, 15)]);
}

#[test]
fn shadowing_a_type_function_or_module_import_name_is_never_reported() {
    // The model has no namespaces, so a local spelled like a type, a function or
    // a single-segment module import is not claimed as shadowing.
    for source in [
        "probe := class:\n    Field:int = 1\nF():void =\n    probe := 1\n    Print(probe)\n",
        "F():void =\n    Print(\"x\")\nG():void =\n    F := 1\n    Print(F)\n",
        "using { Helpers }\nF():void =\n    Helpers := 1\n    Print(Helpers)\n",
        "shade := enum{Light}\nF():void =\n    shade := 1\n    Print(shade)\n",
        "point := struct:\n    X:int = 0\nF():void =\n    point := 1\n    Print(point)\n",
        "face := interface:\n    F():void\nG():void =\n    face := 1\n    Print(face)\n",
    ] {
        let dir = project(source);
        let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
        assert_eq!(code, 0, "{source}: {report}");
        assert!(positions(&report).is_empty(), "{source}: {report}");
    }
}

#[test]
fn a_same_scope_redeclaration_is_not_a_v3003_finding() {
    let dir = project("F():void =\n    Value := 1\n    Value := 2\n    Print(Value)\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
    // Selecting the whole registry does not turn it into a second finding either.
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
}

#[test]
fn an_underscore_name_and_a_shadow_allow_entry_are_silent() {
    let dir = project(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        _Value := 2\n",
        "        Print(_Value)\n",
    ));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");

    // The switch is a list of exact, case-sensitive identifiers.
    fs::write(
        dir.path().join("main.verse"),
        "F():void =\n    Value := 1\n    if (true):\n        Value := 2\n        Print(Value)\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(4, 9, 14)]);

    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3003']\nshadow-allow = ['Value']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "an allowed name is never reported: {report}");
    assert!(positions(&report).is_empty(), "{report}");
    // A different case is a different name, so it does not silence the finding.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3003']\nshadow-allow = ['value']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(4, 9, 14)]);
    // `--show-config` prints the resolved list.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3003']\nshadow-allow = ['Value', 'Slot']\n",
    )
    .unwrap();
    let shown = run(dir.path(), &["--show-config"]);
    let shown = String::from_utf8(shown.stdout).unwrap();
    assert!(shown.contains("shadow-allow = ["), "{shown}");
    assert!(shown.contains("\"Value\""), "{shown}");
    assert!(shown.contains("\"Slot\""), "{shown}");
    // An empty entry is a configuration error, like `external-names`.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3003']\nshadow-allow = ['']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 2, "{report}");
    assert!(
        report["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("shadow-allow entries must not be empty"),
        "{report}"
    );
}

#[test]
fn an_incomplete_graph_reports_nothing_under_every_configuration() {
    // Object-construction field names are unsupported, so the file is not
    // reportable even though `Value` really is shadowed inside it.
    let source = concat!(
        "F(Param:int):void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "    Param := 3\n",
        "    P := point{X := 1}\n",
    );
    let dir = project(source);
    // Every route to enabling the rule stays silent, including the widest one.
    for args in [
        vec!["main.verse", "--select", "V3003", "--output-format=json"],
        vec!["main.verse", "--select", "V3", "--output-format=json"],
        vec!["main.verse", "--select", "V", "--output-format=json"],
        vec![
            "main.verse",
            "--select",
            "V",
            "--deny-warnings",
            "--output-format=json",
        ],
    ] {
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
        let report: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(positions(&report).is_empty(), "{args:?}: {report}");
        assert!(
            report["executionErrors"].as_array().unwrap().is_empty(),
            "{args:?}: silence is a no-op, not an execution error: {report}"
        );
        assert_eq!(report["summary"]["complete"], true, "{args:?}: {report}");
    }
    let dump = run(dir.path(), &["main.verse", "--dump-scopes"]);
    let dump: Value = serde_json::from_slice(&dump.stdout).unwrap();
    assert_eq!(dump["files"][0]["isComplete"], false, "{dump}");
    assert_eq!(
        dump["files"][0]["skipped"]["field_initializer"], 1,
        "{dump}"
    );

    // The same file without the unsupported construct reports both pairs, so the
    // silence above is incompleteness and nothing else.
    fs::write(
        dir.path().join("main.verse"),
        concat!(
            "F(Param:int):void =\n",
            "    Value := 1\n",
            "    if (true):\n",
            "        Value := 2\n",
            "    Param := 3\n",
        ),
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(4, 9, 14), (5, 5, 10)]);
}

#[test]
fn an_inner_binding_shadowing_two_ancestors_yields_one_finding_for_the_nearest() {
    let dir = project(concat!(
        "Value := 1\n",
        "F():void =\n",
        "    Value := 2\n",
        "    if (true):\n",
        "        Value := 3\n",
        "        Print(Value)\n",
    ));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    // One finding per inner declaration: the innermost name shadows the nearest
    // ancestor only, so a third declaration never produces a third diagnostic for
    // the same line.
    assert_eq!(positions(&report), [(3, 5, 10), (5, 9, 14)]);
    assert_eq!(report["summary"]["warnings"], 2, "{report}");
}

/// A parameter -> local -> inner block chain: the depth of the chain does not
/// change the claim, and each inner declaration is reported once.
#[test]
fn a_parameter_local_inner_block_chain_reports_each_inner_declaration_once() {
    let dir = project(concat!(
        "F(Value:int):void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "        Print(Value)\n",
        "    Print(Value)\n",
        "    Print(Value)\n",
    ));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 5, 10), (4, 9, 14)]);
    assert_eq!(report["summary"]["warnings"], 2, "{report}");
}

#[test]
fn the_rule_is_off_by_default_and_reachable_through_every_prefix() {
    let source = "F():void =\n    Value := 1\n    if (true):\n        Value := 2\n";
    let dir = project(source);
    // Off by default: a plain run reports nothing and exits 0.
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "V3003 is not selected by default: {report}");
    assert!(positions(&report).is_empty(), "{report}");
    // `V3003`, `V3` and `V` all reach it.
    for selection in ["V3003", "V3", "V"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert_eq!(positions(&report), [(4, 9, 14)], "{selection}");
    }
    // `--deny-warnings` promotes a warning to exit 1 without changing severity.
    let denied = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V3003",
            "--deny-warnings",
            "--output-format=json",
        ],
    );
    assert_eq!(denied.status.code(), Some(1), "{denied:?}");
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(positions(&denied), [(4, 9, 14)]);
    // `--ignore V3003` and `--ignore V3` both remove it from a full selection.
    for ignore in ["V3003", "V3"] {
        let (code, report) = json(
            dir.path(),
            &["main.verse", "--select", "V", "--ignore", ignore],
        );
        assert_eq!(code, 0, "{ignore}: {report}");
        assert!(positions(&report).is_empty(), "{ignore}: {report}");
    }
}

#[test]
fn suppression_and_sarif_metadata_use_the_shared_plumbing() {
    let dir = project(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        # verse-lint: disable-next-line V3003 -- deliberate convention\n",
        "        Value := 2\n",
    ));
    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V3003", "--show-suppressed"],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(5, 9, 14)], "{report}");
    assert_eq!(report["diagnostics"][0]["suppressed"], true, "{report}");
    // Without the flag the finding is gone and only the count remains.
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3003"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
    assert_eq!(report["summary"]["suppressed"], 1, "{report}");

    // A severity override changes the reported level; SARIF derives metadata
    // from the registry, so its default stays `warning` and `enabled` is false.
    fs::write(
        dir.path().join("main.verse"),
        "F():void =\n    Value := 1\n    if (true):\n        Value := 2\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3003']\n[lint.severity]\nV3003 = 'error'\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 1, "a promoted finding is a hard failure: {report}");
    assert_eq!(report["diagnostics"][0]["severity"], "error", "{report}");

    let sarif = run(dir.path(), &["main.verse", "--output-format=sarif"]);
    let sarif: Value = serde_json::from_slice(&sarif.stdout).unwrap();
    let descriptor = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V3003")
        .expect("V3003 is in the driver metadata");
    assert_eq!(descriptor["name"], "VariableShadowing", "{sarif}");
    assert_eq!(descriptor["defaultConfiguration"]["enabled"], false);
    assert_eq!(descriptor["defaultConfiguration"]["level"], "warning");
    assert_eq!(descriptor["properties"]["fixable"], false, "{sarif}");
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "V3003", "{sarif}");
    assert_eq!(result["level"], "error", "{sarif}");
    let index = result["ruleIndex"].as_u64().unwrap() as usize;
    assert_eq!(
        sarif["runs"][0]["tool"]["driver"]["rules"][index]["id"], "V3003",
        "ruleIndex must resolve to the reporting rule"
    );
}

/// The user-facing documents must state the rule, its exclusions and their
/// rationale, so the type-versus-value decision is not left to a reader's guess.
#[test]
fn the_documented_rule_references_the_type_versus_value_exclusion_and_its_rationale() {
    let rule = include_str!("../docs/rules/v3003.md");
    assert!(rule.contains("shadows an outer binding"), "{rule}");
    assert!(rule.contains("shadow-allow"), "{rule}");
    assert!(rule.contains("is_complete()"), "{rule}");
    assert!(rule.contains("same scope"), "{rule}");
    assert!(rule.contains("no namespaces"), "{rule}");
    let index = include_str!("../docs/rules.md");
    assert!(index.contains("](rules/v3003.md)"), "{index}");
    assert!(index.contains("type-versus-value shadowing"), "{index}");
    let cli = include_str!("../docs/cli.md");
    assert!(cli.contains("### `[lint] shadow-allow`"), "{cli}");
    assert!(
        cli.contains("V3001, V3002 and V3003 on an incomplete graph"),
        "{cli}"
    );
    let readme = include_str!("../README.md");
    assert!(readme.contains("[V3003][v3003]"), "{readme}");
    assert!(readme.contains("no namespaces yet"), "{readme}");
    assert!(
        readme.contains(
            "[v3003]: https://github.com/eiei114/verse-lint/blob/main/docs/rules/v3003.md"
        ),
        "{readme}"
    );
}
