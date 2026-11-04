//! Documentation must advertise the numbers and tables the code actually uses.
//!
//! Each test pairs one document with its source of truth and fails naming the
//! document, the claim and both values, so a drifted number points at its fix.
//! These checks never replace behavioural coverage: `tests/limits-cli.rs` and
//! the unit tests prove a limit works; this module only proves the prose still
//! describes it.
//!
//! The checked surface is fixed, so a new document is never picked up silently.
//! A document that gains or loses a limit without touching [`limits::LIMITS`]
//! fails the coverage assertions below.
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::limits::{Base, CLI_DOC, LIMITS, PARSER_DOC};

/// `docs/rules.md` carries one table row per registered rule.
const RULES_DOC: &str = "docs/rules.md";
/// `docs/syntax-snapshot.md` records the import revision and the vendor hashes.
const SNAPSHOT_DOC: &str = "docs/syntax-snapshot.md";
/// The hash manifest `scripts/verify-grammar.ps1` already validates byte by byte.
const HASH_MANIFEST: &str = "vendor/tree-sitter-verse/hashes.json";
/// `docs/syntax-snapshot.md` defers the grammar revision to this vendor record.
const UPSTREAM_DOC: &str = "vendor/tree-sitter-verse/upstream.md";

/// The revision the snapshot was imported from. This is the one provenance
/// value with no second copy in the repository — no file is compared against it
/// because the import is historical — so the pin lives here and a deliberate
/// change updates both places.
const IMPORT_REVISION: &str = "08a9e6abc3cf7cdb762fdeea2b6448b31baa22e1";

/// The imported files the snapshot records hashes for, in table order.
const IMPORTED_PATHS: [&str; 7] = [
    "src/source.rs",
    "src/lex.rs",
    "src/syntax.rs",
    "src/files.rs",
    "src/config.rs",
    "tests/fixtures/device.input.verse",
    "tests/fixtures/device.expected.verse",
];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read_doc(name: &str) -> String {
    let path = root().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {name} ({}): {e}", path.display()))
}

/// The bullet list that follows `heading`. Blank lines are tolerated inside the
/// list; anything else ends it, so surrounding prose is never parsed as a limit.
fn bullet_list<'a>(document: &'a str, heading: &str) -> Vec<&'a str> {
    let mut lines = document.lines().skip_while(|line| line.trim() != heading);
    assert!(
        lines.next().is_some(),
        "{heading:?} is expected in the document and was not found"
    );
    let mut bullets = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let Some(bullet) = line.strip_prefix("- ") else {
            break;
        };
        bullets.push(bullet);
    }
    bullets
}

/// Reads one documented limit bullet: `<number><unit?> <claim>`.
///
/// `8 MiB per source file` → `(8 MiB, claim "per source file")`,
/// `10,000 source files` → `(10,000, claim "source files")`,
/// `2-second parse budget` → `(2 seconds, claim "parse budget")`.
fn parse_bullet(bullet: &str) -> Option<(usize, Base, String)> {
    let digits_end = bullet
        .find(|c: char| !c.is_ascii_digit() && c != ',' && c != '_')
        .unwrap_or(bullet.len());
    let value: usize = bullet[..digits_end].replace([',', '_'], "").parse().ok()?;
    let rest = bullet[digits_end..].trim_start();

    // A unit is a known word, optionally hyphen-attached (`2-second`) or spaced
    // (`8 MiB`). An unknown word is the start of the claim, not a unit.
    let word_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let word = &rest[..word_end];
    let claim = rest[word_end..].trim_start().to_string();
    Some(match unit_factor(word) {
        Some((factor, base)) => (value * factor, base, claim),
        // No unit: a plain count, and the unit-looking word belongs to the claim.
        None => (value, Base::Count, rest.to_string()),
    })
}

/// The multiplier that turns a documented unit into the limit's base unit.
fn unit_factor(word: &str) -> Option<(usize, Base)> {
    Some(match word.trim_start_matches('-') {
        "B" | "byte" | "bytes" => (1, Base::Bytes),
        "KiB" => (1024, Base::Bytes),
        "MiB" => (1024 * 1024, Base::Bytes),
        "GiB" => (1024 * 1024 * 1024, Base::Bytes),
        "ms" => (1, Base::Milliseconds),
        "second" | "seconds" => (1_000, Base::Milliseconds),
        "minute" | "minutes" => (60 * 1_000, Base::Milliseconds),
        _ => return None,
    })
}

fn documented_limits(document_name: &str, heading: &str) -> Vec<(String, usize, Base)> {
    limits_from(&read_doc(document_name), document_name, heading)
}

/// Parses the `heading` limit list of `document`, failing on a bullet it cannot
/// read and on a claim repeated twice.
fn limits_from(document: &str, document_name: &str, heading: &str) -> Vec<(String, usize, Base)> {
    let bullets = bullet_list(document, heading);
    assert!(
        !bullets.is_empty(),
        "{document_name}: the {heading} list has no limit bullets"
    );
    let mut documented = Vec::new();
    for bullet in bullets {
        // A bullet that does not parse as a limit is a claim the guard cannot
        // read, not prose: fail it rather than silently dropping the line.
        let parsed = parse_bullet(bullet).unwrap_or_else(|| {
            panic!(
                "{document_name}: limit bullet {bullet:?} is not `<number><unit> <claim>`; \
                 write the number first so it stays machine-checkable"
            )
        });
        documented.push(parsed);
    }
    // A duplicated claim would let a second bullet silently shadow the first.
    let claims: BTreeSet<&str> = documented
        .iter()
        .map(|(_, _, claim)| claim.as_str())
        .collect();
    assert_eq!(
        claims.len(),
        documented.len(),
        "{document_name}: a limit claim appears twice in the {heading} list"
    );
    documented
        .into_iter()
        .map(|(value, base, claim)| (claim, value, base))
        .collect()
}

fn assert_documented_limits(document_name: &str, heading: &str) {
    let documented = documented_limits(document_name, heading);
    let expected: Vec<_> = LIMITS
        .iter()
        .filter(|limit| limit.document == document_name)
        .collect();
    assert!(
        !expected.is_empty(),
        "{document_name} is checked against limits::LIMITS but no limit names it"
    );

    for limit in &expected {
        let entry = documented
            .iter()
            .find(|(claim, ..)| claim == limit.claim)
            .unwrap_or_else(|| {
                panic!(
                    "{document_name}: nothing advertises the {:?} limit enforced by {}; documented claims are {:?}",
                    limit.claim,
                    limit.constant,
                    documented.iter().map(|(claim, ..)| claim).collect::<Vec<_>>()
                )
            });
        assert_eq!(
            entry.2, limit.base,
            "{document_name}: the {:?} limit is advertised in {:?} but {} counts {:?}",
            limit.claim, entry.2, limit.constant, limit.base
        );
        assert_eq!(
            entry.1, limit.value,
            "{document_name}: documents {:?} = {} but {} is {}",
            limit.claim, entry.1, limit.constant, limit.value
        );
    }

    // No extra bullet: deleting a constant must not leave a stale claim behind.
    let undocumented: Vec<_> = documented
        .iter()
        .filter(|(claim, ..)| !expected.iter().any(|limit| limit.claim == *claim))
        .map(|(claim, ..)| claim.as_str())
        .collect();
    assert!(
        undocumented.is_empty(),
        "{document_name}: documented but absent from limits::LIMITS: {undocumented:?}"
    );
}

/// Every number `docs/cli.md` advertises is the enforced constant.
#[test]
fn cli_document_limits_match_the_enforcing_constants() {
    assert_documented_limits(CLI_DOC, "Limits:");
}

/// Every parser limit `docs/parser-design.md` advertises is the constant.
#[test]
fn parser_document_limits_match_the_enforcing_constants() {
    assert_documented_limits(PARSER_DOC, "Limits:");
}

/// The table names the enforcing constant instead of copying its value.
#[test]
fn limit_table_names_both_documents_and_their_constants() {
    let documents: BTreeSet<_> = LIMITS.iter().map(|limit| limit.document).collect();
    assert_eq!(
        documents,
        BTreeSet::from([CLI_DOC, PARSER_DOC]),
        "the checked documents are fixed; add a new one to this list deliberately"
    );
    for limit in &LIMITS {
        assert!(
            limit.constant.contains("::"),
            "{:?} must name the module path of the constant that enforces it",
            limit.claim
        );
    }
    let claims: BTreeSet<_> = LIMITS
        .iter()
        .filter(|limit| limit.document == CLI_DOC)
        .map(|limit| limit.claim)
        .collect();
    assert_eq!(claims.len(), 10, "every cli.md limit has its own claim");
}

/// `docs/rules.md` and `REGISTRY` must name exactly the same rules.
#[test]
fn rule_table_matches_the_registry_in_both_directions() {
    let document = read_doc(RULES_DOC);
    let mut documented = Vec::new();
    for line in document.lines() {
        let Some(rest) = line.strip_prefix("| [") else {
            continue;
        };
        let id = rest
            .get(..5)
            .unwrap_or_else(|| panic!("{RULES_DOC}: malformed rule row {line:?}"));
        let target = rest
            .split_once("](")
            .unwrap_or_else(|| panic!("{RULES_DOC}: the {id} row links no rule document"))
            .1
            .split_once(')')
            .unwrap_or_else(|| panic!("{RULES_DOC}: the {id} row has an unterminated link"))
            .0;
        assert_eq!(
            target,
            format!("rules/{}.md", id.to_lowercase()),
            "{RULES_DOC}: the {id} row must link its own rule document"
        );
        documented.push(id.to_string());
    }

    let registered: Vec<String> = crate::rules::REGISTRY
        .iter()
        .map(|rule| rule.id.to_string())
        .collect();
    let documented_set: BTreeSet<_> = documented.iter().cloned().collect();
    let registered_set: BTreeSet<_> = registered.iter().cloned().collect();

    let undocumented: Vec<_> = registered_set.difference(&documented_set).collect();
    assert!(
        undocumented.is_empty(),
        "{RULES_DOC}: registered in REGISTRY but missing a table row: {undocumented:?}"
    );
    let unregistered: Vec<_> = documented_set.difference(&registered_set).collect();
    assert!(
        unregistered.is_empty(),
        "{RULES_DOC}: documented but absent from REGISTRY: {unregistered:?}"
    );
    assert_eq!(
        documented_set.len(),
        documented.len(),
        "{RULES_DOC}: a rule row appears twice"
    );
}

/// The row fields the document can contradict cheaply. A missing row is the
/// bidirectional test's finding, so this test only reads rows that exist.
#[test]
fn rule_table_default_severity_and_fixability_match_the_registry() {
    let document = read_doc(RULES_DOC);
    for rule in &crate::rules::REGISTRY {
        let Some(row) = document
            .lines()
            .find(|line| line.starts_with(&format!("| [{0} ", rule.id)))
        else {
            continue;
        };
        let cells: Vec<&str> = row.trim_matches('|').split('|').map(str::trim).collect();
        assert_eq!(
            cells.len(),
            4,
            "{RULES_DOC}: the {} row needs default, severity and safe fix columns: {row:?}",
            rule.id
        );
        let documented_default = if rule.selected_by_default {
            "On"
        } else {
            "Off"
        };
        assert_eq!(
            cells[1], documented_default,
            "{RULES_DOC}: {} documents default {:?} but REGISTRY selected_by_default is {}",
            rule.id, cells[1], rule.selected_by_default
        );
        assert_eq!(
            cells[2],
            rule.severity.as_str(),
            "{RULES_DOC}: {} documents severity {:?} but REGISTRY has {:?}",
            rule.id,
            cells[2],
            rule.severity.as_str()
        );
        if rule.fixable {
            assert_ne!(
                cells[3], "None",
                "{RULES_DOC}: {} is fixable in REGISTRY but documents no safe fix",
                rule.id
            );
        } else {
            assert_eq!(
                cells[3], "None",
                "{RULES_DOC}: {} is not fixable in REGISTRY but documents a safe fix",
                rule.id
            );
        }
    }
}

/// The rows of a two-column `| key | value |` table that starts at `heading`.
fn table_rows(document: &str, heading: &str) -> Vec<(String, String)> {
    let mut lines = document.lines().skip_while(|line| line.trim() != heading);
    assert!(lines.next().is_some(), "no {heading:?} section");
    let mut rows = Vec::new();
    for line in lines {
        if line.trim().is_empty() || line.starts_with("|-") || line.starts_with("| -") {
            continue;
        }
        let Some(rest) = line.strip_prefix("| ") else {
            break;
        };
        let Some((key, value)) = rest.split_once(" | ") else {
            break;
        };
        let value = value.strip_suffix(" |").unwrap_or(value).trim();
        if key == "Path" || key.starts_with("---") {
            continue;
        }
        rows.push((key.trim().to_string(), value.to_string()));
    }
    rows
}

/// `docs/syntax-snapshot.md`'s vendor hash table must restate the manifest.
#[test]
fn snapshot_vendor_hash_table_matches_the_manifest() {
    let document = read_doc(SNAPSHOT_DOC);
    let manifest: serde_json::Value =
        serde_json::from_str(&read_doc(HASH_MANIFEST)).expect("hashes.json is valid JSON");
    let manifest = manifest
        .as_object()
        .expect("hashes.json holds an object of path/hash pairs");
    let documented = table_rows(&document, "## Pinned vendor manifest");
    assert!(
        !documented.is_empty(),
        "{SNAPSHOT_DOC}: the pinned vendor manifest table is empty"
    );

    for (path, hash) in &documented {
        let expected = manifest
            .get(path)
            .unwrap_or_else(|| {
                panic!("{SNAPSHOT_DOC}: records {path} but {HASH_MANIFEST} has no such entry")
            })
            .as_str()
            .expect("a manifest entry is a string");
        assert_eq!(
            hash, expected,
            "{SNAPSHOT_DOC}: {path} records {hash} but {HASH_MANIFEST} records {expected}"
        );
    }

    let documented_paths: BTreeSet<&str> =
        documented.iter().map(|(path, _)| path.as_str()).collect();
    let missing: Vec<_> = manifest
        .keys()
        .filter(|path| !documented_paths.contains(path.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "{SNAPSHOT_DOC}: the hash table omits manifest entries {missing:?}; regenerate the row from {HASH_MANIFEST}"
    );
}

/// The recorded import revision and its file table stay well formed and pinned.
#[test]
fn snapshot_import_record_is_well_formed_and_pinned() {
    let document = read_doc(SNAPSHOT_DOC);
    let expected = format!("`{IMPORT_REVISION}`");
    assert!(
        document.contains(&expected),
        "{SNAPSHOT_DOC}: the recorded import revision must be {IMPORT_REVISION}"
    );

    let rows = table_rows(&document, "| Original path | SHA-256 at import |");
    let paths: Vec<&str> = rows.iter().map(|(path, _)| path.as_str()).collect();
    assert_eq!(
        paths, IMPORTED_PATHS,
        "{SNAPSHOT_DOC}: the import hash table must list exactly the imported files"
    );
    for (path, hash) in &rows {
        assert!(
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "{SNAPSHOT_DOC}: {path} records {hash:?}, which is not a SHA-256 digest"
        );
    }
}

/// The grammar revision lives once, in the vendor record the snapshot defers to.
#[test]
fn snapshot_defers_the_grammar_revision_to_the_vendor_record() {
    let snapshot = read_doc(SNAPSHOT_DOC);
    let upstream = read_doc(UPSTREAM_DOC);
    let pinned = revision_after(&upstream, "- Revision:")
        .unwrap_or_else(|| panic!("{UPSTREAM_DOC}: no \"- Revision: `...`\" line"));
    assert_eq!(
        pinned.len(),
        40,
        "{UPSTREAM_DOC}: a revision is a full 40-character commit, found {pinned:?}"
    );
    assert!(
        upstream.contains(&format!("`{pinned}`")),
        "{UPSTREAM_DOC}: the revision must stay in backticks so it is machine-readable"
    );
    for name in [SNAPSHOT_DOC, PARSER_DOC] {
        assert!(
            revision_after(&read_doc(name), "- Revision:").is_none(),
            "{name}: restates the grammar revision; {UPSTREAM_DOC} is its single record"
        );
    }
    assert!(
        snapshot.contains("vendor/tree-sitter-verse/upstream.md")
            && snapshot.contains("vendor/tree-sitter-verse/hashes.json"),
        "{SNAPSHOT_DOC}: provenance must keep pointing at the vendor record and manifest"
    );
}

/// The first 40-character hex commit after `marker`, if the marker is present.
fn revision_after(document: &str, marker: &str) -> Option<String> {
    commit_in(&document[document.find(marker)?..])
}

fn commit_in(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    // Every candidate boundary must be a char boundary: indexing a `&str` by
    // bytes alone panics on multibyte text.
    (0..=bytes.len().saturating_sub(40))
        .filter(|&index| text.is_char_boundary(index) && text.is_char_boundary(index + 40))
        .find_map(|index| {
            let candidate = &text[index..index + 40];
            let hex = candidate.bytes().all(|b| b.is_ascii_hexdigit());
            let bounded = !bytes
                .get(index.wrapping_sub(1))
                .is_some_and(u8::is_ascii_hexdigit)
                && !bytes.get(index + 40).is_some_and(u8::is_ascii_hexdigit);
            (hex && bounded).then(|| candidate.to_string())
        })
}

/// Every document this module reads exists where the tests read it.
#[test]
fn checked_documents_exist() {
    for name in [
        CLI_DOC,
        PARSER_DOC,
        RULES_DOC,
        SNAPSHOT_DOC,
        HASH_MANIFEST,
        UPSTREAM_DOC,
    ] {
        let path = root().join(name);
        assert!(
            Path::new(&path).is_file(),
            "{name} is checked by this module and must exist at {}",
            path.display()
        );
    }
}

mod unit {
    use super::*;

    #[test]
    fn bullets_are_read_in_prose_units() {
        assert_eq!(
            parse_bullet("8 MiB per source file"),
            Some((8 << 20, Base::Bytes, "per source file".into()))
        );
        assert_eq!(
            parse_bullet("256 KiB config file"),
            Some((256 << 10, Base::Bytes, "config file".into()))
        );
        assert_eq!(
            parse_bullet("4,096 bytes per exclude glob"),
            Some((4_096, Base::Bytes, "per exclude glob".into()))
        );
        assert_eq!(
            parse_bullet("10,000 source files"),
            Some((10_000, Base::Count, "source files".into()))
        );
        assert_eq!(
            parse_bullet("128 directory levels"),
            Some((128, Base::Count, "directory levels".into()))
        );
        assert_eq!(
            parse_bullet("2-second parse budget"),
            Some((2_000, Base::Milliseconds, "parse budget".into()))
        );
        assert_eq!(
            parse_bullet("1,000,000 fingerprint events"),
            Some((1_000_000, Base::Count, "fingerprint events".into()))
        );
        // Prose that is not a limit bullet parses to nothing.
        assert_eq!(parse_bullet("Each bullet is machine-checked above."), None);
    }

    #[test]
    fn a_list_ends_at_prose() {
        let document =
            "Limits:\n\n- 8 MiB per source file\n- 128 directory levels\n\nTrailing prose.\n";
        assert_eq!(bullet_list(document, "Limits:").len(), 2);
    }

    #[test]
    fn a_stale_value_and_unit_both_dissent() {
        let limit = LIMITS
            .iter()
            .find(|limit| limit.document == CLI_DOC && limit.claim == "per source file")
            .expect("the cli source-byte limit is tabled");
        let (value, base, claim) = parse_bullet("9 MiB per source file").unwrap();
        assert_eq!(claim, limit.claim);
        assert_eq!(base, limit.base);
        assert_ne!(value, limit.value, "a stale number must dissent");

        // The same number in the wrong unit family is still a mismatch.
        let (value, base, _) = parse_bullet("8388608 per source file").unwrap();
        assert_eq!(value, limit.value);
        assert_ne!(base, limit.base, "a count cannot stand in for a byte size");
    }

    #[test]
    fn commit_detection_needs_a_standalone_hex_run() {
        let text = "Revision: `6b5433e37b52c03c4c07f9468bf7cc11e45f2f82`";
        assert_eq!(
            revision_after(text, "Revision:").as_deref(),
            Some("6b5433e37b52c03c4c07f9468bf7cc11e45f2f82")
        );
        assert_eq!(commit_in("no commit here"), None);
        assert_eq!(commit_in(&"a".repeat(41)), None);
        // A commit at the very end is still found, and multibyte text on either
        // side must not panic the byte scanner.
        assert_eq!(
            commit_in(&format!("see {}", "b".repeat(40))).as_deref(),
            Some("b".repeat(40).as_str())
        );
        assert_eq!(
            commit_in(&format!("日本語{}", "c".repeat(40))).map(|c| c.len()),
            Some(40)
        );
        assert_eq!(commit_in(&"d".repeat(40)).map(|c| c.len()), Some(40));
        assert_eq!(
            commit_in(&format!("{}\u{3000}", "e".repeat(40))).map(|c| c.len()),
            Some(40)
        );
    }

    #[test]
    #[should_panic(expected = "is not `<number><unit> <claim>`")]
    fn an_unreadable_bullet_fails_instead_of_looking_like_prose() {
        limits_from(
            "Limits:\n\n- 8 MiB per source file\n- eight MiB per source file\n",
            CLI_DOC,
            "Limits:",
        );
    }

    #[test]
    #[should_panic(expected = "a limit claim appears twice")]
    fn a_duplicated_claim_cannot_shadow_an_earlier_bullet() {
        limits_from(
            "Limits:\n\n- 8 MiB per source file\n- 9 MiB per source file\n",
            CLI_DOC,
            "Limits:",
        );
    }
}
