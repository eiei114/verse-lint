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

pub struct Rule {
    pub id: &'static str,
    pub severity: &'static str,
    pub message: &'static str,
    pub fixable: bool,
}
pub const REGISTRY: [Rule; 5] = [
    Rule {
        id: "V1001",
        severity: "error",
        message: "Trailing whitespace",
        fixable: true,
    },
    Rule {
        id: "V1002",
        severity: "error",
        message: "Missing final newline",
        fixable: true,
    },
    Rule {
        id: "V1003",
        severity: "error",
        message: "Tab in structural indentation",
        fixable: false,
    },
    Rule {
        id: "V2001",
        severity: "warning",
        message: "Line exceeds configured maximum length",
        fixable: false,
    },
    Rule {
        id: "V2002",
        severity: "warning",
        message: "TODO comment needs a nonempty parenthesized reference",
        fixable: false,
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
            select: vec!["V1001".into(), "V1002".into(), "V1003".into()],
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
            if tail.is_empty() || tail.starts_with(source.newline()) {
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
            let body = line.strip_suffix(source.newline()).unwrap_or(line);
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
