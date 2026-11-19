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
| grammar.js | 84febe8e9b430b701ecd68e03033a125251aeb2e86347733cabb5429e52743f6 |
| tree-sitter.json | f57ee6d1a5ee6958875e13074e8cbb8d53d87bb71793291bebd9854dd87e9ad6 |
| src/parser.c | 70069ce9cef1f752a4e33bed80cc45e6bc82373e0ff10acb804bce7295a4fe70 |
| src/scanner.c | 4939d0b2e5ef023cf50ca41dc78e930f4c4fb492367d698df328a8c0d2cedc37 |
| src/grammar.json | 55a8139edb3851477e05ab4066e270c96d4a8449d28429ca8d3fad5eca7bb38d |
| src/node-types.json | de8a345cdc07519b9f19415161b3a6c8f73685f7cf9caa0d32d999b9be218913 |
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

## Declared parity

`parity.json` records the mirrored formatter grammar base revision alongside the
last verified formatter and linter revisions and the formatter corpus hash.
`scripts/verify-parity.ps1` (run by `scripts/verify.ps1`) checks that declaration
structurally and that the recorded linter revision is reachable from `HEAD`; it
needs no formatter and does not fail when the formatter has moved on. Only
`scripts/verify-pair.py` can claim convergence, because only it runs both tools
over the real corpus. See [paired and shell integration checks](integration-testing.md)
and the [parity checklist](../CONTRIBUTING.md).

Local adaptations: config validates `[lint]` and tolerates sibling `[format]`;
syntax removes formatter-only spacing/call offsets. CST shape is walked
and bounded, with equivalent-tree regression checks and production fix
revalidation. **The parsed CST is now retained** in `Document` for the scope
graph in [semantic-model.md](semantic-model.md); retention changes no guard, and
`Document::parse` keeps its signature and every failure mode. That the retained
tree changes no rule output is enforced by the byte-exact pre-slice-versus-HEAD
sweep in `scripts/verify-baseline-parity.ps1`, which `scripts/verify.ps1` runs
over text, JSON and SARIF. Formatter indentation
normalization is not imported: linter must be able to diagnose tabs without
guessing their visual width. Rules/reports/orchestration are independent.

Source bytes, BOM, CRLF, Unicode scalar positions and strict protected spans
share the imported contract. Vendored recovery-parser success alone is never
sufficient. Unsupported/incomplete inputs become execution failures, not lint
rules and not claims of compiler invalidity. See [parser design](parser-design.md).
Corpus updates require a new pinned source revision, hashes and regression tests.
When the formatter's grammar or corpus changes, the linter snapshot and
`parity.json` change with it in the same pull request; the checklist in
[CONTRIBUTING](../CONTRIBUTING.md) names the exact order of operations.
The local grammar delta adds dotted local imports, initialized typed constants,
comma-separated braced enum variants, empty class base lists, if-binding
conditions (one item per line in the indented `if:` form), failable indexed `set` conditions, key/value iterators, block-bodied
`case` arms, single-expression indented `spawn:`, tested
indented object/array construction with anonymous field initializers, string
literals whose first content character is an unescaped `#`, and `map{...}`
literals, as independently documented in
`vendor/tree-sitter-verse/upstream.md`. Scanner bytes remain pinned unchanged.
Generated ABI-15 output, metadata, runtime headers and their separate MIT
notices are pinned in `vendor/tree-sitter-verse/hashes.json`. Run
`scripts/verify-grammar.ps1` to audit; maintainers regenerate with
`-Regenerate` using Tree-sitter CLI 0.25.10. Normal Cargo builds require no
Node/npm or live grammar checkout.

Call arguments admit a tuple type, including in a parametric archetype
head such as `event(tuple(int, logic)){}`. The archetype keeps its existing
call-shaped head and braced body.

Parenthesized `for` headers accept a later comma-delimited filter binding.
A direct range after `:=` retains its generator node. Filter bindings are
visible after their initializer in the loop's scope.

The parameter list in a `:=` declaration may carry effect specifiers,
covering constructors with archetype bodies. The CST keeps the declaration,
parameters, effects and value together. The scope graph skips this effectful
form because it does not model constructor parameter and field semantics.

Indented `enum:` bodies use the same variant list as braced enums, with
optional commas between variants and after the final variant.

Function signatures allow whitespace between their closing parameter
parenthesis and first effect specifier. Comparisons retain their operator
nodes and the remaining specifier openers retain their immediate syntax.

An indented archetype may omit all field initializers. Its scanner
indent/dedent pair still encloses the archetype and keeps following statements
at their original scope. A guard refuses a next-line brace that would otherwise
be attached to the empty archetype as a new construction.

### String and map regeneration (grammar, not scanner)

- Upstream base revision: `6b5433e37b52c03c4c07f9468bf7cc11e45f2f82`, unchanged.
  These are local deltas on the pinned base, not a rebase onto newer upstream.
- Tool: pinned Tree-sitter CLI 0.25.10, `generate --abi 15`.
- Local `grammar.js` changes: `string_content` gains `prec(1)` so the content
  token outranks the `line_comment` extra inside a literal; a `map_literal` /
  `map_entry` rule pair is added and one parse conflict is declared for it.
  `src/scanner.c` is byte-identical to the previous pinned hash
  `470901f9b9408fa552d7cbd14538d45362ef2fb492908643cbb1fdc465288006`.
- Regenerated artifacts and their local SHA-256 are the four manifest entries for `grammar.js`, `src/parser.c`, `src/grammar.json`,
  `src/node-types.json`. `hashes.json` is the single source for those values;
  `src/doc-drift-tests.rs` asserts this table against it and
  `scripts/verify-grammar.ps1` rehashes the files themselves.
- A default re-run of `tree-sitter generate --abi 15` on the unmodified snapshot
  reproduced the previous `parser.c` byte for byte, so the toolchain is
  reproducible here and the identical scanner/grammar.json pair is not a
  coincidence.

Both tools guard unseparated same-line top-level nodes, because the pinned
grammar can split an inline binary function body into unrelated nodes without
ERROR. **This slice does not remove that guard.** See
[parser design](parser-design.md#decided-not-fixed-inline-binary-function-bodies)
for the attempt, why it was withdrawn, and the tested behaviour that replaces
an open question. `map{...}` has positive regressions instead of a negative one
now that it parses.

Read-only practical inspection additionally exposed those grammar gaps. The
newly authored minimal regressions in `tests/coverage-gaps.rs` cover those
shapes without project source. An adjacent-top-level error may still reflect
lost enclosing structure in recovery; it is not a reliable classification of
the original Verse construct. Broad project readiness remains unproven.

### Divergence from the `verse-fmt` sibling, and issue #2

`verse-fmt` keeps its own independent snapshot at its own pointer. This slice
changes only the linter's copy: its grammar gains `string_content` precedence and
`map_literal`, and its `scanner.c` is untouched. The formatter still refuses
both shapes (`A := map{"x" => 1}` exits 2), so the two snapshots now disagree on
exactly these two constructs.

The paired harness therefore needs a deliberate update on the formatter side: the
formatter grammar must gain the same two deltas, and its `reject-map-literal`
and `reject-leading-hash-string` corpus cases must move from `reject: true` to
real expected bytes. Until that lands the two snapshots disagree, and the
corpus hash must change when those cases move.

What this slice did verify: `scripts/verify-pair.py --update-pin --dry-run`
against the current formatter passed all 64 goldens (the `reject:` cases carry no
`expected` bytes and are not part of that count), so the *goldens* still
converge and only the two refusal cases diverge. A real pin update was not
committed here, because a pin that names a linter revision must also be quoted
by a committed paired report that used the formatter's mirrored grammar; writing
one now would record a run taken against the old formatter corpus. That is the
follow-up, tracked by GitHub issue #2 (`[parity] verse-fmt と grammar snapshot /
corpus hash の同期運用`). `scripts/verify-parity.ps1` still passes: it checks the
declaration's structure and the reachability of the recorded linter revision,
and claims no convergence.

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

Indented `for: … do:` headers begin with an iterator. Later clauses reuse the
parenthesized iterator and filter-binding nodes. A guard rejects same-line
clauses without a comma and line-leading postfix openers that would otherwise
be absorbed by the preceding expression. The same check covers a postfix on\na new line after the loop result. Loop bindings stay in the loop scope.

A typed `var` declaration accepts `=` or `:=` before its initializer.
Both spellings keep the same mutable binding, type and initializer fields.

An `option` expression supports comma-separated values in braces and one
expression in an indented body. The guard refuses a line-leading postfix
that would attach to a previous expression across these newly accepted boundaries.

Call arguments admit unambiguous array types (`[]T`) and the bare `logic`
type keyword. `logic{...}` still parses as a logic expression, and the
archetype keeps its call-shaped head and braced body.

An indented `if: … then:` and parenthesized `if (...):` can share an
`else if` chain. Mixed alternatives preserve the condition/body ownership
and the following sibling. The linter marks mixed-clause semantic coverage
incomplete.

An empty indented block can end at the same newline as its enclosing blocks.
The scanner queues the enclosing dedents along with the phantom block's
dedent, keeping the first following statement in its outer scope.

A case arm may use an inline `set` assignment as its body. The assignment
retains the same CST fields as a `set` inside an indented arm.

A `:=` declaration can own a braced list of two or more comma-separated
expressions. Initializer braces with `Name := Value` keep the existing
struct-literal or construction nodes. A line-leading bare list following a
call stays unsupported because of the possible attachment to that call.

An indented `for` header can contain a `set` clause after its iterator.
The header retains the assignment and its operator.
