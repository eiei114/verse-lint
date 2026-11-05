//! Guards the crate metadata and install documentation a publish depends on.
//!
//! `docs/release-checklist.md` lists what a human does at a release boundary.
//! This module keeps the parts a test can check from silently rotting in
//! between: the README must not lie about what is installable, its links must
//! survive crates.io (which does not serve `docs/`), `[package] exclude` must
//! not drop a file the build or a test reads, and no workflow may publish.
//!
//! It never publishes, tags or authenticates: those stay human actions.

use std::{
    fs,
    path::{Path, PathBuf},
};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(name: &str) -> String {
    let path = root().join(name);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {name} ({}): {e}", path.display()))
}

/// Markdown inline link and image targets in `document`, in order.
fn link_targets(document: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut rest = document;
    while let Some(start) = rest.find("](") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find(')') else { break };
        targets.push(&rest[..end]);
        rest = &rest[end + 1..];
    }
    targets
}

/// Removes fenced code blocks and inline code spans, whose brackets are literal
/// text a reader sees, not links.
fn prose_only(document: &str) -> String {
    let mut prose = String::with_capacity(document.len());
    let mut fenced = false;
    for line in document.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            prose.push('\n');
            continue;
        }
        if fenced {
            prose.push('\n');
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find('`') {
            prose.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            match after.find('`') {
                Some(end) => rest = &after[end + 1..],
                None => {
                    break;
                }
            }
        }
        prose.push_str(rest);
        prose.push('\n');
    }
    prose
}

/// Reference-style link labels the body uses, e.g. `[cli]` or `[safe fixes][cli]`.
///
/// A label that resolves to no definition renders as literal text, which is a
/// broken link crates.io shows to a reader, so the definitions are checked too.
fn reference_labels(document: &str) -> Vec<String> {
    let prose = prose_only(document);
    let mut labels = Vec::new();
    let mut rest = prose.as_str();
    while let Some(start) = rest.find('[') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find(']') else { break };
        let label = &rest[..end];
        let after = &rest[end + 1..];
        // `[text][label]` names the definition explicitly. A bare `[label]` is a
        // shortcut reference only when it looks like a label; `[x](url)` is an
        // inline link and `[x]: url` is the definition itself.
        let target = match after.strip_prefix('[') {
            Some(rest) => rest.split_once(']').map(|(label, _)| label.to_string()),
            None => {
                let shortcut = !after.starts_with('(')
                    && !after.starts_with(':')
                    && !label.is_empty()
                    && label
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
                    && label.chars().any(|c| c.is_ascii_alphabetic());
                shortcut.then(|| label.to_string())
            }
        };
        if let Some(target) = target.filter(|target| !target.is_empty()) {
            labels.push(target);
        }
    }
    labels
}

/// `[label]: target` definitions, keyed by lowercased label.
fn reference_definitions(document: &str) -> Vec<(String, String)> {
    document
        .lines()
        .filter_map(|line| {
            let (label, target) = line.strip_prefix('[')?.split_once("]: ")?;
            Some((label.to_ascii_lowercase(), target.trim().to_string()))
        })
        .collect()
}

/// crates.io renders `README.md` alone: a relative target resolves nowhere.
#[test]
fn readme_links_are_absolute_so_crates_io_can_serve_them() {
    let readme = read("README.md");
    let definitions = reference_definitions(&readme);
    let relative: Vec<_> = link_targets(&readme)
        .into_iter()
        .chain(definitions.iter().map(|(_, target)| target.as_str()))
        .filter(|target| {
            let target = target.trim();
            !target.starts_with("http://")
                && !target.starts_with("https://")
                && !target.starts_with("mailto:")
                && !target.starts_with('#')
        })
        .collect();
    assert!(
        relative.is_empty(),
        "README.md is rendered on crates.io, which does not serve this repository's docs/ \
         directory, so every relative link is broken there. Use an absolute \
         https://github.com/eiei114/verse-lint/blob/main/... URL instead: {relative:?}"
    );
}

/// Every reference-style usage must have a definition, or it renders as text.
#[test]
fn readme_reference_links_all_resolve() {
    let readme = read("README.md");
    let defined: Vec<String> = reference_definitions(&readme)
        .into_iter()
        .map(|(label, _)| label)
        .collect();
    let mut missing: Vec<String> = reference_labels(&readme)
        .into_iter()
        .filter(|label| {
            !defined
                .iter()
                .any(|defined| defined == &label.to_ascii_lowercase())
        })
        .collect();
    missing.sort_unstable();
    missing.dedup();
    assert!(
        missing.is_empty(),
        "README.md uses reference-style link labels with no definition, which renders as \
         literal text on crates.io: {missing:?}. Add `[label]: https://...` to the \
         definition block at the end of the file."
    );
}

/// The three documented install paths, each labelled with what is really true.
#[test]
fn readme_installation_names_every_path_and_its_availability() {
    let readme = read("README.md");
    let section = readme
        .split_once("\n## Installation\n")
        .unwrap_or_else(|| panic!("README.md must have an `## Installation` section"))
        .1;
    let section = section
        .split_once("\n## ")
        .map(|(body, _)| body)
        .unwrap_or(section);

    for (needle, why) in [
        ("mise", "the intended prebuilt Windows path must be named"),
        (
            "not yet available",
            "the mise path is unverified, so it must say it is not yet available",
        ),
        (
            "cargo install verse-lint",
            "the cargo install path must be named even though it cannot work yet",
        ),
        (
            "not yet published",
            "no crate is published, so the cargo install path must say so",
        ),
        (
            "available today",
            "building from source is the only working path and must say so",
        ),
    ] {
        assert!(
            section.contains(needle),
            "README.md `## Installation` must contain {needle:?}: {why}"
        );
    }

    // The section never claims a published artifact exists.
    for claim in ["is published on crates.io", "install from crates.io"] {
        assert!(
            !readme.contains(claim),
            "README.md must not claim a published artifact: found {claim:?}"
        );
    }
}

/// `CHANGELOG.md` exists and records only what is already on `main`.
#[test]
fn changelog_opens_with_an_unreleased_section() {
    let changelog = read("CHANGELOG.md");
    assert!(
        changelog.starts_with("# Changelog"),
        "CHANGELOG.md must open with a `# Changelog` heading"
    );
    assert!(
        changelog.contains("\n## Unreleased\n"),
        "CHANGELOG.md must keep an `## Unreleased` section; work that has not merged \
         does not belong in it"
    );
    assert!(
        changelog.contains("not established") || changelog.contains("not** established"),
        "CHANGELOG.md must state what is not established, so a release page cannot be \
         read as a stability claim"
    );
}

/// The checklist exists and keeps the human-owned steps, including the CI rule.
#[test]
fn release_checklist_keeps_the_human_owned_steps() {
    let checklist = read("docs/release-checklist.md");
    for needle in [
        "cargo publish --dry-run",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "CHANGELOG.md",
        "v<VERSION>",
        "CI must never publish",
        "license-file",
        "docs.rs",
    ] {
        assert!(
            checklist.contains(needle),
            "docs/release-checklist.md must keep {needle:?}: it is a prerequisite a \
             future publish would otherwise rediscover"
        );
    }
    assert!(
        !checklist.contains("run `cargo publish`"),
        "the checklist must not instruct a reader to publish; publishing is a \
         separate human approval"
    );
}

/// The manifest stays publishable: a licence pair without `license-file`.
#[test]
fn manifest_licence_declaration_matches_the_shipped_texts() {
    let manifest: toml::Value = toml::from_str(&read("Cargo.toml")).expect("Cargo.toml parses");
    let package = manifest
        .get("package")
        .expect("Cargo.toml has a [package] table")
        .as_table()
        .expect("[package] is a table");

    assert_eq!(
        package.get("license").and_then(|value| value.as_str()),
        Some("MIT OR Apache-2.0"),
        "the declared licence must name both shipped texts"
    );
    assert!(
        !package.contains_key("license-file"),
        "declaring `license-file` alongside `license` is a crates.io error; the two \
         licence texts ship as LICENSE-MIT and LICENSE-APACHE"
    );
    for name in ["LICENSE-MIT", "LICENSE-APACHE", "NOTICE", "README.md"] {
        assert!(
            root().join(name).is_file(),
            "{name} is declared or referenced by the package and must exist"
        );
    }
    assert_eq!(
        package.get("readme").and_then(|value| value.as_str()),
        Some("README.md"),
        "the package readme path must resolve"
    );
}

/// `[package] exclude` must never drop a file the build or a test reads.
#[test]
fn package_exclude_keeps_every_required_file() {
    let manifest: toml::Value = toml::from_str(&read("Cargo.toml")).expect("Cargo.toml parses");
    let excluded: Vec<String> = manifest["package"]
        .get("exclude")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    entry
                        .as_str()
                        .expect("each exclude entry is a string")
                        .trim_start_matches('/')
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();

    // Files the build compiles, the crate reads at test time, and licence texts a
    // published crate must carry.
    let required = [
        "build.rs",
        "README.md",
        "CHANGELOG.md",
        "NOTICE",
        "LICENSE-MIT",
        "LICENSE-APACHE",
        "docs/cli.md",
        "docs/parser-design.md",
        "docs/rules.md",
        "docs/release-checklist.md",
        "docs/rules/v1001.md",
        "docs/rules/v2001.md",
        "vendor/tree-sitter-verse/LICENSE",
        "vendor/tree-sitter-verse/LICENSE-tree-sitter",
        "vendor/tree-sitter-verse/hashes.json",
        "vendor/tree-sitter-verse/upstream.md",
        "vendor/tree-sitter-verse/src/parser.c",
        "vendor/tree-sitter-verse/src/scanner.c",
        "vendor/tree-sitter-verse/src/tree_sitter/alloc.h",
        "vendor/tree-sitter-verse/src/tree_sitter/array.h",
        "vendor/tree-sitter-verse/src/tree_sitter/parser.h",
        "tests/fixtures/device.input.verse",
        "tests/schemas/sarif-schema-2.1.0.json",
        ".github/workflows/ci.yml",
    ];
    for path in required {
        assert!(
            Path::new(&root().join(path)).exists(),
            "{path} is required by the build, a test or a published crate and must exist"
        );
        for entry in &excluded {
            let dropped = if entry.ends_with('/') || Path::new(&root().join(entry)).is_dir() {
                path.starts_with(entry.trim_end_matches('/'))
            } else {
                path == entry
            };
            assert!(
                !dropped,
                "`[package] exclude` entry {entry:?} removes {path}, which the build, a \
                 test or a published crate needs. Check the file list with \
                 `cargo package --list` before changing an exclude entry."
            );
        }
    }

    // Generated grammar inputs and developer-only tooling are the intended
    // exclusions; anything else needs a deliberate decision and a checklist edit.
    for entry in &excluded {
        assert!(
            entry.starts_with("scripts")
                || entry.starts_with("vendor/tree-sitter-verse/grammar.js")
                || entry.starts_with("vendor/tree-sitter-verse/tree-sitter.json")
                || entry.starts_with("vendor/tree-sitter-verse/src/grammar.json")
                || entry.starts_with("vendor/tree-sitter-verse/src/node-types.json")
                || entry.starts_with("tests/test_package.py"),
            "unexpected `[package] exclude` entry {entry:?}: docs/release-checklist.md \
             states which files may be excluded and why"
        );
    }
}

/// CI verifies; it never publishes, tags or releases.
#[test]
fn no_workflow_publishes_or_releases() {
    let dir = root().join(".github/workflows");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("the workflows directory exists") {
        let path = entry.expect("readable directory entry").path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
            continue;
        }
        checked += 1;
        let contents = fs::read_to_string(&path).expect("workflow is readable UTF-8");
        for prohibited in [
            "cargo publish",
            "cargo package",
            "git tag",
            "gh release",
            "action-gh-release",
            "create-release",
            "CARGO_REGISTRY_TOKEN",
            "secrets.",
        ] {
            assert!(
                !contents.contains(prohibited),
                "{}: this repository never publishes, tags or releases from a workflow, \
                 and never handles a registry credential. See docs/release-checklist.md \
                 and the ADR for the SARIF parallel. Remove {prohibited:?} rather than \
                 weakening this guard.",
                path.display()
            );
        }
    }
    assert!(
        checked > 0,
        "no workflow files found under {}; this guard would silently pass",
        dir.display()
    );
}
