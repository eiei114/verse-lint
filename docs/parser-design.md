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
and tested indented object/array construction with anonymous field initializers
are supported by the local grammar delta; this is
not compiler verification. Rejection means the
tool cannot safely inspect a shape, not necessarily that Verse is invalid.
The pinned scanner also misclassifies some strings beginning with an unescaped
`#` (for example `A := "# text"`); these fail with a coverage error, not a
suppression/rule error. This is recorded as an upstream parser limitation, not
silently worked around by changing literal bytes.
Map literals such as `map{"x" => 1}` are also unsupported by the pinned parser.
It can split a top-level inline binary function body (`Add(X:int,Y:int):int =
X+Y`) into a function returning X and a sibling unary +Y without ERROR. A shared
guard now rejects unseparated adjacent top-level nodes on one physical line.
Nested/spaced binary bodies remain supported where their CST is complete;
this is not a ban on all binary expressions or a compiler diagnosis.
Some are valid Verse: rejection means tool coverage is incomplete, not that
UEFN would reject them. Unicode inside literals/comments is supported.

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
the whole live registry, because the baseline binary does not know V3001: a
selection naming a rule the baseline has not registered is a configuration
failure there and the comparison would fail for the wrong reason. V3001 is off
by default, so the default-selection runs stay byte-identical too. The rule's own
non-regression is covered by `tests/v3001-cli.rs` instead.
