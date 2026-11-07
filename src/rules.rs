use crate::{
    diagnostic::Diagnostic,
    lex::Kind,
    semantic::ScopeGraph,
    source::{Failure, Source},
    suppressions::Suppressions,
    syntax::Document,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

pub const MAX_DIAGNOSTICS: usize = 10_000;

/// How a finding is reported. [`REGISTRY`] holds the tool default; `[lint.severity]`
/// may override it per project. Only these two values exist, so no configuration
/// can invent a third or an unknown casing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }

    /// Accepts exactly the two documented literals; anything else is rejected.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "error" => Some(Self::Error),
            "warning" => Some(Self::Warning),
            _ => None,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Per-run detector configuration that is not rule identity.
///
/// These settings come from `[lint]` and none selects or suppresses a rule, so
/// they travel together instead of as separate arguments to every lint call.
#[derive(Clone, Debug)]
pub struct Options {
    /// Used only when V2001 is selected.
    pub max_line_length: usize,
    /// Used only when V3001 is selected: whether an unused parameter is
    /// reported as well as an unused local.
    pub unused_parameters: bool,
    /// Names a project explicitly treats as provided by another file or API.
    /// Used only when V3002 is selected.
    pub external_names: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        let settings = Settings::default();
        Self {
            max_line_length: settings.max_line_length,
            unused_parameters: settings.unused_parameters,
            external_names: settings.external_names,
        }
    }
}

/// Effective severity overrides keyed by complete rule ID. Empty means every
/// rule keeps its registry default.
pub type SeverityOverrides = BTreeMap<String, Severity>;

/// Single source of truth for rule identity and policy. SARIF metadata, the
/// default rule selection and CLI/config validation all read this registry, so
/// adding or reordering a rule never needs a parallel positional list. Detectors
/// address entries by the [`V1001`]-style ID constants, never by index.
#[derive(Clone, Debug)]
pub struct Rule {
    pub id: &'static str,
    /// SARIF `reportingDescriptor.name`; the rule document lives next to the
    /// lowercase ID (`docs/rules/<id>.md`), not at this name.
    pub name: &'static str,
    pub severity: Severity,
    pub message: &'static str,
    pub fixable: bool,
    /// Selected when configuration omits `[lint] select`.
    pub selected_by_default: bool,
}
pub const REGISTRY: [Rule; 7] = [
    Rule {
        id: "V1001",
        name: "TrailingWhitespace",
        severity: Severity::Error,
        message: "Trailing whitespace",
        fixable: true,
        selected_by_default: true,
    },
    Rule {
        id: "V1002",
        name: "MissingFinalNewline",
        severity: Severity::Error,
        message: "Missing final newline",
        fixable: true,
        selected_by_default: true,
    },
    Rule {
        id: "V1003",
        name: "TabIndentation",
        severity: Severity::Error,
        message: "Tab in structural indentation",
        fixable: false,
        selected_by_default: true,
    },
    Rule {
        id: "V2001",
        name: "LineTooLong",
        severity: Severity::Warning,
        message: "Line exceeds configured maximum length",
        fixable: false,
        selected_by_default: false,
    },
    Rule {
        id: "V2002",
        name: "BareTodoComment",
        severity: Severity::Warning,
        message: "TODO comment needs a nonempty parenthesized reference",
        fixable: false,
        selected_by_default: false,
    },
    Rule {
        id: "V3001",
        name: "UnusedDeclaration",
        severity: Severity::Warning,
        message: "Declaration is never referenced",
        fixable: false,
        selected_by_default: false,
    },
    Rule {
        id: "V3002",
        name: "UnresolvedReference",
        severity: Severity::Warning,
        message: "Reference is not declared in a modeled local scope",
        fixable: false,
        selected_by_default: false,
    },
];

/// Stable rule IDs. Detectors dispatch on these, never on a registry position,
/// so reordering [`REGISTRY`] cannot rebind a diagnostic to another rule.
pub const V1001: &str = "V1001";
pub const V1002: &str = "V1002";
pub const V1003: &str = "V1003";
pub const V2001: &str = "V2001";
pub const V2002: &str = "V2002";
pub const V3001: &str = "V3001";
pub const V3002: &str = "V3002";

/// Every ID a detector can emit. Test-side contract table: the registry tests
/// check [`REGISTRY`] against it in both directions.
#[cfg(test)]
pub const DETECTOR_IDS: [&str; 7] = [V1001, V1002, V1003, V2001, V2002, V3001, V3002];

/// `select`/`ignore` entries accept a complete ID (`V1001`) or a numeric prefix
/// (`V`, `V1`, `V2`). A complete ID is itself a valid prefix, so one shape check
/// covers both forms: a leading `V` followed only by ASCII digits. Anything else
/// — `v1`, `V1001x`, an empty string — is malformed, never guessed at.
fn is_well_formed_entry(entry: &str) -> bool {
    entry
        .strip_prefix('V')
        .is_some_and(|digits| digits.bytes().all(|b| b.is_ascii_digit()))
}

/// Expand one `select`/`ignore` entry against the registry, in registry order.
/// A well-formed entry that matches no registered rule is a failure rather than
/// a silent no-op, so a typo can neither empty nor unexpectedly widen a run.
/// New rules become reachable through an existing prefix automatically, because
/// expansion always reads the current registry.
pub fn expand_entry(entry: &str) -> Result<Vec<&'static str>, String> {
    if !is_well_formed_entry(entry) {
        return Err(format!(
            "invalid rule entry {entry:?} in select/ignore; use a complete rule ID like \"V1001\" or a prefix of \"V\" plus digits like \"V1\""
        ));
    }
    let matched: Vec<&'static str> = REGISTRY
        .iter()
        .map(|rule| rule.id)
        .filter(|id| id.starts_with(entry))
        .collect();
    if matched.is_empty() {
        return Err(format!(
            "rule prefix {entry:?} in select/ignore matched no registered rule"
        ));
    }
    Ok(matched)
}

/// Expand every entry of one list, collapsing duplicates into stable ID order.
fn expand_entries(entries: &[String]) -> Result<BTreeSet<String>, String> {
    let mut expanded = BTreeSet::new();
    for entry in entries {
        expanded.extend(expand_entry(entry)?.into_iter().map(str::to_owned));
    }
    Ok(expanded)
}

fn lookup<'a>(registry: &'a [Rule], id: &str) -> &'a Rule {
    registry
        .iter()
        .find(|rule| rule.id == id)
        .expect("rule ID must be registered")
}

/// Resolve a registry entry by ID, independent of its position.
#[cfg(test)]
pub fn rule(id: &str) -> &'static Rule {
    lookup(&REGISTRY, id)
}

/// Read `[lint.severity]` as text keyed by rule ID. Values are validated in
/// [`Settings::validate`], which can name both the rule and the accepted
/// literals; a non-string value is rejected here so the message still names the
/// offending key instead of a bare "invalid type".
fn deserialize_severity<'de, D>(deserializer: D) -> Result<BTreeMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let raw = BTreeMap::<String, toml::Value>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(id, value)| match value {
            toml::Value::String(text) => Ok((id, text)),
            other => Err(Error::custom(format!(
                "invalid severity {other} for {id} in [lint.severity]; expected a quoted \"error\" or \"warning\""
            ))),
        })
        .collect()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    pub max_line_length: usize,
    /// `[lint] unused-parameters`: whether V3001 also reports a parameter with
    /// no resolved reference. `false` by default, because an unused parameter is
    /// usually dictated by a callback signature rather than left behind.
    pub unused_parameters: bool,
    /// `[lint] external-names`: unresolved names a project knows are supplied
    /// by another file or an API not modeled by this local scope graph.
    pub external_names: Vec<String>,
    /// `[lint.severity]`: rule ID to replacement severity text. Kept as text so
    /// [`Settings::validate`] can name the offending key itself instead of
    /// deferring to a generic deserializer message.
    #[serde(deserialize_with = "deserialize_severity")]
    pub severity: BTreeMap<String, String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            select: REGISTRY
                .iter()
                .filter(|rule| rule.selected_by_default)
                .map(|rule| rule.id.into())
                .collect(),
            ignore: vec![],
            max_line_length: 120,
            unused_parameters: false,
            external_names: vec![],
            severity: BTreeMap::new(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        // Expansion is the validation: a malformed entry, or a prefix that
        // matches nothing, can never be silently dropped from `select` or
        // `ignore`. Syntactically complete IDs stay complete IDs, so existing
        // configurations keep their meaning.
        self.expanded_select()?;
        self.expanded_ignore()?;
        if self.max_line_length == 0 || self.max_line_length > 1_000_000 {
            return Err("max-line-length must be between 1 and 1000000".into());
        }
        if self.external_names.iter().any(|name| name.is_empty()) {
            return Err("external-names entries must not be empty".into());
        }
        for (id, value) in &self.severity {
            if !REGISTRY.iter().any(|r| r.id == id) {
                return Err(format!(
                    "unknown rule ID {id:?} in [lint.severity]; use complete IDs"
                ));
            }
            if Severity::parse(value).is_none() {
                return Err(format!(
                    "invalid severity {value:?} for {id} in [lint.severity]; expected \"error\" or \"warning\""
                ));
            }
        }
        Ok(())
    }

    /// Effective per-rule severity overrides. Independent of selection: an
    /// override on an unselected rule is accepted and simply has no finding to
    /// apply to.
    pub fn severity_overrides(&self) -> Result<SeverityOverrides, String> {
        let mut overrides = SeverityOverrides::new();
        for (id, value) in &self.severity {
            let severity = Severity::parse(value).ok_or_else(|| {
                format!(
                    "invalid severity {value:?} for {id} in [lint.severity]; expected \"error\" or \"warning\""
                )
            })?;
            overrides.insert(id.clone(), severity);
        }
        Ok(overrides)
    }

    /// Every registered rule with the severity it would report at today, in
    /// stable ID order. `--show-config` renders this map.
    pub fn effective_severity(&self) -> Result<BTreeMap<String, String>, String> {
        let overrides = self.severity_overrides()?;
        Ok(REGISTRY
            .iter()
            .map(|rule| {
                let severity = overrides.get(rule.id).copied().unwrap_or(rule.severity);
                (rule.id.to_owned(), severity.as_str().to_owned())
            })
            .collect())
    }
    /// Complete IDs behind the configured `select` list, in stable ID order.
    /// `--show-config` renders this so an effective set containing no prefix is
    /// unambiguous.
    pub fn expanded_select(&self) -> Result<Vec<String>, String> {
        Ok(expand_entries(&self.select)?.into_iter().collect())
    }

    /// Complete IDs behind the configured `ignore` list, in stable ID order.
    pub fn expanded_ignore(&self) -> Result<Vec<String>, String> {
        Ok(expand_entries(&self.ignore)?.into_iter().collect())
    }

    /// The detector configuration these settings imply. A pure projection, so a
    /// caller cannot pass a line length or a parameter mode that disagrees with
    /// the resolved configuration.
    pub fn options(&self) -> Options {
        Options {
            max_line_length: self.max_line_length,
            unused_parameters: self.unused_parameters,
            external_names: self.external_names.clone(),
        }
    }

    pub fn active(&self) -> Result<BTreeSet<String>, String> {
        self.validate()?;
        // Both lists are expanded before subtraction, so `--select V --ignore V2`
        // is the V1 family and a prefix never subtracts by string identity.
        let mut selected = expand_entries(&self.select)?;
        for id in expand_entries(&self.ignore)? {
            selected.remove(&id);
        }
        if selected.is_empty() {
            return Err("no active lint rules".into());
        }
        Ok(selected)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LintResult {
    /// Findings that will be reported. Suppressed findings are never in here.
    pub diagnostics: Vec<Diagnostic>,
    /// Findings a suppression directive removed, kept so `--show-suppressed` can
    /// report them. The count is the only trace a default run prints, and it is
    /// what the diagnostic budget and `summary.suppressed` use.
    pub suppressed: Vec<Diagnostic>,
}

impl LintResult {
    /// Findings removed by suppression directives, shown or not.
    pub fn suppressed_count(&self) -> usize {
        self.suppressed.len()
    }
}

pub fn lint(
    source: &Source,
    path: &str,
    active: &BTreeSet<String>,
    severities: &SeverityOverrides,
    options: Options,
    budget: usize,
) -> Result<LintResult, Failure> {
    lint_with(source, path, active, severities, options, budget, &REGISTRY)
}

/// Lint against an explicit registry order. Detector dispatch is by rule ID, so
/// any permutation of the registry produces the same diagnostics.
fn lint_with(
    source: &Source,
    path: &str,
    active: &BTreeSet<String>,
    severities: &SeverityOverrides,
    options: Options,
    budget: usize,
    registry: &[Rule],
) -> Result<LintResult, Failure> {
    let document = Document::parse(source)?;
    let suppressions = Suppressions::parse(source, &document.tokens)?;
    let mut result = LintResult::default();
    let mut count = 0;
    let mut emit = |id: &str, range: Range<usize>| -> Result<(), Failure> {
        let rule = lookup(registry, id);
        if !active.contains(rule.id) {
            return Ok(());
        }
        if count >= budget {
            return Err(Failure::new(
                range.start,
                "diagnostic limit exceeded (10000 per invocation, including suppressed); inspection incomplete",
            ));
        }
        count += 1;
        let mut diagnostic =
            Diagnostic::new(path, source, range, rule, severities.get(rule.id).copied());
        if suppressions.contains(diagnostic.range.start.line, rule.id) {
            // Marked at the source of truth: the flag travels with the finding,
            // so text, JSON and SARIF cannot disagree about what was suppressed.
            diagnostic.suppressed = true;
            result.suppressed.push(diagnostic);
        } else {
            result.diagnostics.push(diagnostic);
        }
        Ok(())
    };
    let mut depth = 0usize;
    for (index, token) in document.tokens.iter().enumerate() {
        if token.kind == Kind::Space {
            let tail = &source.text()[token.range.end..];
            if tail.is_empty() || matches!(tail.as_bytes().first(), Some(b'\r' | b'\n')) {
                emit(V1001, token.range.clone())?;
            }
            if depth == 0
                && token.range.start == source.line_start(token.range.start)
                && token.text(source).contains('\t')
            {
                let line_end = source.text()[token.range.end..]
                    .find('\n')
                    .map_or(source.text().len(), |i| token.range.end + i);
                let code = document.tokens[index + 1..]
                    .iter()
                    .take_while(|t| t.range.start < line_end)
                    .any(|t| !t.kind.is_trivia() && !t.kind.is_comment());
                if code {
                    emit(V1003, token.range.clone())?;
                }
            }
        }
        if token.kind == Kind::Code {
            match token.text(source) {
                "(" | "[" | "{" => depth += 1,
                ")" | "]" | "}" => depth = depth.saturating_sub(1),
                _ => (),
            }
        }
        if active.contains(V2002)
            && token.kind.is_comment()
            && !suppressions.directive_offsets.contains(&token.range.start)
        {
            let text = token.text(source);
            // One advancing cursor avoids quadratic rescans for TODO( repeated
            // throughout a large comment without any closing parenthesis.
            let mut ends = text
                .char_indices()
                .filter(|(_, c)| matches!(c, ')' | '\r' | '\n'))
                .peekable();
            for (offset, _) in text.match_indices("TODO") {
                let word = |c: char| c.is_alphanumeric() || c == '_';
                if text[..offset].chars().next_back().is_some_and(word)
                    || text[offset + 4..].chars().next().is_some_and(word)
                {
                    continue;
                }
                let tail = &text[offset + 4..];
                while ends.peek().is_some_and(|(end, _)| *end < offset + 5) {
                    ends.next();
                }
                let reference = tail.starts_with('(')
                    && ends.peek().is_some_and(|(end, c)| {
                        *c == ')' && text[offset + 5..*end].chars().any(|c| !c.is_whitespace())
                    });
                if !reference {
                    emit(
                        V2002,
                        token.range.start + offset..token.range.start + offset + 4,
                    )?;
                }
            }
        }
    }
    if source.text().len() > source.body_start() && !source.text().ends_with('\n') {
        emit(V1002, source.text().len()..source.text().len())?;
    }
    if active.contains(V2001) {
        let mut offset = source.body_start();
        for line in source.text()[offset..].split_inclusive('\n') {
            let body = line.strip_suffix('\n').unwrap_or(line);
            let body = body.strip_suffix('\r').unwrap_or(body);
            let (mut width, mut excess) = (0, None);
            for (i, c) in body.char_indices() {
                width += if c == '\t' { 4 - width % 4 } else { 1 };
                if width > options.max_line_length && excess.is_none() {
                    excess = Some(i);
                }
            }
            if let Some(excess) = excess {
                emit(V2001, offset + excess..offset + body.len())?;
            }
            offset += line.len();
        }
    }
    if active.contains(V3001) || active.contains(V3002) {
        // Semantic rules share one scope graph. `reportable()` makes an
        // incomplete model a documented no-op: neither an "unused" claim nor
        // an unresolved-name claim is safe when the graph could not see every
        // relevant name.
        let graph = ScopeGraph::build(source, &document)?;
        if let Some(graph) = graph.reportable() {
            if active.contains(V3001) {
                for binding in graph.unused(options.unused_parameters) {
                    emit(V3001, binding.range.clone())?;
                }
            }
            if active.contains(V3002) {
                for reference in graph.unresolved(&options.external_names) {
                    emit(V3002, reference.range.clone())?;
                }
            }
        }
    }
    // Both lists use the same key, so showing suppressed findings never changes
    // the order active findings are printed in.
    let order = |d: &Diagnostic| (d.range.start.byte_offset, d.rule_id);
    result.diagnostics.sort_by_key(order);
    result.suppressed.sort_by_key(order);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Documented contract per rule, addressed by stable ID. Order is not part
    /// of the contract, so this is looked up by ID rather than by position.
    const DOCUMENTED: [(&str, &str, &str, bool, bool); 7] = [
        (V1001, "TrailingWhitespace", "error", true, true),
        (V1002, "MissingFinalNewline", "error", true, true),
        (V1003, "TabIndentation", "error", false, true),
        (V2001, "LineTooLong", "warning", false, false),
        (V2002, "BareTodoComment", "warning", false, false),
        (V3001, "UnusedDeclaration", "warning", false, false),
        (V3002, "UnresolvedReference", "warning", false, false),
    ];

    fn sample() -> Source {
        Source::from_bytes(b"f():void =\n\tPrint(\"aaaaaaaaaaaa\")  \n# TODO later\n# trailing\t")
            .expect("sample source")
    }

    fn active(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|id| (*id).to_string()).collect()
    }

    fn lint_all(registry: &[Rule], active: &BTreeSet<String>) -> LintResult {
        lint_with(
            &sample(),
            "sample.verse",
            active,
            &SeverityOverrides::new(),
            Options {
                max_line_length: 12,
                unused_parameters: false,
                external_names: vec![],
            },
            MAX_DIAGNOSTICS,
            registry,
        )
        .expect("lint succeeds")
    }

    fn reordered() -> Vec<Rule> {
        let mut rules: Vec<Rule> = REGISTRY.to_vec();
        rules.reverse();
        rules
    }

    #[test]
    fn registry_metadata_matches_documented_contract_by_id() {
        // Every registry entry carries the documented metadata, looked up by ID.
        for (id, name, severity, fixable, selected_by_default) in DOCUMENTED {
            let rule = rule(id);
            assert_eq!(rule.id, id);
            assert_eq!(rule.name, name, "{id} SARIF name");
            assert_eq!(rule.severity.as_str(), severity, "{id} severity");
            assert_eq!(rule.fixable, fixable, "{id} fixable");
            assert_eq!(
                rule.selected_by_default, selected_by_default,
                "{id} default selection"
            );
        }
        // Coverage both ways: no missing and no extra registry entries.
        assert_eq!(REGISTRY.len(), DETECTOR_IDS.len());
        for id in DETECTOR_IDS {
            assert!(
                REGISTRY.iter().any(|rule| rule.id == id),
                "detector ID {id} is not registered"
            );
        }
        for rule in &REGISTRY {
            assert!(
                DETECTOR_IDS.contains(&rule.id),
                "{} is registered without a detector",
                rule.id
            );
        }
        // Default selection is derived from the registry, not a parallel list.
        let settings = Settings::default();
        let mut default_selection = settings.select.clone();
        default_selection.sort();
        assert_eq!(default_selection, [V1001, V1002, V1003]);
    }

    #[test]
    fn detector_dispatch_stays_attached_to_rule_ids_under_registry_reorder() {
        let ids = active(&DETECTOR_IDS);
        let baseline = lint_all(&REGISTRY, &ids);
        let reversed = lint_all(&reordered(), &ids);
        assert!(
            !baseline.diagnostics.is_empty(),
            "sample must be diagnosable"
        );
        let outcome = |result: &LintResult| {
            result
                .diagnostics
                .iter()
                .map(|d| (d.rule_id, d.severity, d.message, d.fixable))
                .collect::<Vec<_>>()
        };
        // Same findings, same rule identity: a reorder cannot rebind detectors.
        assert_eq!(outcome(&baseline), outcome(&reversed));
        assert_eq!(baseline.suppressed_count(), reversed.suppressed_count());
        // Each finding still reports the metadata of the rule that produced it.
        for diagnostic in &reversed.diagnostics {
            let documented = DOCUMENTED
                .iter()
                .find(|(id, ..)| *id == diagnostic.rule_id)
                .expect("diagnostic rule is documented");
            assert_eq!(
                diagnostic.severity.as_str(),
                documented.2,
                "{} severity",
                documented.0
            );
            assert_eq!(rule(diagnostic.rule_id).name, documented.1);
        }
        // Guard the intended sample coverage so the equality above is meaningful.
        let mut found: Vec<_> = reversed.diagnostics.iter().map(|d| d.rule_id).collect();
        found.sort_unstable();
        found.dedup();
        assert_eq!(found, [V1001, V1002, V1003, V2001, V2002]);
    }

    #[test]
    fn selection_by_id_is_unaffected_by_registry_order() {
        let only = active(&[V2001]);
        let baseline = lint_all(&REGISTRY, &only);
        let reversed = lint_all(&reordered(), &only);
        assert_eq!(baseline.diagnostics.len(), 1);
        assert_eq!(baseline.diagnostics[0].rule_id, V2001);
        assert_eq!(reversed.diagnostics.len(), 1);
        assert_eq!(reversed.diagnostics[0].rule_id, V2001);
        assert_eq!(
            reversed.diagnostics[0].message,
            rule(V2001).message,
            "selection must keep the V2001 identity, not the entry that replaced it"
        );
    }

    #[test]
    fn suppressed_findings_are_retained_marked_and_ordered_like_reported_ones() {
        let source = Source::from_bytes(
            b"A := 1  \n# verse-lint: disable-next-line V1001 -- reason\nB := 2  \nC := 3  \n",
        )
        .expect("sample source");
        let result = lint_with(
            &source,
            "sample.verse",
            &active(&[V1001]),
            &SeverityOverrides::new(),
            Options::default(),
            MAX_DIAGNOSTICS,
            &REGISTRY,
        )
        .expect("lint succeeds");

        // Retained, not merely counted: the finding keeps its identity and
        // carries the flag a consumer filters on.
        assert_eq!(result.suppressed_count(), 1);
        assert!(result.suppressed.iter().all(|d| d.suppressed));
        assert!(result.diagnostics.iter().all(|d| !d.suppressed));
        assert_eq!(result.suppressed[0].rule_id, V1001);
        assert_eq!(result.suppressed[0].range.start.line, 3);

        // Both lists are sorted by the same key, so printing one after the other
        // and re-sorting is the same list in the same order.
        let order = |d: &Diagnostic| (d.range.start.byte_offset, d.rule_id);
        let mut combined: Vec<_> = result
            .diagnostics
            .iter()
            .chain(&result.suppressed)
            .cloned()
            .collect();
        combined.sort_by_key(order);
        let lines: Vec<_> = combined.iter().map(|d| d.range.start.line).collect();
        assert_eq!(lines, [1, 3, 4]);
        assert!(combined[1].suppressed);

        // The same input without a directive yields the same findings and an
        // empty suppressed list: the flag never changes detection.
        let plain = Source::from_bytes(b"A := 1  \nB := 2  \nC := 3  \n").unwrap();
        let without = lint_with(
            &plain,
            "sample.verse",
            &active(&[V1001]),
            &SeverityOverrides::new(),
            Options::default(),
            MAX_DIAGNOSTICS,
            &REGISTRY,
        )
        .unwrap();
        assert_eq!(without.suppressed_count(), 0);
        let outcome = |r: &LintResult| {
            r.diagnostics
                .iter()
                .chain(&r.suppressed)
                .map(|d| (d.rule_id, d.message))
                .collect::<Vec<_>>()
        };
        assert_eq!(outcome(&result).len(), 3);
        assert_eq!(outcome(&result), outcome(&without));
    }

    fn settings_with(select: &[&str], ignore: &[&str]) -> Settings {
        Settings {
            select: select.iter().map(|id| (*id).into()).collect(),
            ignore: ignore.iter().map(|id| (*id).into()).collect(),
            ..Settings::default()
        }
    }

    #[test]
    fn a_prefix_expands_to_every_matching_registered_rule_in_registry_order() {
        let family = expand_entry("V1").expect("V1 is registered");
        assert_eq!(family, [V1001, V1002, V1003]);
        let policy = expand_entry("V2").expect("V2 is registered");
        assert_eq!(policy, [V2001, V2002]);
        // The semantic family is its own prefix: a V2 policy warning and a V3
        // semantic claim are different opt-ins and never select each other.
        assert_eq!(
            expand_entry("V3").expect("V3 is registered"),
            [V3001, V3002]
        );
        // `V` is a prefix of every ID, so it is the whole registry.
        let all: Vec<_> = REGISTRY.iter().map(|rule| rule.id).collect();
        assert_eq!(expand_entry("V").unwrap(), all);
        // A complete ID is a prefix of itself and of nothing else in the registry.
        assert_eq!(expand_entry(V1001).unwrap(), [V1001]);
        assert_eq!(expand_entry("V2001").unwrap(), [V2001]);
        // Registry order, not alphabetical or hash order, drives expansion.
        assert_eq!(
            expand_entry("V").unwrap(),
            REGISTRY.iter().map(|rule| rule.id).collect::<Vec<_>>()
        );
    }

    #[test]
    fn zero_match_and_malformed_entries_are_errors_that_name_the_entry() {
        // A well-formed prefix that no registered ID starts with.
        let error = expand_entry("V9").unwrap_err();
        assert!(error.contains("\"V9\""), "{error}");
        assert!(error.contains("matched no registered rule"), "{error}");
        assert!(expand_entry("V0000000").unwrap_err().contains("V0000000"));
        // Malformed shapes are never guessed at: lowercase, trailing junk,
        // empty, a bare leading V-less ID and non-ASCII digits all fail.
        for entry in [
            "v1", "V1x", "V1001x", "", "1001", "V1 ", " V1", "V１", "VV1", "V-1", "V1.0",
        ] {
            let error = expand_entry(entry)
                .err()
                .unwrap_or_else(|| panic!("{entry:?} must be rejected"));
            assert!(error.contains("invalid rule entry"), "{entry:?}: {error}");
        }
    }

    #[test]
    fn select_and_ignore_are_expanded_before_subtraction_and_duplicates_collapse() {
        // Documented example: the whole registry minus the V2 family is V1 plus
        // the semantic family, which is a separate prefix on purpose — a V2
        // policy warning and a V3 semantic claim are different opt-ins.
        let settings = settings_with(&["V"], &["V2"]);
        assert_eq!(
            settings.active().unwrap(),
            active(&[V1001, V1002, V1003, V3001, V3002])
        );
        // A prefix subtracts the rule it actually names, not the literal text.
        assert_eq!(
            settings_with(&["V1", "V2"], &["V2002"]).active().unwrap(),
            active(&[V1001, V1002, V1003, V2001])
        );
        // Overlapping entries collapse to one effective set.
        assert_eq!(
            settings_with(&["V", "V1", V1001, "V1001"], &["V1", V1002])
                .active()
                .unwrap(),
            active(&[V2001, V2002, V3001, V3002])
        );
        // Subtracting everything is still the existing empty-selection failure.
        let error = settings_with(&["V1"], &["V1"]).active().unwrap_err();
        assert!(error.contains("no active lint rules"), "{error}");
        assert!(
            settings_with(&["V"], &["V"])
                .active()
                .unwrap_err()
                .contains("no active lint rules")
        );
    }

    #[test]
    fn validation_rejects_prefixes_only_where_the_contract_forbids_them() {
        // A zero-match prefix in either list fails validation, not just active().
        for bad in [settings_with(&["V9"], &[]), settings_with(&[], &["V9"])] {
            assert!(bad.validate().unwrap_err().contains("V9"));
        }
        // Prefix acceptance is confined to select/ignore: `[lint.severity]` keeps
        // requiring complete registered IDs.
        let error = settings(&[("V1", "error")]).validate().unwrap_err();
        assert!(error.contains("rule ID"), "{error}");
        assert!(
            !error.contains("entry"),
            "severity must not accept prefixes: {error}"
        );
    }

    #[test]
    fn expanded_lists_render_complete_ids_for_show_config() {
        let settings = settings_with(&["V2", V1002, V1002], &["V1"]);
        assert_eq!(settings.expanded_select().unwrap(), [V1002, V2001, V2002]);
        assert_eq!(settings.expanded_ignore().unwrap(), [V1001, V1002, V1003]);
    }

    #[test]
    fn registry_names_are_unique_and_map_to_existing_rule_documents() {
        let mut ids: Vec<_> = REGISTRY.iter().map(|r| r.id).collect();
        let mut names: Vec<_> = REGISTRY.iter().map(|r| r.name).collect();
        ids.sort_unstable();
        ids.dedup();
        names.sort_unstable();
        names.dedup();
        assert_eq!(ids.len(), REGISTRY.len());
        assert_eq!(names.len(), REGISTRY.len());
        for rule in &REGISTRY {
            let path = format!(
                "{}/docs/rules/{}.md",
                env!("CARGO_MANIFEST_DIR"),
                rule.id.to_lowercase()
            );
            assert!(
                std::path::Path::new(&path).is_file(),
                "missing rule document for {}: {path}",
                rule.id
            );
            assert!(
                rule.name.is_ascii(),
                "SARIF name must be ASCII: {}",
                rule.name
            );
        }
    }

    fn settings(severity: &[(&str, &str)]) -> Settings {
        Settings {
            severity: severity
                .iter()
                .map(|(id, value)| ((*id).into(), (*value).into()))
                .collect(),
            ..Settings::default()
        }
    }

    #[test]
    fn severity_accepts_only_the_two_exact_literals() {
        assert_eq!(Severity::parse("error"), Some(Severity::Error));
        assert_eq!(Severity::parse("warning"), Some(Severity::Warning));
        for invalid in ["Error", "Warning", "ERROR", "", "notice", "err", " error"] {
            assert_eq!(Severity::parse(invalid), None, "{invalid:?}");
        }
        assert_eq!(Severity::Error.to_string(), "error");
        assert_eq!(Severity::Warning.to_string(), "warning");
    }

    #[test]
    fn overrides_are_validated_against_the_registry_and_never_select_rules() {
        let overriding = settings(&[(V2001, "error"), (V1001, "warning")]);
        overriding.validate().expect("registered IDs are accepted");
        // Severity must not participate in selection.
        let with_override = overriding.active().unwrap();
        let without = Settings::default().active().unwrap();
        assert_eq!(with_override, without);
        assert!(!with_override.contains(V2001), "V2001 stays opt-in");

        for bad in [
            settings(&[("V9999", "error")]),
            settings(&[("V1", "error")]),
        ] {
            let error = bad.validate().unwrap_err();
            assert!(error.contains("rule ID"), "{error}");
        }
        for value in ["Error", "", "notice"] {
            let error = settings(&[(V2001, value)]).validate().unwrap_err();
            assert!(error.contains(V2001), "{error}");
            assert!(error.contains("severity"), "{error}");
        }
    }

    #[test]
    fn effective_severity_covers_every_rule_in_stable_id_order() {
        let settings = settings(&[(V2001, "error"), (V1003, "warning")]);
        let effective = settings.effective_severity().unwrap();
        let ordered: Vec<_> = effective
            .iter()
            .map(|(id, s)| (id.as_str(), s.as_str()))
            .collect();
        assert_eq!(
            ordered,
            [
                (V1001, "error"),
                (V1002, "error"),
                (V1003, "warning"),
                (V2001, "error"),
                (V2002, "warning"),
                (V3001, "warning"),
                (V3002, "warning"),
            ]
        );
        // Every registered rule is present, and the key order is the stable
        // ID order that `--show-config` relies on.
        assert_eq!(effective.len(), REGISTRY.len());
        let mut ids: Vec<_> = effective.keys().cloned().collect();
        ids.sort();
        assert_eq!(ids, effective.keys().cloned().collect::<Vec<_>>());
    }

    #[test]
    fn effective_severity_reaches_diagnostics_without_touching_detection() {
        let ids = active(&DETECTOR_IDS);
        let baseline = lint_all(&REGISTRY, &ids);
        let overrides: SeverityOverrides = [
            (V1001.to_owned(), Severity::Warning),
            (V2002.to_owned(), Severity::Error),
        ]
        .into_iter()
        .collect();
        let overridden = lint_with(
            &sample(),
            "sample.verse",
            &ids,
            &overrides,
            Options {
                max_line_length: 12,
                unused_parameters: false,
                external_names: vec![],
            },
            MAX_DIAGNOSTICS,
            &REGISTRY,
        )
        .expect("lint succeeds");

        let identity = |result: &LintResult| {
            result
                .diagnostics
                .iter()
                .map(|d| (d.rule_id, d.message, d.fixable))
                .collect::<Vec<_>>()
        };
        // Only the reported severity changed: same findings, same order, same
        // fixability, so detectors and dispatch are untouched.
        assert_eq!(identity(&baseline), identity(&overridden));
        assert_eq!(baseline.suppressed_count(), overridden.suppressed_count());
        for diagnostic in &overridden.diagnostics {
            let expected = match diagnostic.rule_id {
                V1001 => Severity::Warning,
                V2002 => Severity::Error,
                other => rule(other).severity,
            };
            assert_eq!(diagnostic.severity, expected, "{}", diagnostic.rule_id);
        }
        assert!(
            overridden
                .diagnostics
                .iter()
                .any(|d| d.severity != rule(d.rule_id).severity),
            "sample must exercise an override"
        );
    }
}
