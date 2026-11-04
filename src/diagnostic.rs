use std::ops::Range;

use serde::Serialize;

use crate::{
    rules::{Rule, Severity},
    source::Source,
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    pub byte_offset: usize,
    pub line: usize,
    pub column: usize,
}

impl Position {
    fn at(source: &Source, byte_offset: usize) -> Self {
        let (line, column) = source.position(byte_offset);
        Self {
            byte_offset,
            line,
            column,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub path: String,
    pub rule_id: &'static str,
    /// Effective severity: the registry default unless `[lint.severity]`
    /// overrides this rule for the current project.
    pub severity: Severity,
    pub message: &'static str,
    pub range: SourceRange,
    pub fixable: bool,
    /// Set when a suppression directive removed this finding from ordinary
    /// output. Present on every diagnostic in JSON so a consumer can filter
    /// without inferring intent from which run produced the object.
    pub suppressed: bool,
}

impl Diagnostic {
    pub fn new(
        path: &str,
        source: &Source,
        range: Range<usize>,
        rule: &Rule,
        severity: Option<Severity>,
    ) -> Self {
        Self {
            path: path.into(),
            rule_id: rule.id,
            severity: severity.unwrap_or(rule.severity),
            message: rule.message,
            range: SourceRange {
                start: Position::at(source, range.start),
                end: Position::at(source, range.end),
            },
            fixable: rule.fixable,
            suppressed: false,
        }
    }
}
