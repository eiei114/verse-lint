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
/// adding or reordering a rule never needs a parallel positional list.
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
    let document = Document::parse(source)?;
    let suppressions = Suppressions::parse(source, &document.tokens)?;
    let mut result = LintResult::default();
    let mut count = 0;
    let mut emit = |index: usize, range: Range<usize>| -> Result<(), Failure> {
        let rule = &REGISTRY[index];
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
                emit(0, token.range.clone())?;
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
                    emit(2, token.range.clone())?;
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
        if active.contains("V2002")
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
                        4,
                        token.range.start + offset..token.range.start + offset + 4,
                    )?;
                }
            }
        }
    }
    if source.text().len() > source.body_start() && !source.text().ends_with('\n') {
        emit(1, source.text().len()..source.text().len())?;
    }
    if active.contains("V2001") {
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
                emit(3, offset + excess..offset + body.len())?;
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

    #[test]
    fn registry_metadata_keeps_documented_ids_names_and_default_selection() {
        let actual: Vec<_> = REGISTRY
            .iter()
            .map(|r| (r.id, r.name, r.severity, r.fixable, r.selected_by_default))
            .collect();
        assert_eq!(
            actual,
            [
                ("V1001", "TrailingWhitespace", "error", true, true),
                ("V1002", "MissingFinalNewline", "error", true, true),
                ("V1003", "TabIndentation", "error", false, true),
                ("V2001", "LineTooLong", "warning", false, false),
                ("V2002", "BareTodoComment", "warning", false, false),
            ]
        );
        // Default selection is derived from the registry, not a parallel list.
        let settings = Settings::default();
        let default_selection: Vec<_> = settings.select.iter().map(String::as_str).collect();
        assert_eq!(default_selection, ["V1001", "V1002", "V1003"]);
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
