//! V3002 unresolved-reference: intentionally conservative local name findings.
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

fn positions(report: &Value) -> Vec<(u64, u64, u64)> {
    let diagnostics = report["diagnostics"].as_array().unwrap();
    for diagnostic in diagnostics {
        assert_eq!(diagnostic["ruleId"], "V3002", "{report}");
        assert_eq!(diagnostic["severity"], "warning", "{report}");
        assert_eq!(diagnostic["fixable"], false, "{report}");
        assert_eq!(
            diagnostic["message"], "Reference is not declared in a modeled local scope",
            "{report}"
        );
    }
    diagnostics
        .iter()
        .map(|diagnostic| {
            (
                diagnostic["range"]["start"]["line"].as_u64().unwrap(),
                diagnostic["range"]["start"]["column"].as_u64().unwrap(),
                diagnostic["range"]["end"]["column"].as_u64().unwrap(),
            )
        })
        .collect()
}

#[test]
fn an_unresolved_name_is_reported_once_on_its_identifier_range() {
    let dir = project("F():void =\n    Print(MissingName)\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "warnings do not fail by default: {report}");
    assert_eq!(positions(&report), [(2, 11, 22)]);
    assert_eq!(report["summary"]["warnings"], 1);
    assert_eq!(report["summary"]["errors"], 0);

    fs::write(
        dir.path().join("main.verse"),
        "F():void =\n    Print(FirstMissing)\n    Print(SecondMissing)\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 11, 23), (3, 11, 24)]);
}

#[test]
fn enclosing_function_block_for_and_if_bindings_resolve() {
    let dir = project(concat!(
        "Outer:int = 1\n",
        "F(Param:int):void =\n",
        "    Local:int = 1\n",
        "    Print(Local)\n",
        "    if (Condition := true):\n",
        "        Print(Condition)\n",
        "    Values := array{1}\n",
        "    for (Index : Values):\n",
        "        Print(Index)\n",
        "    Print(Param)\n",
        "    Print(Outer)\n",
    ));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
}

#[test]
fn member_access_only_checks_the_object_side_and_never_the_member_side() {
    let dir = project(concat!(
        "F():void =\n",
        "    Print(UnknownObject.Member)\n",
        "device := class:\n",
        "    Read():void =\n",
        "        self.UnknownField\n",
    ));
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 11, 24)]);

    let declared = project("F():void =\n    Object := 1\n    Print(Object.Member)\n");
    let (code, report) = json(declared.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
}

#[test]
fn extension_receiver_names_are_excluded_but_the_function_body_is_checked() {
    let dir = project("(Receiver:UnknownType).Extend():void =\n    Print(MissingBodyName)\n");
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 11, 26)]);
}

#[test]
fn external_names_and_a_small_documented_builtin_set_are_silent() {
    let source = concat!(
        "F():void =\n",
        "    Print(ProjectApi)\n",
        "    Sleep(0.1)\n",
        "    GetSimulationEntity()\n",
        "    Print(StillMissing)\n",
    );
    let dir = project(source);
    let (code, report) = json(dir.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 11, 21), (5, 11, 23)]);

    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3002']\nexternal-names = ['ProjectApi']\n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(5, 11, 23)]);
    let shown = run(dir.path(), &["--show-config"]);
    let shown = String::from_utf8(shown.stdout).unwrap();
    assert!(
        shown.contains("external-names = [\"ProjectApi\"]"),
        "{shown}"
    );
}

#[test]
fn self_reference_use_before_declaration_and_shadowing_follow_graph_resolution() {
    let self_reference = project("F():void =\n    SelfRef := SelfRef\n");
    let (code, report) = json(self_reference.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");

    let before = project("F():void =\n    Print(Later)\n    Later := 1\n");
    let (code, report) = json(before.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(2, 11, 16)]);

    let shadowed = project(concat!(
        "F():void =\n",
        "    Value := 1\n",
        "    if (true):\n",
        "        Value := 2\n",
        "        Print(Value)\n",
        "    Print(Value)\n",
    ));
    let (code, report) = json(shadowed.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
}

#[test]
fn unsupported_module_paths_and_incomplete_graphs_produce_no_findings() {
    let module_path = project(concat!(
        "using { /Verse.org/Simulation }\n",
        "F():void =\n",
        "    Print(UnknownName)\n",
    ));
    let (code, report) = json(module_path.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");

    let unsupported = project(concat!(
        "F():void =\n",
        "    Print(UnknownName)\n",
        "    P := point{X := 1}\n",
    ));
    let (code, report) = json(unsupported.path(), &["main.verse", "--select", "V3002"]);
    assert_eq!(code, 0, "{report}");
    assert!(positions(&report).is_empty(), "{report}");
    let dump = run(unsupported.path(), &["main.verse", "--dump-scopes"]);
    let dump: Value = serde_json::from_slice(&dump.stdout).unwrap();
    assert_eq!(dump["files"][0]["isComplete"], false, "{dump}");
}

#[test]
fn selection_default_severity_suppression_and_sarif_metadata_use_shared_plumbing() {
    let source = "F():void =\n    Print(UnknownName)\n";
    let dir = project(source);
    let (code, report) = json(dir.path(), &["main.verse"]);
    assert_eq!(code, 0, "V3002 is not selected by default: {report}");
    assert!(positions(&report).is_empty(), "{report}");

    for selection in ["V3002", "V3", "V"] {
        let (code, report) = json(dir.path(), &["main.verse", "--select", selection]);
        assert_eq!(code, 0, "{selection}: {report}");
        assert_eq!(positions(&report), [(2, 11, 22)], "{selection}");
    }
    let denied = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V3002",
            "--deny-warnings",
            "--output-format=json",
        ],
    );
    assert_eq!(denied.status.code(), Some(1), "{denied:?}");
    let denied: Value = serde_json::from_slice(&denied.stdout).unwrap();
    assert_eq!(positions(&denied), [(2, 11, 22)]);

    let ignored = run(
        dir.path(),
        &[
            "main.verse",
            "--select",
            "V",
            "--ignore",
            "V3002",
            "--output-format=json",
        ],
    );
    let ignored: Value = serde_json::from_slice(&ignored.stdout).unwrap();
    assert!(
        ignored["diagnostics"].as_array().unwrap().is_empty(),
        "{ignored}"
    );

    fs::write(
        dir.path().join("main.verse"),
        "F():void =\n    # verse-lint: disable-next-line V3002 -- from an API\n    Print(UnknownName)\n",
    )
    .unwrap();
    let (code, report) = json(
        dir.path(),
        &["main.verse", "--select", "V3002", "--show-suppressed"],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(positions(&report), [(3, 11, 22)], "{report}");
    assert_eq!(report["diagnostics"][0]["suppressed"], true, "{report}");

    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = ['V3002']\n[lint.severity]\nV3002 = 'error'\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("main.verse"),
        "F():void =\n    Print(UnknownName)\n",
    )
    .unwrap();
    let sarif = run(dir.path(), &["main.verse", "--output-format=sarif"]);
    let sarif: Value = serde_json::from_slice(&sarif.stdout).unwrap();
    let descriptor = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V3002")
        .unwrap();
    assert_eq!(descriptor["defaultConfiguration"]["enabled"], false);
    assert_eq!(descriptor["defaultConfiguration"]["level"], "warning");
    let result = &sarif["runs"][0]["results"][0];
    assert_eq!(result["ruleId"], "V3002");
    assert_eq!(result["level"], "error");
}
