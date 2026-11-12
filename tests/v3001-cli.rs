//! V3001 unused-declaration: the first rule that reads the scope graph.
//!
//! Every test here is an acceptance bullet from the issue, executed through the
//! real CLI so the rule is proven end to end — selection, the `[lint]
//! unused-parameters` switch, the incomplete-graph no-op, suppression, severity
//! override, `--ignore`, `--show-suppressed` and JSON/SARIF output. The
//! false-positive cases are the point of the design, so they assert that *no*
//! diagnostic is produced rather than that one is merely absent from a list.
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

/// Run the binary in `root` with `args`, returning the process output.
fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

/// Run the binary on stdin, which is the cheapest way to lint one source.
fn stdin(source: &str, args: &[&str]) -> Output {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir.path())
        .args(["-"])
        .args(args)
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
    child.wait_with_output().unwrap()
}

/// V3001-only findings from a JSON run, as `(line, column, endColumn)`.
fn findings(out: &Output) -> Vec<(u64, u64, u64)> {
    let report: Value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&out.stderr)));
    let Some(diagnostics) = report["diagnostics"].as_array() else {
        panic!("no diagnostics array: {report}");
    };
    for diagnostic in diagnostics {
        assert_eq!(diagnostic["ruleId"], "V3001", "{report}");
        assert_eq!(diagnostic["severity"], "warning", "{report}");
        assert_eq!(
            diagnostic["fixable"], false,
            "V3001 has no safe fix: {report}"
        );
        assert_eq!(
            diagnostic["message"], "Declaration is never referenced",
            "the message must be the fixed, name-free string: {report}"
        );
    }
    diagnostics
        .iter()
        .map(|d| {
            (
                d["range"]["start"]["line"].as_u64().unwrap(),
                d["range"]["start"]["column"].as_u64().unwrap(),
                d["range"]["end"]["column"].as_u64().unwrap(),
            )
        })
        .collect()
}

/// Lint `source` through stdin with V3001 selected and return its findings.
fn only(source: &str) -> Vec<(u64, u64, u64)> {
    let out = stdin(source, &["--select", "V3001", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    findings(&out)
}

#[test]
fn an_unused_local_var_declaration_is_reported_once_on_the_name_range() {
    // `Unused` is never referenced; `Used` is. Only the first is a finding, and
    // its range covers the name (columns 9..15, after the `var ` keyword) rather
    // than the whole declaration, which starts at column 5.
    let source = "F():void =\n    var Unused:int = 1\n    var Used:int = 2\n    Print(Used)\n";
    assert_eq!(only(source), [(2, 9, 15)]);
    // The same rule must not depend on the declaration form: the immutable
    // spelling is reported identically, at its own name columns.
    let constant = "F():void =\n    Unused:int = 1\n    Used:int = 2\n    Print(Used)\n";
    assert_eq!(only(constant), [(2, 5, 11)]);
}

#[test]
fn an_unused_constant_declaration_is_reported_and_a_used_one_is_not() {
    // Two constants in one body: only the unread one is a finding, so the rule
    // is not merely "reports constants".
    let source = "F():void =\n    Unused:int = 1\n    Used:int = 2\n    Print(Used)\n";
    assert_eq!(only(source), [(2, 5, 11)]);
    // A constant used from an inner block is used: the reference resolves
    // outward through the scope chain.
    let nested = "F():void =\n    Value:int = 1\n    if (true):\n        Print(Value)\n";
    assert_eq!(only(nested), Vec::new());
}

#[test]
fn exported_declarations_are_never_reported_even_with_zero_intra_file_use() {
    // A public class field, a public function and a top-level type name are all
    // reachable from outside this file, so intra-file silence proves nothing.
    let source = concat!(
        "probe<public> := class:\n",
        "    Field<public>:int = 1\n",
        "    Touch<public>():void =\n",
        "        Print(\"x\")\n",
        "helper := class:\n",
        "    Unused<public>:int = 2\n",
    );
    assert_eq!(only(source), Vec::new());
    // A top-level function with no caller is exported too: it is the module's
    // public surface, not a leftover local.
    assert_eq!(only("Orphan():void =\n    Print(\"x\")\n"), Vec::new());
    // A `protected` member is equally reachable from a subclass.
    let protected = "probe := class:\n    Field<protected>:int = 1\n";
    assert_eq!(only(protected), Vec::new());
    // The control: an unexported body local in the same shape *is* reported, so
    // the assertions above are not passing because the rule is switched off.
    assert_eq!(
        only(
            "probe := class:\n    Get():void =\n        Local:int = 1\n    Use():int =\n        Get()\n"
        ),
        [(3, 9, 14)]
    );
}

#[test]
fn underscore_prefixed_bindings_are_never_reported() {
    // The conventional "intentionally unused" marker, for a local, a
    // constant-shaped local and a parameter in both parameter modes.
    for source in [
        "F():void =\n    _Ignored:int = 1\n",
        "F():void =\n    var _Ignored:int = 1\n",
        "F(_Parameter:int):void =\n    Print(\"x\")\n",
    ] {
        assert_eq!(only(source), Vec::new(), "{source:?}");
    }
    // With parameters enabled the switch still honours the marker. This needs a
    // configuration file, so it cannot go through the stdin helper.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\nunused-parameters = true\n",
    )
    .unwrap();
    // The plain parameter in the same signature *is* reported, which proves the
    // switch is really on and the silence above is the marker's doing.
    fs::write(
        dir.path().join("a.verse"),
        "F(_Parameter:int, Plain:int):void =\n    Print(\"x\")\n",
    )
    .unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(findings(&out), [(1, 19, 24)]);
}

#[test]
fn a_using_import_with_no_traced_usage_is_not_reported() {
    // An import can be re-exported and the grammar does not let this model trace
    // every name a package path binds, so an untraceable import is treated as
    // used rather than unused.
    assert_eq!(only("using { Demo.Helpers }\n"), Vec::new());
    assert_eq!(
        only("using { /Fortnite.com/Devices }\nF():void =\n    Print(\"x\")\n"),
        Vec::new()
    );
    // A single-segment import that *is* traced and unread also stays silent: the
    // exclusion is the construct, not a failed lookup.
    assert_eq!(only("using { Helpers }\n"), Vec::new());
}

#[test]
fn parameters_are_silent_by_default_and_reported_only_when_asked() {
    let source = "F(Unused:int, _Ignored:int):void =\n    Print(\"x\")\n";
    // Default: no parameter finding, and the body has no local to report.
    assert_eq!(only(source), Vec::new());

    // With the switch on, the plain parameter is reported on its name range and
    // the `_`-prefixed one still is not.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\nunused-parameters = true\n",
    )
    .unwrap();
    fs::write(dir.path().join("a.verse"), source).unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(findings(&out), [(1, 3, 9)]);

    // `unused-parameters = false` is the explicit form of the default.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\nunused-parameters = false\n",
    )
    .unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(findings(&out), Vec::new());

    // A used parameter is never a finding in either mode, so the switch does not
    // simply report every parameter.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\nunused-parameters = true\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("b.verse"),
        "F(Value:int):void =\n    Print(Value)\n",
    )
    .unwrap();
    let out = run(dir.path(), &["b.verse", "--output-format=json"]);
    assert_eq!(findings(&out), Vec::new());

    // `--show-config` prints the resolved value so the mode is inspectable.
    let shown = String::from_utf8(run(dir.path(), &["--show-config"]).stdout).unwrap();
    assert!(shown.contains("unused-parameters = true"), "{shown}");
}

#[test]
fn an_incomplete_graph_reports_nothing_regardless_of_configuration() {
    // Object-construction field names are unsupported, so the whole file is
    // unreportable — even though `Leftover` really is unused inside it.
    let source = "F():void =\n    Leftover:int = 1\n    P := point{X := 1}\n";
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), source).unwrap();
    // Every route to enabling the rule must stay silent, including the widest
    // one and the parameter switch.
    for args in [
        vec!["a.verse", "--select", "V3001", "--output-format=json"],
        vec!["a.verse", "--select", "V3", "--output-format=json"],
        vec!["a.verse", "--select", "V", "--output-format=json"],
        vec![
            "a.verse",
            "--select",
            "V",
            "--deny-warnings",
            "--output-format=json",
        ],
    ] {
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
        assert_eq!(findings(&out), Vec::new(), "{args:?}");
    }
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\nunused-parameters = true\n",
    )
    .unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(findings(&out), Vec::new());

    // The silence is the *graph's* state, not a lint failure: no execution error
    // and `complete: true`, and `--dump-scopes` names the unsupported kind.
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        report["executionErrors"].as_array().unwrap().is_empty(),
        "{report}"
    );
    assert_eq!(report["summary"]["complete"], true, "{report}");
    let dump = run(dir.path(), &["a.verse", "--dump-scopes"]);
    assert_eq!(dump.status.code(), Some(0), "{dump:?}");
    let dump: Value = serde_json::from_slice(&dump.stdout).unwrap();
    assert_eq!(dump["files"][0]["isComplete"], false, "{dump}");
    assert_eq!(
        dump["files"][0]["skipped"]["field_initializer"], 1,
        "{dump}"
    );

    // The same file without the unsupported construct reports the leftover, so
    // the silence above is caused by incompleteness and nothing else.
    fs::write(
        dir.path().join("a.verse"),
        "F():void =\n    Leftover:int = 1\n",
    )
    .unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(findings(&out), [(2, 5, 13)]);
}

#[test]
fn the_rule_is_off_by_default_and_reachable_through_every_prefix() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        "F():void =\n    Unused:int = 1\n",
    )
    .unwrap();
    // Off by default: a plain run reports nothing and exits 0.
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(findings(&out), Vec::new());
    // `V3001`, `V3` and `V` all reach it.
    for selection in ["V3001", "V3", "V"] {
        let out = run(
            dir.path(),
            &["a.verse", "--select", selection, "--output-format=json"],
        );
        assert_eq!(out.status.code(), Some(0), "{selection}: {out:?}");
        assert_eq!(findings(&out), [(2, 5, 11)], "{selection}");
    }
}

#[test]
fn warning_exit_semantics_and_deny_warnings_are_the_shared_ones() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        "F():void =\n    Unused:int = 1\n",
    )
    .unwrap();
    // A warning alone exits 0, and `--deny-warnings` promotes it to 1 without
    // changing the reported severity.
    let out = run(
        dir.path(),
        &["a.verse", "--select", "V3001", "--output-format=json"],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let out = run(
        dir.path(),
        &[
            "a.verse",
            "--select",
            "V3001",
            "--deny-warnings",
            "--output-format=json",
        ],
    );
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["severity"], "warning", "{report}");
    assert_eq!(report["summary"]["warnings"], 1, "{report}");
    assert_eq!(report["summary"]["errors"], 0, "{report}");
    // Text output keeps the shared shape and the fixed message.
    let out = run(dir.path(), &["a.verse", "--select", "V3001"]);
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "a.verse:2:5: warning V3001 Declaration is never referenced\n"
    );
}

#[test]
fn severity_override_ignore_and_suppression_apply_without_special_cases() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        "F():void =\n    Unused:int = 1\n",
    )
    .unwrap();
    // `[lint.severity]` promotes the warning to an error, which fails the run.
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\n[lint.severity]\nV3001 = \"error\"\n",
    )
    .unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["severity"], "error", "{report}");
    // SARIF `results[].level` follows the override while the driver metadata
    // keeps the tool default, exactly like a style rule.
    let out = run(dir.path(), &["a.verse", "--output-format=sarif"]);
    let sarif: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(sarif["runs"][0]["results"][0]["level"], "error", "{sarif}");
    let driver = sarif["runs"][0]["tool"]["driver"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .find(|rule| rule["id"] == "V3001")
        .expect("V3001 is in the driver metadata");
    assert_eq!(
        driver["defaultConfiguration"]["level"], "warning",
        "{sarif}"
    );
    assert_eq!(driver["defaultConfiguration"]["enabled"], false, "{sarif}");

    // `--ignore V3001` removes it, and it is also reachable as the V3 family.
    fs::write(dir.path().join("verse.toml"), "").unwrap();
    for args in [
        vec![
            "a.verse",
            "--select",
            "V",
            "--ignore",
            "V3001",
            "--output-format=json",
        ],
        vec![
            "a.verse",
            "--select",
            "V",
            "--ignore",
            "V3",
            "--output-format=json",
        ],
    ] {
        let out = run(dir.path(), &args);
        assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
        assert_eq!(findings(&out), Vec::new(), "{args:?}");
    }
}

#[test]
fn a_line_suppression_on_the_binding_line_removes_the_finding() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        "F():void =\n    # verse-lint: disable-next-line V3001 -- kept for a callback\n    Unused:int = 1\n",
    )
    .unwrap();
    let out = run(
        dir.path(),
        &["a.verse", "--select", "V3001", "--output-format=json"],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        0,
        "{report}"
    );
    assert_eq!(report["summary"]["suppressed"], 1, "{report}");
    // `--show-suppressed` lists it again, marked and still without an exit code
    // change.
    let out = run(
        dir.path(),
        &[
            "a.verse",
            "--select",
            "V3001",
            "--show-suppressed",
            "--output-format=json",
        ],
    );
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        report["diagnostics"].as_array().unwrap().len(),
        1,
        "{report}"
    );
    assert_eq!(report["diagnostics"][0]["suppressed"], true, "{report}");
}

#[test]
fn a_shadowed_binding_reports_the_inner_unused_local_and_keeps_the_outer_used_one() {
    // Both declarations are named `Value`. The inner one is never referenced and
    // the outer one is read after the block, so exactly one finding is expected —
    // on the inner name, which is on line 4.
    let source = concat!(
        "F():void =\n",
        "    Value:int = 1\n",
        "    if (true):\n",
        "        Value:int = 2\n",
        "    Print(Value)\n",
    );
    assert_eq!(only(source), [(4, 9, 14)]);

    // The mirror image: the inner binding is read and the outer one is not, so
    // the outer declaration is the finding.
    let mirrored = concat!(
        "F():void =\n",
        "    Value:int = 1\n",
        "    if (true):\n",
        "        Value:int = 2\n",
        "        Print(Value)\n",
    );
    assert_eq!(only(mirrored), [(2, 5, 10)]);
}

#[test]
fn a_nested_block_scope_reports_a_local_that_only_its_own_block_could_use() {
    // The local sits in an inner `if` block and nothing there reads it, while the
    // enclosing body's local is used from that same inner block.
    let source = concat!(
        "F():void =\n",
        "    Outer:int = 1\n",
        "    if (true):\n",
        "        Inner:int = 2\n",
        "        Print(Outer)\n",
    );
    assert_eq!(only(source), [(4, 9, 14)]);
    // A local used only by a deeper block is used: resolution walks outward.
    let used = concat!(
        "F():void =\n",
        "    Outer:int = 1\n",
        "    if (true):\n",
        "        if (true):\n",
        "            Print(Outer)\n",
    );
    assert_eq!(only(used), Vec::new());
}

#[test]
fn json_ranges_point_at_the_name_and_are_half_open() {
    // The range covers the name only — start column 5, end column 11 for a
    // six-character identifier — so a consumer can highlight exactly the thing to
    // remove instead of the whole declaration.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        "F():void =\n    Unused:int = 1\n",
    )
    .unwrap();
    let out = run(
        dir.path(),
        &["a.verse", "--select", "V3001", "--output-format=json"],
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    let range = &report["diagnostics"][0]["range"];
    assert_eq!(range["start"]["byteOffset"], 15, "{report}");
    assert_eq!(range["end"]["byteOffset"], 21, "{report}");
    assert_eq!(range["start"]["column"], 5, "{report}");
    assert_eq!(range["end"]["column"], 11, "{report}");
    // The same numbers appear in SARIF, in the unicodeCodePoints convention.
    let out = run(
        dir.path(),
        &["a.verse", "--select", "V3001", "--output-format=sarif"],
    );
    let sarif: Value = serde_json::from_slice(&out.stdout).unwrap();
    let region = &sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
    assert_eq!(region["startLine"], 2, "{sarif}");
    assert_eq!(region["startColumn"], 5, "{sarif}");
    assert_eq!(region["endColumn"], 11, "{sarif}");
    assert_eq!(region["byteLength"], 6, "{sarif}");
    // SARIF never carries a suggested edit for an unfixable rule.
    assert!(sarif["runs"][0]["results"][0].get("fixes").is_none());
}

#[test]
fn one_file_reports_each_unused_binding_once_and_stays_deterministic() {
    let source = concat!(
        "F():void =\n",
        "    First:int = 1\n",
        "    Second:int = 2\n",
        "    Used:int = 3\n",
        "    Print(Used)\n",
        "G():void =\n",
        "    Third:int = 4\n",
    );
    let expected = [(2, 5, 10), (3, 5, 11), (7, 5, 10)];
    assert_eq!(only(source), expected);
    // Same input, same bytes: no hashed container decides the order.
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("a.verse"), source).unwrap();
    let first = run(
        dir.path(),
        &["a.verse", "--select", "V3001", "--output-format=json"],
    )
    .stdout;
    let second = run(
        dir.path(),
        &["a.verse", "--select", "V3001", "--output-format=json"],
    )
    .stdout;
    assert_eq!(first, second);
}

#[test]
fn an_assigned_but_unread_local_is_not_reported() {
    // Resolution counts every reference, including a write target. This rule
    // proves "no reference at all", never "no read", so a write keeps the
    // declaration silent: guessing intent from it is the false positive the
    // design is built to avoid.
    let source = "F():void =\n    var Counter:int = 0\n    set Counter = 1\n";
    assert_eq!(only(source), Vec::new());
}

#[test]
fn a_local_read_only_through_string_interpolation_is_used() {
    // `"{Value}"` is a read of `Value`. The string must not be treated as an
    // inert literal, or the only use disappears and a live local is reported.
    let source = "F():void =\n    Value:int = 1\n    Print(\"value: {Value}\")\n";
    assert_eq!(only(source), Vec::new());
    // Control: the same shape without the interpolation is still reported.
    let plain = "F():void =\n    Value:int = 1\n    Print(\"value\")\n";
    assert_eq!(only(plain), [(2, 5, 10)]);
}

#[test]
fn a_bodyless_signature_never_reports_its_parameters() {
    // Interface, abstract and native declarations have no body that could
    // reference a parameter, so even with the switch on they stay silent.
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("verse.toml"),
        "[lint]\nselect = [\"V3001\"]\nunused-parameters = true\n",
    )
    .unwrap();
    for source in [
        "i := interface:\n    F(Value:int):void\n",
        "c := class<abstract>:\n    F(Value:int):void\n",
        "F<native>(Value:int):void\n",
    ] {
        fs::write(dir.path().join("a.verse"), source).unwrap();
        let out = run(dir.path(), &["a.verse", "--output-format=json"]);
        assert_eq!(out.status.code(), Some(0), "{source:?}: {out:?}");
        assert_eq!(findings(&out), Vec::new(), "{source:?}");
    }
    // Control: a defined function's unused parameter is still reported.
    fs::write(
        dir.path().join("a.verse"),
        "F(Value:int):void =\n    Print(\"x\")\n",
    )
    .unwrap();
    let out = run(dir.path(), &["a.verse", "--output-format=json"]);
    assert_eq!(findings(&out), [(1, 3, 8)]);
}

#[test]
fn the_documented_rule_document_and_cli_reference_describe_the_incomplete_graph_no_op() {
    // Every semantic rule reports nothing on an incomplete graph; the user-facing
    // rule, index, CLI and README documents must all state that contract.
    let rule = include_str!("../docs/rules/v3001.md");
    assert!(rule.contains("when the graph is incomplete"), "{rule}");
    assert!(rule.contains("emits nothing"), "{rule}");
    assert!(rule.contains("unused-parameters"), "{rule}");
    let index = include_str!("../docs/rules.md");
    assert!(
        index.contains("all three rules report nothing for that file"),
        "{index}"
    );
    let cli = include_str!("../docs/cli.md");
    assert!(
        cli.contains("V3001, V3002 and V3003 on an incomplete graph"),
        "{cli}"
    );
    assert!(
        cli.contains("All three semantic rules report nothing at all"),
        "the CLI reference must state the incomplete-graph no-op: {cli}"
    );
    let readme = include_str!("../README.md");
    assert!(
        readme.contains("All three rules report nothing at all"),
        "{readme}"
    );
}
