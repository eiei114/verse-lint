# Contributing

## Before you start

```powershell
pwsh -NoProfile -File scripts/verify.ps1
```

This is the same entry point CI runs. It builds with the Rust toolchain pinned in
`rust-toolchain.toml` and it needs neither a formatter binary nor a formatter
checkout. Beyond the Cargo registry it may still touch the network: it installs
the pinned toolchain when that toolchain is absent and it always adds the
`rustfmt` and `clippy` components through `rustup`, so a first run on a bare or
offline host needs registry and `static.rust-lang.org` access. Once the pinned
toolchain and components are installed, `--locked` builds resolve from the local
cache and `scripts/verify-parity.ps1` itself stays offline.

## Parity checklist (verse-fmt)

The linter keeps an independent syntax snapshot and an independent paired
convergence harness. It never takes a runtime dependency on `verse-fmt`: no
formatter crate, no submodule, no CI checkout, no build-time download. The
relationship is a recorded contract in `parity.json`, checked by humans at
review time and by tooling for structural problems.

### Changes that require a paired update

Update the linter snapshot **and** `parity.json` in the same pull request when
your change touches any of:

- the grammar or scanner snapshot under `vendor/tree-sitter-verse/`;
- the formatter's golden corpus (`tests/fixtures/corpus.json` in `verse-fmt`);
- safe-fix behaviour, i.e. the bytes `verse-lint --fix` produces.

Both repositories must carry the same grammar delta. Add the grammar change to
`verse-fmt` first (or in lockstep), then mirror it into the linter snapshot with
the same intent, the same self-authored regressions, and an updated hash
manifest — never by taking a path dependency or by invoking the formatter at
build time.

### Order of operations

1. Update the linter's own snapshot and its regressions; update
   `vendor/tree-sitter-verse/upstream.md` and `hashes.json`.
2. Commit the source and corpus changes in both repositories, then make sure
   **both checkouts are clean**. The paired harness refuses dirty checkouts.
3. Build both release binaries and run the full harness with `--update-pin`.
   Point `--output` at the report path under `docs/evidence/` so no manual copy
   is needed. The command updates `parity.json` only after every golden passes.
   Add `--dry-run` to preview the candidate; it runs the full harness but writes
   neither the pin nor the report.
4. Commit the updated `parity.json` and the generated report with the linter
   change. Record both full 40-character revisions, the corpus hash, and case
   counts in the pull request. A revision in `parity.json` that no committed
   report names is an unverified claim.
5. Run `scripts/verify.ps1` and include its result in the pull request.

The updater changes `corpusSha256`, `goldenCaseCount`,
`lastVerifiedFormatterRevision`, and `lastVerifiedLinterRevision` from the
successful run. It preserves `formatterGrammarBaseRevision`; do not hand-edit
the other pin values. Without `--update-pin`, a corpus hash or golden-count
mismatch still fails the harness exactly as before.

A pin can never name the commit that contains it. Record the revision the
harness actually ran in, which is normally the parent of the declaring commit;
the declaring commit itself changes only documentation, the pin and the report,
so the tested binaries are unchanged. Say that explicitly in the pull request
instead of implying the declaring commit was verified by a run inside itself.

```powershell
python scripts/verify-pair.py `
  --formatter ..\verse-fmt\target\x86_64-pc-windows-msvc\release\verse-fmt.exe `
  --corpus ..\verse-fmt\tests\fixtures\corpus.json `
  --output docs\evidence\parity-paired-YYYY-MM-DD-full.json `
  --update-pin
```

Replace `YYYY-MM-DD` with the run date. The second-cycle byte-invariance check
remains part of every paired run; updating a pin never skips that assertion.

### What to do when only the linter changes

Most changes — rules, CLI, JSON/SARIF output, diagnostics, configuration,
performance — do not touch the grammar, the corpus, or safe-fix bytes. In that
case:

- no paired run is required;
- `parity.json` revisions stay as they are, because they name the last verified
  pair and not your branch.

`scripts/verify.ps1` accepts that state by design: a declared revision only has
to be an ancestor of `HEAD`. It fails only when the declaration is missing,
malformed, incomplete, or names a revision that is not reachable from `HEAD`.

### What to do when the formatter's grammar changes

- Mirror the change into the linter snapshot in the same work item.
- Commit the formatter corpus update, rebuild both binaries from clean
  checkouts, then run the paired harness with `--update-pin`.
- The successful run updates the corpus hash, golden count, and both verified
  revisions in `parity.json`.
- If you cannot do the paired run, say so explicitly in the pull request and
  leave the pin untouched. Do not widen the pin to make a failing harness pass,
  and do not claim convergence you did not observe.

## Declared vs. verified

- `scripts/verify.ps1` → `scripts/verify-parity.ps1` checks the **declared**
  side only: structure of `parity.json`, and that its linter revision is
  reachable from `HEAD`. That step is fast, offline and needs no formatter;
  `verify.ps1` as a whole still resolves its pinned toolchain through `rustup`.
- `scripts/verify-pair.py` checks the **real** convergence against a real
  formatter binary and corpus. It is the only thing that can claim convergence,
  and it stays a deliberate developer operation. Use `--update-pin` to move the
  declaration; a dirty, failed or incomplete run cannot update it.

See [paired and shell integration checks](docs/integration-testing.md) for the
harness details and [the independent syntax snapshot](docs/syntax-snapshot.md)
for what the snapshot is allowed to contain.

## Scope and honesty rules

- A green local run does not prove UEFN compiler acceptance or broad-project
  readiness. Say what you ran and what you did not.
- No crate publish, tag, release or GitHub code-scanning upload from a pull
  request, and no new runtime dependency without a recorded decision.
- Keep commits focused; do not reformat unrelated code.
