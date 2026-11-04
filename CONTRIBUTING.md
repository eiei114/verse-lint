# Contributing

## Before you start

```powershell
pwsh -NoProfile -File scripts/verify.ps1
```

This is the same entry point CI runs. It builds with the Rust toolchain pinned in
`rust-toolchain.toml` and it needs neither a formatter binary nor network access
beyond the Cargo registry.

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
2. Update `parity.json`: `corpusSha256`, `goldenCaseCount`,
   `formatterGrammarBaseRevision`, and — only after a successful run — both
   verified revisions. Declaring a revision you have not actually verified is a
   false claim, not a pin.
3. Build both release binaries.
4. Run `scripts/verify-pair.py` against **clean checkouts of both repositories**.
   It refuses a dirty tree on either side, and it must not be run against one.
5. Record both revisions it reported in `parity.json`, and state the run in the
   pull request with the corpus hash, both revisions, and the case counts.
6. Run `scripts/verify.ps1` again and commit everything together.

```powershell
python scripts/verify-pair.py `
  --formatter ..\verse-fmt\target\x86_64-pc-windows-msvc\release\verse-fmt.exe `
  --corpus ..\verse-fmt\tests\fixtures\corpus.json `
  --output target\verification\paired\convergence.json
```

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
- Rebuild the formatter from a clean checkout and re-run the paired harness.
- Update the corpus hash, golden count, and both verified revisions in
  `parity.json` only after that run passes.
- If you cannot do the paired run, say so explicitly in the pull request and
  leave the pin untouched. Do not widen the pin to make a failing harness pass,
  and do not claim convergence you did not observe.

## Declared vs. verified

- `scripts/verify.ps1` → `scripts/verify-parity.ps1` checks the **declared**
  side only: structure of `parity.json`, and that its linter revision is
  reachable from `HEAD`. Fast, offline, no formatter.
- `scripts/verify-pair.py` checks the **real** convergence against a real
  formatter binary and corpus. It is the only thing that can claim convergence,
  and it stays a deliberate developer operation.

See [paired and shell integration checks](docs/integration-testing.md) for the
harness details and [the independent syntax snapshot](docs/syntax-snapshot.md)
for what the snapshot is allowed to contain.

## Scope and honesty rules

- A green local run does not prove UEFN compiler acceptance or broad-project
  readiness. Say what you ran and what you did not.
- No crate publish, tag, release or GitHub code-scanning upload from a pull
  request, and no new runtime dependency without a recorded decision.
- Keep commits focused; do not reformat unrelated code.
