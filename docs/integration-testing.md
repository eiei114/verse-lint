# Paired and shell integration checks

The linter repository stays independent of `verse-fmt`: it has no formatter
crate, binary, or runtime dependency. This developer-only script accepts a
formatter release binary and the formatter's self-authored golden corpus to
verify the tools' on-disk convergence contract.

## Declared parity vs. verified convergence

The parity contract has two layers, and only one of them is checkable without the
formatter.

- `parity.json` at the repository root is the **declaration**: the expected
  formatter corpus SHA-256, the expected golden-case count, the formatter grammar
  base revision the snapshot was mirrored from, and the formatter/linter
  revisions of the last real paired run.
- `scripts/verify-parity.ps1` validates the declaration only. It checks that the
  file exists, parses, has exactly the documented keys with well-formed values,
  and that `lastVerifiedLinterRevision` is `HEAD` or an ancestor of it. It needs
  no formatter binary, no formatter checkout and no network, and `verify.ps1`
  runs it, so a malformed or stale declaration fails the ordinary verification
  path.
- `scripts/verify-pair.py` is the **verification**: it reads its expected corpus
  hash and golden-case count from `parity.json`, then runs both real binaries
  over the whole corpus and asserts byte-level convergence. It is the only thing
  that can claim convergence. `--update-pin` refreshes the declaration only
  after every golden passes; `--dry-run` reports the candidate without writing
  any files.

`verify.ps1` passing means the declaration is structurally sound and its
recorded linter revision is still reachable. It does **not** mean the corpus or
the formatter still agree with the linter, and it never fails merely because the
formatter moved on after the last paired run. Between paired updates that drift
is expected; the checklist in [CONTRIBUTING](../CONTRIBUTING.md) is what keeps it
from going unnoticed.

A missing, unparseable or partially populated `parity.json` is a hard failure in
both scripts. Neither falls back to built-in constants, so the pin cannot quietly
disappear.

## Resource-limit boundary tests

`tests/limits-cli.rs` covers every limit advertised in [cli.md](cli.md) and
[parser-design.md](parser-design.md) at the boundary and one step past it. Two
cases need a heavy input (a 64 MiB aggregate read and a one-million-event CST)
and are skipped by default so the ordinary verification path stays fast. Enable
them explicitly:

```powershell
$env:VERSE_LINT_HEAVY_LIMIT_TESTS = '1'
cargo test --locked --test limits-cli
```

Without the variable, those two tests print a skip notice and assert nothing.
Both limits are still proven in the default path by their unit tests over the
shared predicates (`files::total_bytes_exceeded`, `syntax::tree_limit`), so a
regression in the boundary comparison still fails `scripts/verify.ps1`.

The two-second parse budget is not tested by making a real parse exceed it: that
would flake on a loaded runner. `syntax::tests` asserts the budget predicate on
both sides of the bound, asserts the documented duration constant, and proves
that a zero budget refuses a real parse.

## Mutation harness (fuzz hardening)

`tests/fuzz-cli.rs` runs a bounded, deterministic mutation loop over the
self-authored fixtures in `tests/fixtures/baseline/`. It needs no new dependency
and no external corpus: a seeded xorshift generator applies six mutation shapes
(delete, insert a malformed needle, corrupt one byte, duplicate a line, truncate
— including mid-character — and splice another fixture in) and every case is
reproducible from the fixed seed.

Each mutation asserts all five of these:

1. the tool never panics (the exit status is an exit code, and stderr never says
   `panicked`);
2. it exits only 0, 1 or 2 — never another code, never a crash;
3. nothing on disk changes without `--fix` (a full before/after snapshot of the
   temporary project, covering text, JSON, SARIF, GitHub and coverage runs);
4. JSON output validates against `docs/diagnostics.schema.json` when the exit
   code is 0 or 1;
5. SARIF output validates against the pinned
   `tests/schemas/sarif-schema-2.1.0.json` when it is produced.

The semantic walker is inside the loop. Every mutation also runs
`--semantic-coverage`, whose document must validate against
`docs/coverage.schema.json`, and the same bytes are then linted with
`--select V3001,V3002,V3003`: when the coverage report says the graph is
incomplete, that lint run must report zero `V3xxx` findings. A malformed input
therefore becomes a coverage rejection or a skipped-kind entry, never a panic and
never an overreaching finding.

`--fix` is the one mode that may write, so it is checked separately by
`fix_mutations_refuse_cleanly_without_corrupting_the_input`: an exit 2 leaves the
original bytes untouched, and a write the tool does perform must still pass the
same coverage validation when re-read.

The loop is bounded so it fits the ordinary verification path: three mutations
per fixture, which is one full round over the corpus and keeps the whole test
file well under a minute on a debug build. For a deliberate longer soak, raise the
bound without editing the test:

```powershell
$env:VERSE_LINT_FUZZ_ITERATIONS = '100'
cargo test --locked --test fuzz-cli
```

The two pinned schemas are validated by `tests/support/json_schema.rs`, a small
in-repo validator for exactly the keyword subset those documents use. It is
shared by `tests/fuzz-cli.rs` and `tests/semantic-coverage-cli.rs` via `#[path]`,
adds no dependency, and fails loudly on a schema keyword or pattern it cannot
read rather than skipping the constraint. `format` is not enforced there:
`scripts/verify-json.ps1` remains the authoritative check of real release-binary
JSON with PowerShell's schema validator.

The harness weakens no existing limit, guard or rejection: it drives the real
binary through the same CLI paths, so a mutation that trips a limit still exits 2
with an execution error, and the assertions are about the outcome contract rather
than about bypassing a check.

## Documentation drift guard

A limit that changes in code while the prose keeps the old number is caught by
`src/doc-drift-tests.rs`, which runs as part of `cargo test` and therefore under
`scripts/verify.ps1` with no extra step. It asserts that:

- every limit bullet in the `Limits:` list of [cli.md](cli.md) and
  [parser-design.md](parser-design.md) equals its constant in
  `src/limits.rs`, compared in the documented unit (`8 MiB` is checked as
  8 × 1024 × 1024 bytes) and with no bullet missing in either direction;
- [rules.md](rules.md) covers exactly the `REGISTRY` rules in both directions,
  and each row's default, severity and safe-fix wording matches its entry;
- the vendor hash table in [syntax-snapshot.md](syntax-snapshot.md) restates
  `vendor/tree-sitter-verse/hashes.json` exactly, and the recorded import
  revision and file list stay as pinned.

A failure names the document, the claim and both values. The guard is a
consistency assertion, not a generator: edit the document and the constant
together, and never delete a claim to silence it.

## Paired fix/format convergence

The paired harness is a deliberate developer operation; CI does not check out
the formatter or run this script. It locates the formatter checkout from the
tracked corpus path, so the release binary may live under `target/release` or
an external `CARGO_TARGET_DIR`.

For each golden, it runs `verse-lint --fix` followed by `verse-fmt --write`,
checks the formatter's independent golden bytes, and repeats the same pair to
prove the second cycle changes nothing. This **second-cycle byte-invariance
assertion remains required**. The reverse order (formatter first) is not run,
and the 23 explicit formatter refusals are not exercised by this successful
paired-fix path.

### Update procedure

Use one sequence; do not hand-edit the pin:

1. Update the linter snapshot and corpus-driven regressions, and build both
   release binaries.
2. Ensure **both** linter and formatter checkouts are clean. The harness refuses
   either dirty checkout; this requirement cannot be bypassed.
3. From the linter root, run the complete harness with `--update-pin`. Choose
   an output path under `docs/evidence/` so the full report is ready to commit:

   ```powershell
   python scripts/verify-pair.py `
     --formatter ..\verse-fmt\target\x86_64-pc-windows-msvc\release\verse-fmt.exe `
     --corpus ..\verse-fmt\tests\fixtures\corpus.json `
     --output docs\evidence\parity-paired-YYYY-MM-DD-full.json `
     --update-pin
   ```

   Replace `YYYY-MM-DD` with the run date. The script updates `parity.json`
   only after every corpus golden passed and records the corpus hash, golden
   count, and both observed revisions. `formatterGrammarBaseRevision` remains
   the declared snapshot-origin fact. Add `--dry-run` to preview the changes;
   that still runs every case but writes neither the pin nor the report.
4. Commit the updated `parity.json` and the report produced by `--output`
   together with the paired corpus/snapshot change. Record both full revisions,
   the corpus hash, and the case counts in the pull request.

Without `--update-pin`, a corpus hash or golden-count mismatch against
`parity.json` still fails the run. The updater refuses dirty checkouts, failed
cases, incomplete or skipped results, and a mismatch between observed goldens
and the corpus's declared golden set. Each refusal names the reason and leaves
`parity.json` untouched. A pinned hash never replaces running the harness.

The report behind the current pin is committed as
[`docs/evidence/parity-paired-2026-11-05-full.json`](evidence/parity-paired-2026-11-05-full.json),
with a summary at
[`parity-paired-2026-11-05.json`](evidence/parity-paired-2026-11-05.json).
The revisions declared in `parity.json` must be named by committed evidence;
an unrecorded run in a discussion comment is not evidence. A pin can never name
the commit that contains it, so record the linter revision the harness actually
ran in, normally the declaring commit's parent.

## Pre-slice output parity

The linter's own `scripts/verify-baseline-parity.ps1` needs no formatter and no
external corpus. It rebuilds the binary from the merge base with the base branch,
rebuilds HEAD, and requires identical stdout, stderr and exit codes for
every source in `tests/fixtures/baseline/` in text, JSON and SARIF, plus
whole-corpus and stdin runs. Text and JSON are byte-exact; a SARIF document may
differ only by an added registry-derived `tool.driver.rules` entry, recorded as
`sarifDriverRuleAdded` in the report. It runs inside `scripts/verify.ps1`, so it
gates every Windows verification. Override the baseline explicitly when history
is shallow or the slice intentionally changes output:

```powershell
pwsh -NoProfile -File scripts/verify-baseline-parity.ps1 -BaseRef <pre-slice commit>
pwsh -NoProfile -File scripts/verify-baseline-parity.ps1 -BaselineBinary <path to pre-slice binary>
```

The JSON report is a local artifact under
`target/verification/baseline-parity/` and is not committed.

## Shell matrix

The PowerShell 5.1-compatible script checks exit codes 0/1/2 for both tools
using a Unicode-and-space file path, plus `cmd.exe` invocation for formatter
codes 0 and 1. Run the base matrix under PowerShell 7 and Windows PowerShell
5.1, then run `cmd.exe` checks from Windows PowerShell 5.1:

```powershell
$fmt = '..\verse-fmt-implementation\target\x86_64-pc-windows-msvc\release\verse-fmt.exe'
$lint = 'target\x86_64-pc-windows-msvc\release\verse-lint.exe'
pwsh -NoProfile -File scripts/verify-shells.ps1 -Formatter $fmt -Linter $lint -ShellLabel pwsh
powershell.exe -NoProfile -File scripts/verify-shells.ps1 -Formatter $fmt -Linter $lint -ShellLabel powershell51
powershell.exe -NoProfile -File scripts/verify-shells.ps1 -Formatter $fmt -Linter $lint -ShellLabel cmd
```

All three invocations passed on Windows: PowerShell 7 and PowerShell 5.1 each
observed formatter/linter exit codes 0, 1, 2; `cmd.exe` observed formatter 0
and 1. Error output is intentionally suppressed by the harness while the
native process exit code is checked. This tests local file arguments, not
PowerShell pipeline encoding; file input remains recommended for PS 5.1.

## Historical record (not validation of the current corpus)

Historical paired runs were recorded as the corpus grew: 52, 53, 56, and 58
goldens, followed by the 64/64 run on 2026-11-05 (corpus SHA-256
`27e0497bbccaba2bed56dfc93efcda762887f3888c9cd0c5707f0ae670050b47`). These
records describe only the corpus and revisions tested at those dates; **they do
not validate the current corpus**. Only a fresh complete paired run can do so.
