//! The documented resource limits, named once.
//!
//! Nothing here enforces a limit: every entry carries the value the enforcement
//! path already uses (`config`, `files`, `lex`, `rules`, `source`, `syntax`), so
//! the table cannot disagree with production. `docs_drift` reads it to check the
//! numbers `docs/cli.md` and `docs/parser-design.md` advertise, which is why the
//! module is compiled only for tests.
//!
//! `claim` is the key: it is the phrase the document must use after the number,
//! so renaming a claim in the prose means renaming it here too.
//!
//! Deliberately absent: the `max-line-length` range (documented as prose rather
//! than as a limit bullet) and internal caps no document advertises
//! (`files::MAX_WALK_ERRORS`, `files::MAX_DIRECTORY_ENTRIES`).
use std::time::Duration;

use crate::{
    config::MAX_CONFIG_BYTES,
    files::{
        MAX_DIRECTORY_DEPTH, MAX_EXCLUDE_GLOB_BYTES, MAX_EXCLUDE_GLOBS, MAX_IGNORE_BYTES,
        MAX_SOURCE_FILES, MAX_TOTAL_BYTES, MAX_VISITED_ENTRIES,
    },
    lex::MAX_TOKENS,
    rules::MAX_DIAGNOSTICS,
    source::{MAX_NESTING, MAX_SOURCE_BYTES},
    syntax::{MAX_CST_DEPTH, MAX_FINGERPRINT_EVENTS, PARSE_BUDGET},
};

pub const CLI_DOC: &str = "docs/cli.md";
pub const PARSER_DOC: &str = "docs/parser-design.md";

/// Base unit a limit is counted in. The documented unit has to belong to the
/// same family, so a count can never be checked as a byte size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Base {
    Count,
    Bytes,
    Milliseconds,
}

/// One limit exactly as its document advertises it.
pub struct Limit {
    /// The document that has to keep the claim.
    pub document: &'static str,
    /// The documented claim, i.e. the bullet text after the number and unit.
    pub claim: &'static str,
    pub base: Base,
    /// The enforced value, in `base` units.
    pub value: usize,
    /// The constant that enforces it, named in the failure message.
    pub constant: &'static str,
}

const fn millis(duration: Duration) -> usize {
    duration.as_secs() as usize * 1_000 + duration.subsec_millis() as usize
}

pub const LIMITS: [Limit; 17] = [
    Limit {
        document: CLI_DOC,
        claim: "per source file",
        base: Base::Bytes,
        value: MAX_SOURCE_BYTES,
        constant: "source::MAX_SOURCE_BYTES",
    },
    Limit {
        document: CLI_DOC,
        claim: "total successfully read input",
        base: Base::Bytes,
        value: MAX_TOTAL_BYTES,
        constant: "files::MAX_TOTAL_BYTES",
    },
    Limit {
        document: CLI_DOC,
        claim: "source files",
        base: Base::Count,
        value: MAX_SOURCE_FILES,
        constant: "files::MAX_SOURCE_FILES",
    },
    Limit {
        document: CLI_DOC,
        claim: "visited entries",
        base: Base::Count,
        value: MAX_VISITED_ENTRIES,
        constant: "files::MAX_VISITED_ENTRIES",
    },
    Limit {
        document: CLI_DOC,
        claim: "directory levels",
        base: Base::Count,
        value: MAX_DIRECTORY_DEPTH,
        constant: "files::MAX_DIRECTORY_DEPTH",
    },
    Limit {
        document: CLI_DOC,
        claim: "config file",
        base: Base::Bytes,
        value: MAX_CONFIG_BYTES,
        constant: "config::MAX_CONFIG_BYTES",
    },
    Limit {
        document: CLI_DOC,
        claim: "ignore file",
        base: Base::Bytes,
        value: MAX_IGNORE_BYTES,
        constant: "files::MAX_IGNORE_BYTES",
    },
    Limit {
        document: CLI_DOC,
        claim: "exclude globs",
        base: Base::Count,
        value: MAX_EXCLUDE_GLOBS,
        constant: "files::MAX_EXCLUDE_GLOBS",
    },
    Limit {
        // `verse-lint.ignore` shares the glob-count bound, but it is a separate
        // source with its own budget, so the document states it separately.
        document: CLI_DOC,
        claim: "ignore patterns",
        base: Base::Count,
        value: MAX_EXCLUDE_GLOBS,
        constant: "files::MAX_EXCLUDE_GLOBS",
    },
    Limit {
        document: CLI_DOC,
        claim: "per exclude glob",
        base: Base::Bytes,
        value: MAX_EXCLUDE_GLOB_BYTES,
        constant: "files::MAX_EXCLUDE_GLOB_BYTES",
    },
    Limit {
        document: CLI_DOC,
        claim: "diagnostics per invocation",
        base: Base::Count,
        value: MAX_DIAGNOSTICS,
        constant: "rules::MAX_DIAGNOSTICS",
    },
    Limit {
        document: PARSER_DOC,
        claim: "per source file",
        base: Base::Bytes,
        value: MAX_SOURCE_BYTES,
        constant: "source::MAX_SOURCE_BYTES",
    },
    Limit {
        document: PARSER_DOC,
        claim: "nesting levels",
        base: Base::Count,
        value: MAX_NESTING,
        constant: "source::MAX_NESTING",
    },
    Limit {
        document: PARSER_DOC,
        claim: "tokens",
        base: Base::Count,
        value: MAX_TOKENS,
        constant: "lex::MAX_TOKENS",
    },
    Limit {
        document: PARSER_DOC,
        claim: "parse budget",
        base: Base::Milliseconds,
        value: millis(PARSE_BUDGET),
        constant: "syntax::PARSE_BUDGET",
    },
    Limit {
        document: PARSER_DOC,
        claim: "CST depth",
        base: Base::Count,
        value: MAX_CST_DEPTH,
        constant: "syntax::MAX_CST_DEPTH",
    },
    Limit {
        document: PARSER_DOC,
        claim: "fingerprint events",
        base: Base::Count,
        value: MAX_FINGERPRINT_EVENTS,
        constant: "syntax::MAX_FINGERPRINT_EVENTS",
    },
];
