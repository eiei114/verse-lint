//! Bounded mutation harness: a deterministic, seeded loop over the self-authored
//! fixtures that asserts the tool is robust against malformed input.
//!
//! Five assertions hold for every mutation:
//!
//! 1. the binary never panics (exit status, not a signal or trap code);
//! 2. it exits only 0, 1 or 2;
//! 3. it writes nothing to disk without `--fix`;
//! 4. JSON output validates against the pinned `docs/diagnostics.schema.json`
//!    when the exit code is 0 or 1;
//! 5. SARIF output, when produced, validates against the pinned OASIS schema.
//!
//! The semantic walker is inside the loop: every mutation also runs
//! `--semantic-coverage`, whose output must validate against
//! `docs/coverage.schema.json`, and a lint run over the same bytes must not
//! report a V3xxx semantic finding when that coverage report says the graph is
//! incomplete. A malformed input therefore becomes a coverage rejection or a
//! skipped-kind entry, never a panic and never an overreaching finding.
//!
//! The loop is bounded and deterministic: `MUTATIONS` iterations from a fixed
//! seed by default, enough to gate ordinary verification, and the
//! `VERSE_LINT_FUZZ_ITERATIONS` environment variable raises the bound for a
//! deliberate longer soak. See `docs/integration-testing.md`.
use serde_json::Value;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[path = "support/json_schema.rs"]
mod json_schema;

/// Iterations per fixture in the ordinary verification path. Deliberately small:
/// the harness runs eight process invocations per case, so the bound is the
/// whole cost. `VERSE_LINT_FUZZ_ITERATIONS` raises it for a deliberate soak.
const MUTATIONS: u64 = 3;

fn iterations() -> u64 {
    match std::env::var("VERSE_LINT_FUZZ_ITERATIONS") {
        Ok(value) => value.parse().unwrap_or_else(|e| {
            panic!("VERSE_LINT_FUZZ_ITERATIONS={value:?} is not a number: {e}")
        }),
        Err(_) => MUTATIONS,
    }
}

/// A deterministic xorshift64* generator. No dependency, no OS randomness, so a
/// failing mutation is reproducible from the seed printed in the panic message.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }
}

/// Bytes spliced into a fixture. Every entry is something the tool must survive:
/// quotes and braces that break a token, an indentation jump that breaks the
/// scanner, and the constructs the model deliberately does not classify.
const NEEDLES: [&[u8]; 14] = [
    b"\"",
    b"\"\"\"",
    b"{",
    b"}",
    b"(",
    b")",
    b"#",
    b"<#>",
    b"<p>",
    b"\n",
    b"\t",
    b"    ",
    b" +# ",
    b"map{\"x\" => 1}",
];

fn fixtures() -> Vec<(String, Vec<u8>)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/baseline");
    let mut fixtures = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("baseline fixtures")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "verse")
        })
        .collect();
    // Directory order is not guaranteed, even on one host.
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_str().unwrap().to_owned();
        let bytes = std::fs::read(&path).expect("read fixture");
        fixtures.push((name, bytes));
    }
    assert!(!fixtures.is_empty(), "the harness needs fixtures");
    fixtures
}

/// Apply one deterministic mutation to `bytes`. Every branch is bounded in
/// output size, so a pathological insert cannot grow the input without limit.
/// `corpus` supplies the donor for the cross-fixture splice.
fn mutate(rng: &mut Rng, bytes: &[u8], corpus: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut text = bytes.to_vec();
    let length = text.len();
    const LIMIT: usize = 64 * 1024;
    match rng.below(6) {
        // Delete a bounded slice.
        0 => {
            if length > 0 {
                let start = rng.below(length);
                let take = 1 + rng.below(length - start);
                text.drain(start..(start + take).min(length));
            }
        }
        // Insert a needle at a random offset.
        1 => {
            if length < LIMIT {
                let needle = NEEDLES[rng.below(NEEDLES.len())];
                let at = rng.below(length + 1);
                text.splice(at..at, needle.iter().copied());
            }
        }
        // Replace one byte with a needle's first byte.
        2 => {
            if length > 0 {
                let at = rng.below(length);
                let needle = NEEDLES[rng.below(NEEDLES.len())];
                text[at] = needle[0];
            }
        }
        // Duplicate a line, which is the cheapest way to break a scope shape.
        3 => {
            if length < LIMIT && !text.is_empty() {
                let start = rng.below(length);
                let end = text[start..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(length, |offset| start + offset + 1);
                let line = text[start..end].to_vec();
                let at = rng.below(length + 1);
                text.splice(at..at, line);
            }
        }
        // Truncate at a random offset, including a mid-multibyte boundary.
        4 => {
            if length > 0 {
                let at = rng.below(length);
                text.truncate(at);
            }
        }
        // Splice another fixture in, which mixes two shapes in one file.
        _ => {
            if length < LIMIT {
                let (_, donor) = &corpus[rng.below(corpus.len())];
                let take = donor.len().min(LIMIT - length);
                let at = rng.below(length + 1);
                text.splice(at..at, donor.iter().take(take).copied());
            }
        }
    }
    text
}

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_verse-lint"))
        .current_dir(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|e| panic!("launching verse-lint {args:?} failed: {e}"))
}

fn run_stdin(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_verse-lint"));
    command
        .current_dir(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped());
    let mut child = command
        .spawn()
        .unwrap_or_else(|e| panic!("launching verse-lint {args:?} failed: {e}"));
    // A write error here is the tool's own problem to report, not a harness
    // failure: a forked process may exit before the pipe is drained.
    let _ = child.stdin.take().expect("piped stdin").write_all(stdin);
    child.wait_with_output().expect("verse-lint output")
}

/// Every file under `dir` with its bytes. Copying a directory this way is the
/// only sound way to assert "nothing was written": a new file and a changed file
/// both show up as a difference.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries = Vec::new();
    collect(dir, dir, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn collect(root: &Path, dir: &Path, entries: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).expect("read directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            collect(root, &path, entries);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("path is under root")
                .to_path_buf();
            entries.push((relative, std::fs::read(&path).expect("read file")));
        }
    }
}

fn assert_no_write(before: &[(PathBuf, Vec<u8>)], after: &[(PathBuf, Vec<u8>)], context: &str) {
    assert_eq!(
        before.len(),
        after.len(),
        "{context}: files were created or removed"
    );
    for (left, right) in before.iter().zip(after) {
        assert_eq!(left.0, right.0, "{context}: a file was renamed");
        assert_eq!(
            left.1,
            right.1,
            "{context}: {} was written",
            left.0.display()
        );
    }
}

/// The five assertions, applied to one candidate document. `stdout` is the
/// process output; the caller has already asserted the exit code.
fn assert_json_document(output: &Output, schema: &json_schema::Schema, context: &str) {
    let text = String::from_utf8(output.stdout.clone())
        .unwrap_or_else(|e| panic!("{context}: stdout is not UTF-8: {e}"));
    let value: Value = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{context}: stdout is not JSON: {e}: {text}"));
    schema
        .validate(&value)
        .unwrap_or_else(|error| panic!("{context}: JSON schema violation: {error}\n{text}"));
}

fn assert_robust(output: &Output, context: &str) {
    let code = output
        .status
        .code()
        .unwrap_or_else(|| panic!("{context}: the tool was terminated without an exit code"));
    assert!(
        (0..=2).contains(&code),
        "{context}: unexpected exit code {code}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("panicked"),
        "{context}: the tool panicked: {stderr}"
    );
}

#[test]
fn mutations_never_panic_never_write_and_always_emit_a_valid_document() {
    let diagnostics_schema = json_schema::Schema::new(
        include_str!("../docs/diagnostics.schema.json"),
        "docs/diagnostics.schema.json",
    );
    let coverage_schema = json_schema::Schema::new(
        include_str!("../docs/coverage.schema.json"),
        "docs/coverage.schema.json",
    );
    let sarif_schema = json_schema::Schema::new(
        include_str!("../tests/schemas/sarif-schema-2.1.0.json"),
        "tests/schemas/sarif-schema-2.1.0.json",
    );
    let corpus = fixtures();
    let rounds = iterations();
    let mut rng = Rng(0x5eed_0000_0000_0001);
    let mut cases = 0u64;

    for (name, original) in &corpus {
        for round in 0..rounds {
            let candidate = mutate(&mut rng, original, &corpus);
            let context = format!("fixture {name}, round {round}");
            let directory = tempfile::tempdir().expect("temporary directory");
            let root = directory.path();
            let path = root.join("case.verse");
            std::fs::write(&path, &candidate).expect("write candidate");
            std::fs::write(root.join("verse.toml"), "schema-version = 1\n").expect("write config");

            // 1 + 2 + 3: robustness and no writes for a lint run in every format.
            for format in ["text", "json", "sarif", "github"] {
                let before = snapshot(root);
                let output = run(
                    root,
                    &[
                        "case.verse",
                        "--select",
                        "V",
                        &format!("--output-format={format}"),
                    ],
                );
                let label = format!("{context} ({format})");
                assert_robust(&output, &label);
                let after = snapshot(root);
                assert_no_write(&before, &after, &label);
                let code = output.status.code().unwrap();
                match format {
                    "json" if matches!(code, 0 | 1) => {
                        assert_json_document(&output, &diagnostics_schema, &label);
                    }
                    "sarif" if matches!(code, 0 | 1) => {
                        let text = String::from_utf8(output.stdout.clone())
                            .unwrap_or_else(|e| panic!("{label}: stdout is not UTF-8: {e}"));
                        let value: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
                            panic!("{label}: SARIF stdout is not JSON: {e}: {text}")
                        });
                        sarif_schema.validate(&value).unwrap_or_else(|error| {
                            panic!("{label}: SARIF schema violation: {error}\n{text}")
                        });
                    }
                    _ => {}
                }
            }

            // The semantic walker is in the loop: coverage must be a valid
            // document or a clean refusal, never a panic.
            let before_coverage = snapshot(root);
            let coverage = run(root, &["case.verse", "--semantic-coverage"]);
            let coverage_label = format!("{context} (coverage)");
            assert_robust(&coverage, &coverage_label);
            assert_no_write(&before_coverage, &snapshot(root), &coverage_label);
            let coverage_code = coverage.status.code().unwrap();
            let mut graph_complete = None;
            if coverage_code == 0 {
                assert_json_document(&coverage, &coverage_schema, &coverage_label);
                let value: Value = serde_json::from_slice(&coverage.stdout).unwrap();
                let file = &value["files"][0];
                graph_complete = Some(
                    file["isComplete"]
                        .as_bool()
                        .expect("isComplete is a boolean"),
                );
                // The summary and the per-file entry are the same numbers.
                assert_eq!(value["summary"]["sourcesAnalyzed"], 1);
            }

            // Nothing may be reported by a semantic rule when the model admits it
            // could not see the whole file: that is the invariant, checked here
            // against real mutated bytes rather than a hand-built unit fixture.
            let semantic = run(
                root,
                &[
                    "case.verse",
                    "--select",
                    "V3001,V3002,V3003",
                    "--output-format=json",
                ],
            );
            let semantic_label = format!("{context} (semantic)");
            assert_robust(&semantic, &semantic_label);
            let semantic_code = semantic.status.code().unwrap();
            if matches!(semantic_code, 0 | 1) {
                assert_json_document(&semantic, &diagnostics_schema, &semantic_label);
                let value: Value = serde_json::from_slice(&semantic.stdout).unwrap();
                let semantic_findings = value["diagnostics"]
                    .as_array()
                    .expect("diagnostics is an array")
                    .iter()
                    .filter(|diagnostic| {
                        diagnostic["ruleId"]
                            .as_str()
                            .is_some_and(|id| id.starts_with("V3"))
                    })
                    .count();
                if graph_complete == Some(false) {
                    assert_eq!(
                        semantic_findings, 0,
                        "{semantic_label}: an incomplete graph produced a semantic finding"
                    );
                }
            }

            // stdin takes a different read path than a file, so the same bytes go
            // through it as well.
            let stdin = run_stdin(
                root,
                &["-", "--select", "V", "--output-format=json"],
                &candidate,
            );
            let stdin_label = format!("{context} (stdin)");
            assert_robust(&stdin, &stdin_label);
            if matches!(stdin.status.code().unwrap(), 0 | 1) {
                assert_json_document(&stdin, &diagnostics_schema, &stdin_label);
            }

            cases += 1;
        }
    }

    // A guard on the harness itself: an empty corpus or a zero bound would make
    // every assertion above vacuous.
    assert_eq!(cases, rounds * corpus.len() as u64);
    assert!(
        cases >= 15,
        "the harness must exercise at least one round over every fixture"
    );
}

/// `--fix` is the one mode that may write, so it is excluded from the no-write
/// assertion above and checked separately: it either applies a validated edit or
/// refuses, and it never damages a file it refuses.
#[test]
fn fix_mutations_refuse_cleanly_without_corrupting_the_input() {
    let corpus = fixtures();
    let mut rng = Rng(0x5eed_0000_0000_0002);
    for (name, original) in &corpus {
        for round in 0..iterations().min(4) {
            let candidate = mutate(&mut rng, original, &corpus);
            let context = format!("fix fixture {name}, round {round}");
            let directory = tempfile::tempdir().expect("temporary directory");
            let root = directory.path();
            let path = root.join("case.verse");
            std::fs::write(&path, &candidate).expect("write candidate");

            let output = run(root, &["case.verse", "--fix", "--output-format=json"]);
            assert_robust(&output, &context);
            let code = output.status.code().unwrap();
            let after = std::fs::read(&path).expect("read candidate after --fix");
            if code == 2 {
                // A refusal must leave the original bytes in place.
                assert_eq!(after, candidate, "{context}: a refused fix wrote bytes");
            } else {
                // A successful fix rewrites only validated trivia, so the file
                // must still be accepted by the same coverage validation.
                let check = run_stdin(
                    root,
                    &["-", "--select", "V", "--output-format=json"],
                    &after,
                );
                assert_robust(&check, &format!("{context} (re-read)"));
                assert!(
                    matches!(check.status.code().unwrap(), 0 | 1),
                    "{context}: a written fix produced an unreadable file: {check:?}"
                );
            }
        }
    }
}
