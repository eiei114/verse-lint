use crate::{
    diagnostic::Diagnostic,
    lex::Kind,
    source::{Failure, Source},
    suppressions::Suppressions,
    syntax::Document,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ops::Range};

pub const MAX_DIAGNOSTICS: usize = 10_000;

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
    pub severity: &'static str,
    pub message: &'static str,
    pub fixable: bool,
    /// Selected when configuration omits `[lint] select`.
    pub selected_by_default: bool,
}
pub const REGISTRY: [Rule; 5] = [
    Rule {
        id: "V1001",
        name: "TrailingWhitespace",
        severity: "error",
        message: "Trailing whitespace",
        fixable: true,
        selected_by_default: true,
    },
    Rule {
        id: "V1002",
        name: "MissingFinalNewline",
        severity: "error",
        message: "Missing final newline",
        fixable: true,
        selected_by_default: true,
    },
    Rule {
        id: "V1003",
        name: "TabIndentation",
        severity: "error",
        message: "Tab in structural indentation",
        fixable: false,
        selected_by_default: true,
    },
    Rule {
        id: "V2001",
        name: "LineTooLong",
        severity: "warning",
        message: "Line exceeds configured maximum length",
        fixable: false,
        selected_by_default: false,
    },
    Rule {
        id: "V2002",
        name: "BareTodoComment",
        severity: "warning",
        message: "TODO comment needs a nonempty parenthesized reference",
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

/// Every ID a detector can emit. Test-side contract table: the registry tests
/// check [`REGISTRY`] against it in both directions.
#[cfg(test)]
pub const DETECTOR_IDS: [&str; 5] = [V1001, V1002, V1003, V2001, V2002];

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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    pub max_line_length: usize,
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
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        for id in self.select.iter().chain(&self.ignore) {
            if !REGISTRY.iter().any(|r| r.id == id) {
                return Err(format!("unknown rule ID {id:?}; use complete IDs"));
            }
        }
        if self.max_line_length == 0 || self.max_line_length > 1_000_000 {
            return Err("max-line-length must be between 1 and 1000000".into());
        }
        Ok(())
    }
    pub fn active(&self) -> Result<BTreeSet<String>, String> {
        self.validate()?;
        let mut selected: BTreeSet<_> = self.select.iter().cloned().collect();
        for id in &self.ignore {
            selected.remove(id);
        }
        if selected.is_empty() {
            return Err("no active lint rules".into());
        }
        Ok(selected)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LintResult {
    pub diagnostics: Vec<Diagnostic>,
    pub suppressed: usize,
}

pub fn lint(
    source: &Source,
    path: &str,
    active: &BTreeSet<String>,
    max_line_length: usize,
    budget: usize,
) -> Result<LintResult, Failure> {
    lint_with(source, path, active, max_line_length, budget, &REGISTRY)
}

/// Lint against an explicit registry order. Detector dispatch is by rule ID, so
/// any permutation of the registry produces the same diagnostics.
fn lint_with(
    source: &Source,
    path: &str,
    active: &BTreeSet<String>,
    max_line_length: usize,
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
        let diagnostic = Diagnostic::new(path, source, range, rule);
        if suppressions.contains(diagnostic.range.start.line, rule.id) {
            result.suppressed += 1;
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
                if width > max_line_length && excess.is_none() {
                    excess = Some(i);
                }
            }
            if let Some(excess) = excess {
                emit(V2001, offset + excess..offset + body.len())?;
            }
            offset += line.len();
        }
    }
    result
        .diagnostics
        .sort_by_key(|d| (d.range.start.byte_offset, d.rule_id));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Documented contract per rule, addressed by stable ID. Order is not part
    /// of the contract, so this is looked up by ID rather than by position.
    const DOCUMENTED: [(&str, &str, &str, bool, bool); 5] = [
        (V1001, "TrailingWhitespace", "error", true, true),
        (V1002, "MissingFinalNewline", "error", true, true),
        (V1003, "TabIndentation", "error", false, true),
        (V2001, "LineTooLong", "warning", false, false),
        (V2002, "BareTodoComment", "warning", false, false),
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
            12,
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
            assert_eq!(rule.severity, severity, "{id} severity");
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
        assert_eq!(baseline.suppressed, reversed.suppressed);
        // Each finding still reports the metadata of the rule that produced it.
        for diagnostic in &reversed.diagnostics {
            let documented = DOCUMENTED
                .iter()
                .find(|(id, ..)| *id == diagnostic.rule_id)
                .expect("diagnostic rule is documented");
            assert_eq!(
                diagnostic.severity, documented.2,
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
}
