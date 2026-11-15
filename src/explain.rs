//! `--explain <RULE>`: print one rule's registry summary and its embedded
//! document.
//!
//! This is the only surface that answers "what is this `V3003` in my CI output?"
//! for someone running an installed binary: the Markdown comes from
//! [`crate::rules::embedded_doc`], which `include_str!`s the repository's own
//! `docs/rules/<id>.md` into the executable, so nothing is read from disk at run
//! time and `docs/` never has to ship next to the binary. No configuration is
//! resolved, no source is parsed and no rule runs — like `--show-config`, it
//! describes the tool rather than inspecting a project.

use std::fmt::Write as _;

use crate::rules;

/// Render the plain-text answer for one complete rule ID.
///
/// The summary line carries the registry facts that decide how the rule
/// reports — default selection, default severity and whether a safe fix exists
/// — and the embedded document follows unchanged. Output is diffable ASCII/Markdown
/// with no ANSI colour, so it stays readable in a CI log.
pub fn render(id: &str) -> Result<String, String> {
    let (rule, doc) = rules::explain_entry(id)?;
    let mut out = String::with_capacity(doc.len() + 128);
    writeln!(
        out,
        "{} {} — default: {}; default severity: {}; safe fix: {}",
        rule.id,
        rule.name,
        if rule.selected_by_default {
            "on"
        } else {
            "off"
        },
        rule.severity.as_str(),
        if rule.fixable { "yes" } else { "none" },
    )
    .expect("formatting a string cannot fail");
    writeln!(out, "document: docs/rules/{}.md", rule.id.to_lowercase())
        .expect("formatting a string cannot fail");
    out.push('\n');
    out.push_str(doc);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    Ok(out)
}
