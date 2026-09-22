use std::ops::Range;

use serde::Serialize;

use crate::{rules::Rule, source::Source};

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
    pub severity: &'static str,
    pub message: &'static str,
    pub range: SourceRange,
    pub fixable: bool,
}

impl Diagnostic {
    pub fn new(path: &str, source: &Source, range: Range<usize>, rule: &Rule) -> Self {
        Self {
            path: path.into(),
            rule_id: rule.id,
            severity: rule.severity,
            message: rule.message,
            range: SourceRange {
                start: Position::at(source, range.start),
                end: Position::at(source, range.end),
            },
            fixable: rule.fixable,
        }
    }
}
