# Parser coverage and safety

Linter uses the pinned snapshot described in [syntax-snapshot.md](syntax-snapshot.md).
A strict lossless UTF-8 lexer protects strings, interpolation, characters,
line comments and nested block comments before the pinned CST is accepted.
Reject ERROR/MISSING nodes, unterminated spans, bare-CR endings, NUL, UTF-16
and invalid UTF-8. Mixed LF/CRLF files are scanned losslessly.

Limits:

- 8 MiB per source file
- 256 nesting levels
- 250,000 tokens
- 2-second parse budget
- 512 CST depth
- 1,000,000 fingerprint events

Each bullet is machine-checked against its constant by `src/doc-drift-tests.rs`;
keep one limit per bullet.

Initial supported examples include assignments, creative devices, class fields,
methods/specifiers, using paths and nested literal/comment spans. Known rejected
forms include `<#>` indented comments, markup, quoted/non-ASCII identifiers
and multiline strings outside interpolation. Local dotted imports, initialized
typed constants, comma-separated braced enum variants, empty class base lists,
if-binding conditions, failable indexed `set` conditions, key/value iterators,
tested indented object/array construction with anonymous field initializers,
string literals whose first character is an unescaped `#`, and `map{...}`
literals are supported by the local grammar delta; this is
not compiler verification. Rejection means the
tool cannot safely inspect a shape, not necessarily that Verse is invalid.

### Closed in this slice: strings starting with `#`

The pinned scanner classified a string whose content began with an unescaped `#`
in a way that lost the closing quote: `A := "# text"` produced a `line_comment`
extra inside the literal and a missing `"`, so it failed as a coverage error
rather than as a rule or suppression error. That was a scanner/grammar bug, not
a deliberate rejection: the text is valid Verse and the linter refused to
inspect a real project over formatting trivia.

The fix is one token declaration in the vendored grammar:
`string_content` now carries `prec(1)`, so the content token wins over the
`line_comment` extra at the same position and `token.immediate` still keeps the
span inside the literal. Nothing else changes: the lossless lexer still scans
the whole string as one protected `String` token, every byte offset is
unchanged, and `Document::parse` keeps its failure modes. `A := "#"`,
`A := "# text"`, `A := "{Name}#tail"` and a trailing `# comment` after such a
string are all covered by tests. `map{...}` literals below use the same
mechanism.

> **Not** a licence to "accept all of Verse": only this one scanner
> classification changed, and every other refusal below is unchanged.

### Closed in this slice: `map{...}` literals

The pinned grammar had no rule for the map literal constructor, so
`map{"x" => 1}` degraded into an `identifier`, an `ERROR` and a stray
`string_literal`, and the linter refused it as ambiguous. `map` as a *value*
name worked (`map := 1`, `Print(map)`), which made the refusal surprising.

`map_literal` is now a rule: an ordinary `identifier` head followed by braces
containing at least one `key => value` entry, with `,`-separated entries.
The head stays an `identifier` rather than the keyword string so a project's own
`map` name keeps resolving; at least one entry is required so a brace body of
`name := value` initializers, or an entry-less `ident{}`, keeps its previous
`object_construction` reading instead of being silently reclassified. Dynamic
precedence picks the map reading when the body really holds `=>` pairs.
Documented positions today: constant/`type_definition` initializers, class field
values, inside `array`/`option`/argument lists, as a receiver of `[...]` indexing,
and nested inside another map value.

`map{}` (no entries) still parses through the older `object_construction` path
rather than `map_literal`, and that is what the walker observes: a field-less
`object_construction` is pure structure, so the graph stays complete
(`isComplete: true` with an empty `skipped` map), the head keeps an ordinary
identifier reference to `map`, and that reference resolves only if the project
binds the name. `A := map{}` therefore reports nothing by default and leaves
`map` unresolved for V3002 to name (`1:6`) when that rule is selected. So the
entry-less form is neither modelled as an empty map nor counted as an
unsupported kind; it is inspectable exactly as written, and
`src/semantic-tests.rs` pins the dump and the reference so this paragraph cannot
drift. Any other entry-less `ident{}` has the same shape.

### Not implemented: `<#>` indented comments

The pinned grammar **does** carry a `hash_gt_comment` extra for this form; its
comment says it models the "UEFN digest file format": `<#>` followed by the rest
of the line and any indented continuation lines. So the construct is named in the
grammar, and the refusal is not "this is not Verse".

The problem is the boundary. `hash_gt_comment` is a token that starts at `<#>` and
runs to the end of the last indented line, while the nested `<# … #>` block form
is an external scanner token. Deciding which one a given `<#>` begins needs the
digest-file context, and the independently retained lexical guard cannot prove
where a `<#>` text ends. Losing that boundary would silently swallow code, and a
lexer that guesses is worse than one that refuses. The scanner also sees `<#>`
first, so it currently wins: the flagged case exits 2 before the CST is accepted.

So this slice does **not** implement it. Instead the refusal names the construct
in both places:

- the lexer reports `` `<#>` indented comments (the UEFN digest-file form) are not
  safely inspectable by this strict lexer `` at the exact offset;
- the CST still refuses it as a coverage error, so nothing is silently accepted.

The decision is recorded rather than deferred: the linter will not inspect the
construct until its boundary rule is settled deliberately in both tools,
including the interaction between `hash_gt_comment` and the external block
comment scanner. Loosening the guard to reduce a refusal is not that settlement.

### Decided, not fixed: inline binary function bodies

A top-level inline binary body such as `Add(X:int,Y:int):int = X+Y` was split by
the pinned grammar into a function returning `X` plus a sibling unary `+Y`
**without** any `ERROR` node, which is the one failure mode a lossless CST walk
cannot see. The shared adjacent-statement guard rejects unseparated adjacent
top-level nodes on one physical line, and that guard stays.

Naming `$.binary_expression` as its own `_function_body` alternative was tried
and does reduce `X+Y` (and `X - Y`, `X * Y`, single same-line operators
generally) into one body, but it also changed which *other* adjacent-statement
shapes the grammar produces, so the guard would have had to weaken to keep
`= X+Y+Z` refused. A grammar change that buys one shape while making a second
refusal less reliable trades a visible limitation for a quieter one. The guard
therefore keeps **both** behaviours, and this slice records the outcome as a
known, tested limitation instead of an open question:

- `Add(X:int,Y:int):int = X+Y` is refused with a message naming inline binary
  function bodies;
- nested and spaced binary bodies (`X * Y`, `1 < 2`, `X + Y + Z` inside an
  indented block, parenthesised `(X+Y)`) stay supported;
- `tests/coverage-gaps.rs` pins the refusal and the supported neighbours,
  including the cross-line `(` / `.` continuation that the guard's precedence
  protects.

This is a coverage statement, not a compiler diagnosis: `X+Y` is valid Verse and
the tool does not claim otherwise.

Some refusals are for valid Verse: rejection means tool coverage is incomplete,
not that UEFN would reject the construct. Unicode inside literals/comments is
supported.

Block-ownership regression tests and original fixture provenance are retained.
No formatter layout pass runs during lint. Current tests are parser/tool tests;
**UEFN compiler and runtime acceptance have not been performed**. The original
formatter probe ran on Rust 1.98.1; current verification explicitly resolves
the pinned Rust 1.97.0 instead of trusting PATH ordering.

## Retained CST and the semantic layer

`Document` now retains the parsed tree alongside `tokens` and `shape`, so the
scope graph in `src/semantic.rs` can walk it. Retaining the tree changes no
coverage decision: the lexical scan, coverage validation, adjacent-statement
guard, budgets, block-comment boundary match and `shape` fingerprint all run in
the same order and still fail closed.

A non-empty skip map makes the graph incomplete, and a semantic rule must refuse
to report from an incomplete graph rather than guess. See
[semantic model](semantic-model.md). No semantic rule exists yet.

## Baseline output parity

"Byte-identical diagnostics" is a claim that has to be reproducible, so it is a
committed gate rather than a one-off observation.
`scripts/verify-baseline-parity.ps1` builds the pre-slice binary from the merge
base with the base branch, builds HEAD, and compares **stdout bytes, stderr
bytes and the exit code** for every source in `tests/fixtures/baseline/` in
text, JSON and SARIF, under both the default rule selection and an all-rules
selection, plus whole-corpus and stdin runs. A mismatch prints the differing
bytes and fails the script.

Text and JSON comparisons are byte-exact. SARIF output cannot be, because
`runs[].tool.driver.rules` is derived from the live rule registry, so a slice
that registers a rule gains one driver entry in every SARIF document. A SARIF
case is therefore accepted when that is the *only* difference: with the
driver-rule list removed from both documents every remaining field must be
identical, every rule the baseline emitted must reappear byte-for-byte
unchanged, and at least one rule must have been added. The acceptance relaxes
the stdout comparison and nothing else — a different exit code or any stderr
difference still fails — and it is recorded per case in the report as
`sarifDriverRuleAdded`, whose total is `sarifDriverRuleAdditionCount`. The
accept/reject condition itself is exercised by `-SelfTest`, which
`scripts/verify.ps1` runs first.

`scripts/verify.ps1` runs that script, so a regression cannot reach a green
Windows verification path, and `ci.yml` checks out full history so the merge
base is resolvable on the runner. The sweep also asserts its own
non-vacuity: the corpus runs must actually report rule IDs, and the JSON and
SARIF documents must contain their diagnostic containers, so a format that
silently serialized nothing cannot pass by comparing equal empty strings.

The fixture set is self-authored (see `tests/fixtures/provenance.md`) and
covers clean input, each of the five style rules, suppression, string and comment
protection, Unicode literals, and the grammar refusals the linter documents. It
is not compiler acceptance and does not need UEFN.

The sweep's all-rules selection is the five style IDs it names explicitly, not
the whole live registry, because the baseline binary does not know V3001, V3002 or
V2003: a selection naming a rule the baseline has not registered is a
configuration failure there and the comparison would fail for the wrong reason.
All three are off by default, so the default-selection text and JSON runs stay
byte-identical; the default-selection SARIF runs may instead gain their
registry-derived driver-rule entries. Those rules' own non-regression is covered
by `tests/v3001-cli.rs`, `tests/v3002-cli.rs` and `tests/v2003-cli.rs` instead.
