//! Pure edit planning: no filesystem effects and no formatter layout pass.
use crate::{
    rules::{self, LintResult},
    source::{Failure, Source},
    syntax::Document,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, ops::Range};

#[derive(Clone, Debug)]
struct Edit {
    range: Range<usize>,
    expected: String,
    replacement: String,
    source_hash: [u8; 32],
}

pub struct Plan {
    pub original: LintResult,
    pub output: String,
    pub after: LintResult,
}

fn edits(source: &Source, lint: &LintResult) -> Result<Vec<Edit>, Failure> {
    let hash = Sha256::digest(source.text().as_bytes()).into();
    let mut edits = Vec::new();
    for diagnostic in &lint.diagnostics {
        if !diagnostic.fixable {
            continue;
        }
        let range = diagnostic.range.start.byte_offset..diagnostic.range.end.byte_offset;
        let expected = source
            .text()
            .get(range.clone())
            .ok_or_else(|| Failure::new(range.start, "invalid fix range"))?;
        let replacement = match diagnostic.rule_id {
            "V1001"
                if !expected.is_empty() && expected.bytes().all(|b| matches!(b, b' ' | b'\t')) =>
            {
                ""
            }
            "V1002" if range.is_empty() && range.start == source.text().len() => source.newline(),
            _ => return Err(Failure::new(range.start, "unsafe or unknown edit refused")),
        };
        edits.push(Edit {
            range,
            expected: expected.into(),
            replacement: replacement.into(),
            source_hash: hash,
        });
    }
    Ok(edits)
}

fn apply(source: &Source, mut edits: Vec<Edit>) -> Result<String, Failure> {
    let hash: [u8; 32] = Sha256::digest(source.text().as_bytes()).into();
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    for (index, edit) in edits.iter().enumerate() {
        if edit.source_hash != hash
            || source.text().get(edit.range.clone()) != Some(edit.expected.as_str())
        {
            return Err(Failure::new(
                edit.range.start,
                "fix source hash/span changed",
            ));
        }
        if index > 0 {
            let previous = &edits[index - 1];
            if previous.range.end > edit.range.start || previous.range.start == edit.range.start {
                return Err(Failure::new(
                    edit.range.start,
                    "overlapping or same-position fixes refused",
                ));
            }
        }
    }
    let mut result = source.text().to_owned();
    for edit in edits.into_iter().rev() {
        result.replace_range(edit.range, &edit.replacement);
    }
    Ok(result)
}

pub fn plan(
    source: &Source,
    path: &str,
    active: &BTreeSet<String>,
    max_line_length: usize,
    budget: usize,
) -> Result<Plan, Failure> {
    let original = rules::lint(source, path, active, max_line_length, budget)?;
    let output = apply(source, edits(source, &original)?)?;
    if output == source.text() {
        return Ok(Plan {
            after: original.clone(),
            original,
            output,
        });
    }
    let candidate = Source::from_bytes(output.as_bytes())?;
    let before = Document::parse(source)?;
    let after = Document::parse(&candidate)?;
    if !before.equivalent(source, &after, &candidate) {
        return Err(Failure::new(
            0,
            "fix failed token/structure/protected-span preservation",
        ));
    }
    let after = rules::lint(&candidate, path, active, max_line_length, budget)?;
    if !edits(&candidate, &after)?.is_empty() {
        return Err(Failure::new(0, "safe fixes failed one-pass convergence"));
    }
    Ok(Plan {
        original,
        output,
        after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_overlap_duplicate_insertions_hash_and_expected_span_mismatch() {
        let source = Source::from_bytes(b"A := 1  ").unwrap();
        let result = rules::lint(
            &source,
            "a",
            &rules::Settings::default().active().unwrap(),
            120,
            100,
        )
        .unwrap();
        let edits = edits(&source, &result).unwrap();
        assert_eq!(apply(&source, edits.clone()).unwrap(), "A := 1\n");
        let mut duplicate = edits.clone();
        duplicate.push(edits[1].clone());
        assert!(apply(&source, duplicate).is_err());
        let mut overlap = edits.clone();
        overlap[1].range = 7..8;
        overlap[1].expected = " ".into();
        assert!(apply(&source, overlap).is_err());
        let mut wrong = edits.clone();
        wrong[0].expected = "x".into();
        assert!(apply(&source, wrong).is_err());
        assert!(apply(&Source::from_bytes(b"B := 1  ").unwrap(), edits).is_err());
    }
}
