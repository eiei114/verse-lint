//! Rule-family prefixes for `--select`/`--ignore` and the `[lint] select`/
//! `[lint] ignore` arrays. An entry is either a complete ID (`V1001`) or a
//! prefix of `V` plus digits (`V`, `V1`, `V2`). Expansion runs over the current
//! registry before subtraction, a prefix matching nothing is an error rather
//! than a silent no-op, and suppression directives keep requiring complete IDs.
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

/// Every rule family in the registry, so a test that selects `V` proves the
/// whole registry is reachable instead of only the default three.
const ALL_IDS: [&str; 9] = [
    "V1001", "V1002", "V1003", "V2001", "V2002", "V2003", "V3001", "V3002", "V3003",
];
const V1_IDS: [&str; 3] = ["V1001", "V1002", "V1003"];
const V2_IDS: [&str; 2] = ["V2001", "V2002"];
/// The complete V2 family as the registry expands it. V2003 is part of the
/// selection but is silent on this fixture, whose lines are all LF, so it is not
/// in [`V2_IDS`], which lists the rules that actually report here.
const V2_REGISTERED: [&str; 3] = ["V2001", "V2002", "V2003"];
/// The V3001/V3002/V3003 semantic family is separate from the V2 policy warnings.
/// `SEMANTIC_SOURCE` below only carries a V3001 finding, so this is the set of
/// IDs the family prefix reports for that fixture, not the family's membership.
const V3_IDS: [&str; 1] = ["V3001"];

/// A single comment line whose width exceeds the default 120 columns, so V2001
/// fires once and nothing else does. It deliberately carries no trailing blank
/// and no `TODO`, keeping the other four rules out of it.
const LONG_LINE: &str = "# aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

/// One finding per *style* rule, so a wrong active set changes the reported ID
/// set: a bare TODO (V2002), tab indentation (V1003), trailing whitespace
/// (V1001), an over-long line (V2001) and a missing final newline (V1002).
///
/// V3001 has no finding here on purpose: this fixture declares no function-body
/// local, so it keeps one detector per family and the prefix assertions stay
/// about selection. The semantic family has its own fixture, `SEMANTIC_SOURCE`.
fn every_rule() -> String {
    format!("# TODO bare\nf():void =\n\tPrint(\"x\")  \n{LONG_LINE}")
}

/// One V3001-only finding: a function-body local with no reference. It carries
/// no style finding at all, so the reported ID set here is exactly the semantic
/// rules an active set reaches.
const SEMANTIC_SOURCE: &str = "f():void =\n    Unused := 1\n    Print(\"x\")\n";

/// The style rules `every_rule()` can report, which is every registered rule
/// except V3001, V3002, V3003 and V2003. `--select V` reaches all nine; only these
/// five have something to say about that fixture.
const STYLE_IDS: [&str; 5] = ["V1001", "V1002", "V1003", "V2001", "V2002"];

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

fn project(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("verse.toml"), config).unwrap();
    fs::write(dir.path().join("all.verse"), every_rule()).unwrap();
    dir
}

/// The reported rule IDs, deduplicated and sorted, for an active-set assertion.
fn reported(report: &Value) -> Vec<String> {
    let mut ids: Vec<String> = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["ruleId"].as_str().unwrap().to_owned())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

fn sorted(ids: &[&str]) -> Vec<String> {
    let mut ids: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
    ids.sort();
    ids
}

#[test]
fn a_family_prefix_selects_exactly_the_registered_rules_it_names() {
    let dir = project("");
    // `--select V1` is the default three rules, and only those.
    let (code, report) = json(dir.path(), &["--select", "V1"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    // `--select V` reaches every registered rule, including the opt-in ones.
    // The fixture only has style findings, so both semantic rules are silent;
    // the semantic family is covered by its own test below.
    let (code, report) = json(dir.path(), &["--select", "V"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&STYLE_IDS));
    // `--select V2` is exactly the two policy warnings.
    let (code, report) = json(dir.path(), &["--select", "V2"]);
    assert_eq!(code, 0, "warnings alone exit 0: {report}");
    assert_eq!(reported(&report), sorted(&V2_IDS));
    assert_eq!(report["summary"]["warnings"], 2);
    assert_eq!(report["summary"]["errors"], 0);
    // A prefix that names a single registered rule is still just that rule.
    let (code, report) = json(dir.path(), &["--select", "V100"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    let (code, report) = json(dir.path(), &["--select", "V2002"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(reported(&report), sorted(&["V2002"]));
}

/// The semantic family is reached by its own prefix and by no other: `V3`
/// selects V3001, V3002 and V3003, and `V2` never brings them in. This is the
/// whole contract a reader of `docs/rules.md` relies on when opting in to
/// semantic rules separately from policy warnings.
#[test]
fn the_semantic_family_is_its_own_prefix_and_reports_through_it() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("semantic.verse"), SEMANTIC_SOURCE).unwrap();
    let (code, report) = json(dir.path(), &["--select", "V3"]);
    assert_eq!(code, 0, "a warning alone exits 0: {report}");
    assert_eq!(reported(&report), sorted(&V3_IDS));
    assert_eq!(report["summary"]["warnings"], 1);
    // `V2` is the policy warnings only; the semantic rule stays unselected.
    let (code, report) = json(dir.path(), &["--select", "V2"]);
    assert_eq!(code, 0, "{report}");
    assert!(
        report["diagnostics"].as_array().unwrap().is_empty(),
        "{report}"
    );
    // The whole registry reaches it, and the surviving finding keeps its rule.
    // A V3001-only file has one warning and no error, so a full selection still
    // exits 0: selecting a rule never changes the severity contract.
    let (code, report) = json(dir.path(), &["--select", "V"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(reported(&report), sorted(&V3_IDS));
    // `--ignore V3` subtracts it from a full selection by ID, not by text.
    let (code, report) = json(dir.path(), &["--select", "V", "--ignore", "V3"]);
    assert_eq!(code, 0, "{report}");
    assert!(
        report["diagnostics"].as_array().unwrap().is_empty(),
        "{report}"
    );
    // It is off by default, like every other opt-in rule.
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 0, "{report}");
    assert!(
        report["diagnostics"].as_array().unwrap().is_empty(),
        "{report}"
    );
}

#[test]
fn an_ignore_prefix_removes_exactly_its_family_from_the_selection() {
    let dir = project("");
    // Default selection minus the V2 family is unchanged, because V2 is opt-in.
    let (code, report) = json(dir.path(), &["--ignore", "V2"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    // Removing V1 leaves the default selection empty: still exit 2.
    let (code, report) = json(dir.path(), &["--ignore", "V1"]);
    assert_eq!(code, 2, "{report}");
    assert_eq!(report["summary"]["complete"], false);
    assert!(
        report["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no active lint rules")
    );
    // Select everything, then drop one family: the other families survive.
    // V3001 is selected but reports nothing in this fixture, so `reported` is
    // exactly the two policy warnings.
    let (code, report) = json(dir.path(), &["--select", "V", "--ignore", "V1"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(reported(&report), sorted(&V2_IDS));
    // Replacing config ignore with a prefix behaves the same way.
    let (code, report) = json(dir.path(), &["--select", "V", "--ignore", "V2"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
}

#[test]
fn select_and_ignore_expand_before_subtraction_so_prefixes_never_cancel_by_text() {
    let dir = project("");
    // `--select V --ignore V2` is documented as the V1 family. V3001 is
    // selected by the `V` prefix but has nothing to report here.
    let (code, report) = json(dir.path(), &["--select", "V", "--ignore", "V2"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    // A prefix ignore against an overlapping select removes rules, not a string.
    let (code, report) = json(dir.path(), &["--select", "V1,V2001", "--ignore", "V100"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(reported(&report), sorted(&["V2001"]));
    // The same contract holds through the configuration arrays.
    let dir = project("[lint]\nselect = ['V']\nignore = ['V2']\n");
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    // CLI `--select` replaces the configured selection, and a configured
    // `ignore` keeps subtracting until `--ignore` replaces it in turn.
    let dir = project("[lint]\nselect = ['V1']\nignore = ['V1']\n");
    let (code, report) = json(dir.path(), &["--select", "V2"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(reported(&report), sorted(&V2_IDS));
    let (code, report) = json(dir.path(), &["--select", "V", "--ignore", "V1"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(reported(&report), sorted(&V2_IDS));
}

#[test]
fn duplicate_and_overlapping_entries_collapse() {
    let dir = project("");
    let (baseline_code, baseline) = json(dir.path(), &["--select", "V1"]);
    let (code, report) = json(
        dir.path(),
        &["--select", "V,V1,V1001,V1001,V1", "--ignore", "V2,V2001"],
    );
    assert_eq!(code, 1, "{report}");
    assert_eq!(code, baseline_code, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    // Repeating a prefix must not repeat, drop or reorder any finding.
    assert_eq!(
        report["diagnostics"], baseline["diagnostics"],
        "duplicate entries must collapse to the same active set: {report}"
    );
}

#[test]
fn zero_match_malformed_lowercase_and_empty_entries_exit_two_naming_the_entry() {
    let dir = project("");
    // A well-formed prefix no registered rule starts with.
    let (code, report) = json(dir.path(), &["--select", "V9"]);
    assert_eq!(code, 2, "{report}");
    assert_eq!(report["summary"]["complete"], false);
    assert!(report["diagnostics"].as_array().unwrap().is_empty());
    let message = report["executionErrors"][0]["message"].as_str().unwrap();
    assert!(message.contains("\"V9\""), "{message}");
    assert!(message.contains("no registered rule"), "{message}");

    // Same failure from `--ignore`, and from a configuration array.
    let (code, report) = json(dir.path(), &["--ignore", "V9"]);
    assert_eq!(code, 2, "{report}");
    assert!(
        report["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("\"V9\"")
    );
    let dir = project("[lint]\nselect = ['V9']\n");
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 2, "{report}");
    assert!(
        report["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("\"V9\"")
    );
    let dir = project("[lint]\nignore = ['V9']\n");
    assert_eq!(json(dir.path(), &[]).0, 2);

    // Malformed shapes: lowercase, trailing junk, a bare number.
    for entry in ["v1", "v", "V1001x", "V1x", "1001"] {
        let dir = project("");
        let (code, report) = json(dir.path(), &["--select", entry]);
        assert_eq!(code, 2, "{entry}: {report}");
        assert_eq!(report["summary"]["complete"], false, "{entry}");
        let message = report["executionErrors"][0]["message"].as_str().unwrap();
        assert!(
            message.contains(&format!("{entry:?}")),
            "{entry}: message must name the offending entry: {message}"
        );
        assert!(
            message.contains("complete rule ID"),
            "{entry}: message must state the accepted forms: {message}"
        );
    }

    // Empty entries: `--select ""` and a trailing comma both produce one.
    for args in [vec!["--select", ""], vec!["--select", "V1001,"]] {
        let dir = project("");
        let (code, report) = json(dir.path(), &args);
        assert_eq!(code, 2, "{args:?}: {report}");
        assert!(
            report["executionErrors"][0]["message"]
                .as_str()
                .unwrap()
                .contains("invalid rule entry")
        );
    }
    // An empty `ignore` list from the CLI is also one empty entry, not "no list".
    let (code, _) = json(project("").path(), &["--ignore="]);
    assert_eq!(code, 2);
}

#[test]
fn show_config_prints_expanded_complete_ids_in_a_deterministic_order() {
    let dir = project("[lint]\nselect = ['V']\nignore = ['V2']\n");
    let out = run(dir.path(), &["--show-config"]);
    assert!(out.status.success(), "{out:?}");
    let shown = String::from_utf8(out.stdout.clone()).unwrap();
    assert!(
        shown.contains("# source:"),
        "show-config must keep its header: {shown}"
    );
    // No prefix survives rendering: the effective set is stated as complete IDs.
    let block = |marker: &str| -> Vec<String> {
        let start = shown
            .find(marker)
            .unwrap_or_else(|| panic!("show-config must list {marker}: {shown}"));
        let end = start + shown[start..].find(']').expect("unterminated array");
        shown[start..end]
            .split('"')
            .skip(1)
            .step_by(2)
            .map(str::to_owned)
            .collect()
    };
    assert_eq!(block("select = ["), sorted(&ALL_IDS));
    assert_eq!(block("ignore = ["), sorted(&V2_REGISTERED));
    // `ignore = ['V2']` leaves V3001 selected: the semantic family is separate.
    assert!(
        !block("ignore = [")
            .iter()
            .any(|id| V3_IDS.contains(&id.as_str()))
    );
    assert!(
        !block("select = [")
            .iter()
            .chain(block("ignore = [").iter())
            .any(|id| id.len() < 5),
        "no prefix may appear in --show-config: {shown}"
    );
    // Deterministic: same configuration, same bytes.
    assert_eq!(out.stdout, run(dir.path(), &["--show-config"]).stdout);
    // Reversing the source order must not change the rendered order. Only the
    // rule arrays are compared: `# source:`/`# root:` name each temp directory,
    // so whole-file equality across two projects would be the wrong assertion.
    let reversed = project("[lint]\nignore = ['V2']\nselect = ['V']\n");
    let reversed = String::from_utf8(run(reversed.path(), &["--show-config"]).stdout).unwrap();
    let arrays = |text: &str| -> String {
        text.lines()
            .filter(|line| {
                let line: &str = line;
                line.starts_with("select = ")
                    || line.starts_with("ignore = ")
                    || line.starts_with("    \"V")
                    || line == "]"
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(arrays(&shown), arrays(&reversed), "{reversed}");
    assert!(arrays(&shown).contains("select = ["), "{shown}");
    // A complete-ID selection renders unchanged, so the output stays honest.
    let literal = project("[lint]\nselect = ['V1002']\n");
    let shown = String::from_utf8(run(literal.path(), &["--show-config"]).stdout).unwrap();
    assert!(shown.contains("\"V1002\""), "{shown}");
    assert!(!shown.contains("\"V\""), "{shown}");
}

#[test]
fn suppression_directives_still_require_complete_ids() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.verse"),
        "# verse-lint: disable-next-line V1 -- prefixes stay invalid here\nA := 1  \n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["--select", "V1"]);
    assert_eq!(code, 2, "{report}");
    assert_eq!(report["summary"]["complete"], false);
    assert!(
        report["executionErrors"][0]["message"]
            .as_str()
            .unwrap()
            .contains("unknown or empty rule ID")
    );
    // The same directive with a complete ID keeps suppressing as before.
    fs::write(
        dir.path().join("a.verse"),
        "# verse-lint: disable-next-line V1001 -- complete ID stays supported\nA := 1  \n",
    )
    .unwrap();
    let (code, report) = json(dir.path(), &["--select", "V1"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["summary"]["suppressed"], 1);
}

#[test]
fn prefix_selection_composes_with_the_per_rule_severity_override() {
    // A prefix selects the rule; `[lint.severity]` still owns how it reports.
    let dir = project("[lint]\nselect = ['V2']\n[lint.severity]\nV2001 = 'error'\n");
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 1, "{report}");
    let long = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d["ruleId"] == "V2001")
        .expect("V2001 finding");
    assert_eq!(long["severity"], "error", "{report}");
    assert_eq!(reported(&report), sorted(&V2_IDS));
    // Promoting a rule the prefix does not select keeps it inactive.
    let dir = project("[lint]\nselect = ['V1']\n[lint.severity]\nV2001 = 'error'\n");
    let (code, report) = json(dir.path(), &[]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V1_IDS));
    // `--select V2` on the CLI replaces the config selection, so the override
    // applies to the newly selected family and not to the old one.
    let dir = project("[lint]\nselect = ['V1']\n[lint.severity]\nV2001 = 'error'\n");
    let (code, report) = json(dir.path(), &["--select", "V2"]);
    assert_eq!(code, 1, "{report}");
    assert_eq!(reported(&report), sorted(&V2_IDS));
    assert_eq!(report["summary"]["errors"], 1, "{report}");
    assert_eq!(report["summary"]["warnings"], 1, "{report}");
}

#[test]
fn prefix_expansion_changes_neither_exit_codes_nor_diagnostic_order() {
    let dir = project("");
    // The same active set expressed two ways must be byte-identical on stdout.
    let literal = run(dir.path(), &["--select", "V1,V2"]);
    let prefixed = run(dir.path(), &["--select", "V"]);
    assert_eq!(literal.status.code(), prefixed.status.code());
    assert!(literal.status.success() || literal.status.code() == Some(1));
    let (_, literal) = json(dir.path(), &["--select", "V1,V2,V3"]);
    let (_, prefixed) = json(dir.path(), &["--select", "V"]);
    assert_eq!(literal["diagnostics"], prefixed["diagnostics"]);
    assert_eq!(literal["summary"], prefixed["summary"]);
    // Diagnostics stay ordered by path, byte offset and rule ID.
    let ordered = prefixed["diagnostics"].as_array().unwrap();
    let keys: Vec<_> = ordered
        .iter()
        .map(|d| {
            (
                d["path"].as_str().unwrap().to_owned(),
                d["range"]["start"]["byteOffset"].as_u64().unwrap(),
                d["ruleId"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    let mut sorted_keys = keys.clone();
    sorted_keys.sort();
    assert_eq!(keys, sorted_keys, "{prefixed}");
    // A valid run's exit code follows the existing severity contract: a prefix
    // that selects only warnings still exits 0.
    assert_eq!(json(dir.path(), &["--select", "V2"]).0, 0);
    assert_eq!(json(dir.path(), &["--select", "V1"]).0, 1);
}
