# Changelog

User-visible changes to `verse-lint`. This file records what is already on
`main`; an unreleased slice is not listed here until it merges.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and
the project intends to follow semantic versioning once there is a release to
version. There is no published release yet: `0.1.0-alpha.1` is an unreleased
alpha.

## Unreleased

### Added

- Five style rules: V1001 trailing whitespace, V1002 missing final newline and
  V1003 tab indentation (on by default), plus V2001 line length and V2002 bare
  `TODO` comment (opt-in warnings). Each rule has a document under `docs/rules/`.
- Narrow line suppression: only lexer-confirmed line comments whose content
  begins exactly `verse-lint:` are directives, so strings and block comments can
  never suppress a finding. `--show-suppressed` also lists the findings a
  directive removed, marked ` (suppressed)` in text and `"suppressed": true` in
  JSON (SARIF keeps them out of `results`); it does not say which directive
  removed them, and they never change the exit code.
- Text, JSON and SARIF 2.1.0 output. The JSON contract has a published schema in
  `docs/diagnostics.schema.json`; SARIF is validated against the pinned official
  OASIS schema and is emitted to stdout only, with no network access and no
  upload.
- `verse.toml` configuration, including `[lint.severity]` per-rule overrides,
  where severity and selection are independent and a promoted but unselected
  rule stays inactive. Unknown IDs, unknown values and malformed records exit 2.
- Rule-family prefixes in `select`/`ignore`: `V1`, `V2` or `V` expand to the
  registered rules they name, on the command line and in configuration.
  `--show-config` always prints the expanded complete IDs.
- `verse-lint.ignore`, a project-level exclusion list read from the
  configuration root only, combined with `[files] exclude` as a union.
  `--no-ignore-file` skips it for one run.
- Guarded Windows safe fixes for V1001 and V1002 only, with staged writes,
  backup-backed replacement and revalidation of identity, attributes and bytes.
  Fixes refuse rather than guess tab widths, rename symbols or reflow comments.
- Independent pinned Verse syntax snapshot from `verse-fmt`, with recorded
  revision and hashes, plus a local grammar delta for constructs found in
  practical projects. No formatter crate, binary or path dependency at runtime.
- Stdin input, with `--stdin-filepath` as a virtual path rather than a write
  target.
- A deterministic local Windows archive built by `scripts/package.py`, and a
  paired formatter convergence harness and PowerShell/cmd smoke matrix for
  developers. None of them publishes anything.

### Notes

- Compiler and UEFN acceptance is **not** established, and broad-project
  readiness is unproven. Unsupported or incomplete syntax is an execution error
  (exit 2), never a claim that the UEFN compiler rejects the source.
- Every documented resource limit is enforced and covered at its boundary, and
  `src/doc-drift-tests.rs` fails the build when a documented number, rule table
  or provenance hash drifts from the code.
- The project does not upload SARIF to GitHub code scanning; see
  `docs/adr/0001-sarif-upload-policy.md`.
- Windows x64 is the test and distribution target. `verse-lint.dev` is a
  planned, unregistered domain and not an active homepage.
