use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::{
    diagnostic::Diagnostic,
    lex::Kind,
    source::{Failure, Source},
    syntax::Document,
};

pub const MAX_DIAGNOSTICS: usize = 10_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Settings {
    pub select: Vec<String>,
    pub ignore: Vec<String>,
    pub max_line_length: usize,
}

impl Default for Settings {
    fn default() -> Self {
        // This intermediate slice implements V1001 only. Never pretend the
        // remaining planned rules were executed; they explicitly fail below.
        Self {
            select: vec!["V1001".into()],
            ignore: vec![],
            max_line_length: 120,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        for id in self.select.iter().chain(&self.ignore) {
            match id.as_str() {
                "V1001" => (),
                "V1002" | "V1003" | "V2001" | "V2002" => {
                    return Err(format!(
                        "rule {id} is planned but not implemented in this slice"
                    ));
                }
                _ => return Err(format!("unknown rule ID {id:?}; use complete IDs")),
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

pub fn lint(
    source: &Source,
    path: &str,
    active: &BTreeSet<String>,
    budget: usize,
) -> Result<Vec<Diagnostic>, Failure> {
    let document = Document::parse(source)?;
    let mut diagnostics = Vec::new();
    for token in &document.tokens {
        // Until L04, never silently ignore a requested suppression directive.
        if token.kind.is_comment()
            && token.kind == Kind::LineComment
            && token
                .text(source)
                .trim_start_matches('#')
                .trim_start()
                .starts_with("verse-lint:")
        {
            return Err(Failure::new(
                token.range.start,
                "suppression directives are not implemented in this slice",
            ));
        }
        if !token.kind.is_trivia() {
            continue;
        }
        if active.contains("V1001") && token.kind == Kind::Space {
            let tail = &source.text()[token.range.end..];
            if tail.is_empty() || tail.starts_with(source.newline()) {
                if diagnostics.len() >= budget {
                    return Err(Failure::new(
                        token.range.start,
                        "diagnostic limit exceeded (10000 per invocation); inspection incomplete",
                    ));
                }
                diagnostics.push(Diagnostic::trailing_whitespace(
                    path,
                    source,
                    token.range.clone(),
                ));
            }
        }
    }
    Ok(diagnostics)
}
