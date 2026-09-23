use crate::{
    lex::{Kind, Token},
    rules,
    source::{Failure, Source},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Suppressions {
    lines: BTreeMap<usize, BTreeSet<String>>,
    pub directive_offsets: BTreeSet<usize>,
}

impl Suppressions {
    pub fn parse(source: &Source, tokens: &[Token]) -> Result<Self, Failure> {
        let mut result = Self::default();
        for (index, token) in tokens.iter().enumerate() {
            if token.kind != Kind::LineComment {
                continue;
            }
            let text = token.text(source)[1..].trim_start();
            if !text.starts_with("verse-lint:") {
                continue;
            }
            let invalid = |message| Failure::new(token.range.start, message);
            let body = text
                .strip_prefix("verse-lint:")
                .ok_or_else(|| invalid("invalid verse-lint directive prefix"))?
                .trim();
            let (command, reason) = body
                .split_once("--")
                .ok_or_else(|| invalid("suppression requires '-- reason'"))?;
            if !command.ends_with(char::is_whitespace)
                || !reason.starts_with(char::is_whitespace)
                || reason.trim().is_empty()
            {
                return Err(invalid(
                    "suppression requires a nonempty reason separated by ' -- '",
                ));
            }
            let (mode, ids) = command
                .trim()
                .split_once(char::is_whitespace)
                .ok_or_else(|| invalid("suppression requires mode and complete rule IDs"))?;
            let (line, _) = source.position(token.range.start);
            let target = match mode {
                "disable-next-line" => line + 1,
                "disable-line" => {
                    let start = source.line_start(token.range.start);
                    let has_code = tokens[..index]
                        .iter()
                        .rev()
                        .take_while(|t| t.range.end > start)
                        .any(|t| !t.kind.is_trivia() && !t.kind.is_comment());
                    if !has_code {
                        return Err(invalid(
                            "disable-line requires preceding code on the same physical line",
                        ));
                    }
                    line
                }
                _ => {
                    return Err(invalid(
                        "unsupported suppression mode; use disable-line or disable-next-line",
                    ));
                }
            };
            for id in ids.trim().split(',').map(str::trim) {
                if !rules::REGISTRY.iter().any(|r| r.id == id) {
                    return Err(invalid("suppression contains an unknown or empty rule ID"));
                }
                if id == "V1002" {
                    return Err(invalid(
                        "file-level V1002 cannot be suppressed by a line directive; use config ignore",
                    ));
                }
                result.lines.entry(target).or_default().insert(id.into());
            }
            result.directive_offsets.insert(token.range.start);
        }
        Ok(result)
    }

    pub fn contains(&self, line: usize, id: &str) -> bool {
        self.lines.get(&line).is_some_and(|ids| ids.contains(id))
    }
}
