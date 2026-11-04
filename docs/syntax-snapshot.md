# Independent syntax snapshot

Shared toolchain contract version: **1** (not the same as config schema-version).

Imported from https://github.com/eiei114/verse-fmt at exact commit
`08a9e6abc3cf7cdb762fdeea2b6448b31baa22e1` on 2026-09-22, MIT OR Apache-2.0,
copyright 2026 eiei114. Both license texts and NOTICE are retained. This is an
independent source snapshot: no path dependency, formatter binary invocation,
third crate, latest-main fetch or build-time upstream download.

P1 adds an independently retained `:=` range-generator grammar branch and
structural regression tests to this snapshot. Parser artifacts are regenerated
locally with pinned Tree-sitter CLI 0.25.10; no scanner or production guard
changes. See [range-binding scope](range-binding.md) and the vendor hash manifest.

| Original path | SHA-256 at import |
|---|---|
| src/source.rs | 71d35cbd2cc7aa0a9a58119875580ccb937d6cf99574c0474bab5546b718745d |
| src/lex.rs | ed7b21ae796873b28f4eb4a1c398a3d0e43bdcbca002d00bdcee4f0decd5bc0a |
| src/syntax.rs | ef8e6ddcf88424dea4f7758c4a7d9abb1b160d7fc4d02d614372d8318c7c1379 |
| src/files.rs | 79c5fa3055bfe09a919d75ff399d77298ec3cc8c6544cbd95af61b7cb584b2b2 |
| src/config.rs | ef50d9805070bb938ad65b4740fed01555d1c7220ece6894ed7cdb11391543d3 |
| tests/fixtures/device.input.verse | 151b438268ead552e9bd87a516a0748120bd9ec90360b7190d832272b3765541 |
| tests/fixtures/device.expected.verse | 3d62fd67d28cb69f8ef2c8a511af181f65d43b8fe64ea33442f935049525320d |

These hashes are immutable provenance for the original import: the imported files
have since changed locally, so they are not compared against the working tree.
The `IMPORTED_PATHS` and `IMPORT_REVISION` pins in `src/doc-drift-tests.rs` keep
this record complete and stable when the table is edited.

## Pinned vendor manifest

| Path | SHA-256 |
|---|---|
| grammar.js | be54ee4d73e48039e19682178f356d714af804a0992e9248ec637ad013b2e58c |
| tree-sitter.json | f57ee6d1a5ee6958875e13074e8cbb8d53d87bb71793291bebd9854dd87e9ad6 |
| src/parser.c | 76b1089741ebc27a7c4e6745857253c728f142d15776df5bcde38ef936720197 |
| src/scanner.c | 470901f9b9408fa552d7cbd14538d45362ef2fb492908643cbb1fdc465288006 |
| src/grammar.json | f983ec8d3b0b52501c5587dd04c533801c35ebae0d9e202f41a3b5c40c06208b |
| src/node-types.json | d5399c4befebc170a08a30ef3268e4fa80b857333c2dbaf46733e25528b3f605 |
| src/tree_sitter/alloc.h | b29c1c9fb7cc82f58c84b376df1297d6e2737a1d655fd356db0859e3c29c2fea |
| src/tree_sitter/array.h | 5bdf6ed1a78e3409fd443e085ca967a64c188a5d082aaf7f819bccd53a471c94 |
| src/tree_sitter/parser.h | 180b893c8734778fd32f372dfbc27bd6ad1cd2221f26150b31256ff6716320d2 |
| LICENSE | d6a46baba7c836a5b7e20fe7dd7e51b0cdcb6b9fd3b597a6e89699c4cbbd0dd6 |
| LICENSE-tree-sitter | 5f9cf9fb6acb1972b35ae29119ce563bb60ec097656bc4b69b9bac2d04c7a147 |

The entries restate `vendor/tree-sitter-verse/hashes.json`, so a reader sees the
pinned bytes without opening the manifest. `src/doc-drift-tests.rs` asserts this
table against the manifest, and `scripts/verify-grammar.ps1` hashes the files
themselves, so an added, removed or regenerated artifact cannot leave either one
stale. Regenerate the rows from the manifest, never by hand.

Also imported: build.rs, byte-preserving .gitattributes, fixture provenance and
the MIT-licensed tree-sitter-verse snapshot. Its grammar/parser are locally
patched/regenerated, and scanner.c later gained a serialization-bound fix;
headers and license remain upstream bytes. Exact base/local hashes are in
[`upstream.md`](../vendor/tree-sitter-verse/upstream.md).
Grammar revision/hashes are in [upstream.md](../vendor/tree-sitter-verse/upstream.md).
All Verse examples are self-authored; no Epic assets/digests/proprietary code.

Local adaptations: config validates `[lint]` and tolerates sibling `[format]`;
syntax removes formatter-only spacing/call offsets. CST shape is walked
and bounded, with equivalent-tree regression checks and production fix
revalidation. Formatter indentation
normalization is not imported: linter must be able to diagnose tabs without
guessing their visual width. Rules/reports/orchestration are independent.

Source bytes, BOM, CRLF, Unicode scalar positions and strict protected spans
share the imported contract. Vendored recovery-parser success alone is never
sufficient. Unsupported/incomplete inputs become execution failures, not lint
rules and not claims of compiler invalidity. See [parser design](parser-design.md).
Corpus updates require a new pinned source revision, hashes and regression tests.
The local grammar delta adds dotted local imports, initialized typed constants,
comma-separated braced enum variants, empty class base lists, if-binding
conditions, failable indexed `set` conditions, key/value iterators, and tested
indented object/array construction with anonymous field initializers, as
independently documented in
`vendor/tree-sitter-verse/upstream.md`. Scanner bytes remain pinned unchanged.
Generated ABI-15 output, metadata, runtime headers and their separate MIT
notices are pinned in `vendor/tree-sitter-verse/hashes.json`. Run
`scripts/verify-grammar.ps1` to audit; maintainers regenerate with
`-Regenerate` using Tree-sitter CLI 0.25.10. Normal Cargo builds require no
Node/npm or live grammar checkout.
During corpus expansion, both tools gained an independent guard for unseparated
same-line top-level nodes: the pinned grammar can split an inline binary
function body into unrelated nodes without ERROR. The grammar delta does not
address this ambiguity. Map literals and this ambiguity have explicit negative
regressions.

Read-only practical inspection additionally exposed those grammar gaps. The
newly authored minimal regressions in `tests/coverage-gaps.rs` now cover those
shapes without project source. An adjacent-top-level error may still reflect
lost enclosing structure in recovery; it is not a reliable classification of
the original Verse construct. Broad project readiness remains unproven.

## Independently retained block-comment boundary guard

The local syntax adapter now requires each lexer `BlockComment` token to match
an exact CST `block_comment` span, accounting for the UTF-8 BOM. This guard is
retained independently in both tools; no formatter runtime dependency or vendor
grammar/scanner change is introduced. Matching runs within the existing bounded
CST traversal with a token-limit-bounded span set. The earliest unmatched span
deterministically reports exit 2. All-input validation prevents fixes when a
later input has a mismatch.

This is one-way lexical block-comment coverage, not full semantic or all-token
validation. Strings/interpolation stay opaque. Some previously accepted
ambiguous comments are intentionally refused, not repaired. P2 newline-if
support remains absent. Original `tests/comment-boundary.rs` cases verify
inspection/fix refusal, unchanged bytes, positive nested/multiple comments,
opaque literals, BOM/CRLF, stable positions and batch write prevention.

## Windows adapter snapshot (L05)

`src/write.rs` was imported from the same formatter commit. Import SHA-256:
`596fb99e28a8295392cfb339063e24d12908cec3cb7fb42b2c95902172301833`.
Local changes: remove unused stdin constructor (stdin fixing is prohibited),
request DELETE access during preflight to detect a readable handle that denies
replacement. The DELETE fix was also applied to the formatter with a regression
test. Adapter fault-injection tests are retained; linter additionally tests
mid-batch accounting and remaining diagnostic positions. This source copy is
not a runtime dependency. Both adapters require future fixes to be cross-reviewed.

The subsequent Windows audit added owner/primary-group equality checks before
staged bytes are written and again before replacement, synchronized in both
tools. Non-matching ownership is refused, not forcibly changed. Writes accept
only normal/hidden/system/archive/not-content-indexed attributes; unverified
compressed/encrypted/sparse/temporary/cloud-managed flags are refused. Tests cover
an actual non-default primary group, basic attributes, >260-character Unicode
paths, oversized sources and closed stdout. These are not complete SACL,
adversarial filesystem or network-share guarantees.
