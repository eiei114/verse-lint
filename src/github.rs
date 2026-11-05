//! GitHub Actions workflow commands for pull-request review.
//!
//! One annotation line per reported diagnostic on stdout and nothing else: no
//! ANSI, no summary, no `::group::`, `::set-output`, `::add-matcher` or upload
//! step. Execution errors stay on stderr with the same text contract as the
//! `text` format, so a tool or configuration failure never becomes a workflow
//! command and never changes the exit-code contract.
//!
//! Field values are escaped as GitHub requires, because a raw `%`, CR, LF, `:`
//! or `,` inside a path or message would otherwise terminate a property and
//! silently truncate or mis-attribute the annotation. Escaping is lossless: the
//! runner decodes every escape before it resolves the file or shows the text.
use std::io::Write;

use crate::{
    diagnostic::Diagnostic,
    report::Report,
    rules::{self, Severity},
};

/// A workflow-command `message` escapes only these three sequences.
const MESSAGE_ESCAPES: [(char, &str); 3] = [('%', "%25"), ('\r', "%0D"), ('\n', "%0A")];

/// A workflow-command property value additionally reserves the `:` and `,`
/// separators, so a Windows drive letter or a path with a comma stays one value.
const PROPERTY_ESCAPES: [(char, &str); 5] = [
    ('%', "%25"),
    ('\r', "%0D"),
    ('\n', "%0A"),
    (':', "%3A"),
    (',', "%2C"),
];

/// Replace each reserved character with its documented percent sequence. A
/// single left-to-right pass is required: re-escaping an emitted `%25` would
/// corrupt the value.
fn escape(text: &str, table: &[(char, &'static str)]) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match table.iter().find(|(reserved, _)| *reserved == character) {
            Some((_, sequence)) => escaped.push_str(sequence),
            None => escaped.push(character),
        }
    }
    escaped
}

/// The rule `reportingDescriptor.name` behind an emitted rule ID. Diagnostics can
/// only be produced by a registered rule, so a missing entry is a programming
/// error rather than an input-dependent condition.
fn rule_name(rule_id: &str) -> &'static str {
    rules::REGISTRY
        .iter()
        .find(|rule| rule.id == rule_id)
        .map(|rule| rule.name)
        .expect("registered diagnostic rule")
}

/// `error` for active errors, `warning` for active warnings, and `notice` for a
/// finding a suppression directive removed. `notice` is the documented choice:
/// GitHub's command vocabulary has no per-finding "suppressed" field, and a
/// notice cannot fail a check run, which matches the rule that a suppressed
/// finding never changes the exit code.
fn command(diagnostic: &Diagnostic) -> &'static str {
    if diagnostic.suppressed {
        return "notice";
    }
    match diagnostic.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

/// One annotation. The region numbers are the same one-based line and Unicode
/// scalar column values JSON and SARIF report, so all three formats agree on
/// where a finding is.
pub fn annotation(diagnostic: &Diagnostic) -> String {
    let start = &diagnostic.range.start;
    let end = &diagnostic.range.end;
    format!(
        "::{} file={},line={},col={},endLine={},endColumn={},title={}::{}",
        command(diagnostic),
        escape(&diagnostic.path, &PROPERTY_ESCAPES),
        start.line,
        start.column,
        end.line,
        end.column,
        escape(
            &format!("{} {}", diagnostic.rule_id, rule_name(diagnostic.rule_id)),
            &PROPERTY_ESCAPES
        ),
        escape(diagnostic.message, &MESSAGE_ESCAPES),
    )
}

/// Write every recorded diagnostic as one annotation, in the shared sort order.
pub fn emit(report: &Report, out: &mut impl Write) -> Result<(), String> {
    for diagnostic in &report.diagnostics {
        writeln!(out, "{}", annotation(diagnostic)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{Position, SourceRange};

    fn diagnostic(path: &str, message: &'static str) -> Diagnostic {
        Diagnostic {
            path: path.into(),
            rule_id: rules::V1001,
            severity: Severity::Error,
            message,
            range: SourceRange {
                start: Position {
                    byte_offset: 4,
                    line: 2,
                    column: 7,
                },
                end: Position {
                    byte_offset: 6,
                    line: 2,
                    column: 9,
                },
            },
            fixable: true,
            suppressed: false,
        }
    }

    #[test]
    fn escapes_are_applied_in_one_pass_per_field_kind() {
        assert_eq!(escape("100%", &MESSAGE_ESCAPES), "100%25");
        assert_eq!(escape("a\rb\nc", &MESSAGE_ESCAPES), "a%0Db%0Ac");
        // A message keeps its colons and commas: only properties reserve them.
        assert_eq!(escape("a:b,c", &MESSAGE_ESCAPES), "a:b,c");
        assert_eq!(escape("a:b,c", &PROPERTY_ESCAPES), "a%3Ab%2Cc");
        // Already-escaped text must not be escaped twice: one left-to-right pass
        // emits each sequence exactly once.
        assert_eq!(escape("%25", &PROPERTY_ESCAPES), "%2525");
        assert_eq!(escape("%0A", &MESSAGE_ESCAPES), "%250A");
        // Non-ASCII passes through; it is not a workflow-command reserved byte.
        assert_eq!(escape("日本, 語", &PROPERTY_ESCAPES), "日本%2C 語");
    }

    #[test]
    fn a_message_with_a_colon_and_newline_stays_one_annotation() {
        let mut report = Report::default();
        let bad = diagnostic(
            "src/x.verse",
            "Line too long: add 12, then rewrap\nsecond line 100%",
        );
        report.diagnostics.push(bad);
        let mut out = Vec::new();
        emit(&report, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "::error file=src/x.verse,line=2,col=7,endLine=2,endColumn=9,title=V1001 TrailingWhitespace::Line too long: add 12, then rewrap%0Asecond line 100%25\n"
        );
        assert_eq!(text.lines().count(), 1, "an embedded LF must stay escaped");
    }

    #[test]
    fn a_path_with_every_reserved_property_character_stays_one_value() {
        let mut report = Report::default();
        report
            .diagnostics
            .push(diagnostic("a, b:c\nd%e.verse", "message"));
        let mut out = Vec::new();
        emit(&report, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "::error file=a%2C b%3Ac%0Ad%25e.verse,line=2,col=7,endLine=2,endColumn=9,title=V1001 TrailingWhitespace::message\n"
        );
    }

    #[test]
    fn suppressed_findings_are_notices_and_warnings_stay_warnings() {
        let mut report = Report::default();
        let mut suppressed = diagnostic("a.verse", "message");
        suppressed.suppressed = true;
        report.diagnostics.push(suppressed);
        let mut warning = diagnostic("a.verse", "message");
        warning.severity = Severity::Warning;
        warning.rule_id = rules::V2001;
        report.diagnostics.push(warning);
        let mut out = Vec::new();
        emit(&report, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("::notice "), "{text}");
        assert!(
            text.contains("::warning file=a.verse"),
            "a warning must not borrow the notice level: {text}"
        );
        assert!(!text.contains("::error"), "{text}");
    }
}
