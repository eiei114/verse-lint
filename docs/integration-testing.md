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
  that can claim convergence.

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

From the linter worktree, after building both release binaries:

```powershell
python scripts/verify-pair.py `
  --formatter ..\verse-fmt-implementation\target\x86_64-pc-windows-msvc\release\verse-fmt.exe `
  --corpus ..\verse-fmt-implementation\tests\fixtures\corpus.json `
  --output target\verification\paired\convergence.json
```

The script takes the corpus SHA-256 and the required golden-case count from
`parity.json` and requires the corpus to match both. It locates the formatter
checkout from the tracked corpus path, so the release
binary may live under `target/release` or an external `CARGO_TARGET_DIR`.
It exercises **one** order per case, linter first and formatter second: for each
case it runs `verse-lint --fix`, then `verse-fmt --write`, checks the formatter's
independent golden bytes, and repeats the same linter-then-formatter pair to
prove the second cycle changes nothing. The reverse order (formatter first) is
not run, so a defect that only appears when `verse-fmt` writes before
`verse-lint --fix` is outside what this harness observes. It uses isolated
temporary files and removes them on exit. The 23 explicit formatter refusals are
not sent through a successful paired-fix path; this harness does not exercise
them at all.

The one report that backs the current `parity.json` revisions is committed as
[`docs/evidence/parity-paired-2026-11-05-full.json`](evidence/parity-paired-2026-11-05-full.json),
with a short summary beside it in
[`parity-paired-2026-11-05.json`](evidence/parity-paired-2026-11-05.json).
[`parity.json`](../parity.json) may only quote revisions that a committed report
names; an unrecorded run in a discussion comment is not evidence. Other runs
stay local artifacts and are reported in the pull request with the corpus hash,
both full 40-character revisions and the case counts.

On 2026-09-23, all 52/52 cases passed with formatter revision
`ae191de08058a217d331640f0b098d6fb76435f3` and linter revision
`8ecb25acfebdf22d08accfcf56ecf145a72bf9cd`; corpus SHA-256 was
`5b981328c3020541697d95a21db76031386347a6d4902aef2fb2428b35ecf5f7`.
This is a tool-to-tool invariant test, not UEFN compiler acceptance.

After mixed LF/CRLF preservation became supported, the corpus grew by one golden
case. Current Iteration 10 paired report validates 53/53 with corpus SHA-256
`bb9275b7950b798ca001757f8c7f34aa87d424d7391d899b873986b3355adc83`.

Iteration 11 adds self-authored parser/formatter coverage for `class()`,
if-binding conditions and nested indented object/array construction. After the
changes were committed, the paired harness validated all 56 golden cases using
corpus SHA-256
`f5e44fbb19e017bf7caf242ab2275fab991169e7fb22c325cef462ec48452c94`.

Iteration 12 adds self-authored coverage for failable indexed `set` conditions
and key/value `for` iterators. That paired run validated 58 golden cases;
the corpus SHA-256 is `68918abdf3f7b574ccdc803c4af34bc1f5ebeaf1cc0637e8330fcfa6f1465f28`.

P1 extends the corpus to 87 cases (64 goldens and 23 refusals), adding bounded
`:=` range generators.

On 2026-11-05 a paired run validated all 64/64 goldens with formatter revision
`27d3dc76a61e569a394a9115d5c0c37a63345626` and linter revision
`a9b43c6c137be9f03618869d0c2ec6635dd42e29`; corpus SHA-256 was
`27e0497bbccaba2bed56dfc93efcda762887f3888c9cd0c5707f0ae670050b47` (87 corpus
cases, 64 goldens, 23 refusals). Both binaries were built from clean checkouts
with the pinned 1.97.0 MSVC toolchain, the formatter corpus was the tracked file
at that formatter revision, and every case reported exit 0 for both tools with
zero second-cycle changes. The full per-case report is committed at
[`docs/evidence/parity-paired-2026-11-05-full.json`](evidence/parity-paired-2026-11-05-full.json).
`parity.json` quotes exactly those two revisions and that corpus hash.

That linter revision is the commit the harness actually ran in. The pin update
that quotes it is a declaration-only descendant: `parity.json`, this document
and the evidence file are not compiled into either binary, so the tested bytes
are unchanged. A pin can never name the commit that contains it, which is why
the recorded revision is the tested parent rather than the declaring commit.
Historical runs above do not validate the six P1 goldens; only the 2026-11-05
run does.

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
