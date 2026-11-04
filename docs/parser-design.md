# Parser coverage and safety

Linter uses the pinned snapshot described in [syntax-snapshot.md](syntax-snapshot.md).
A strict lossless UTF-8 lexer protects strings, interpolation, characters,
line comments and nested block comments before the pinned CST is accepted.
Reject ERROR/MISSING nodes, unterminated spans, bare-CR endings, NUL, UTF-16
and invalid UTF-8. Mixed LF/CRLF files are scanned losslessly. Limits: 8 MiB/source, nesting 256, 250,000 tokens, two-second
parse budget, CST depth 512 and one million fingerprint events.

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
the same order and still fail closed. Every existing rule, test and fixture
produces byte-identical diagnostics; a fixture sweep pins that.

The graph models a bounded working set and records everything else as
unsupported. A non-empty skip map makes the graph incomplete, and a semantic
rule must refuse to report from an incomplete graph rather than guess. See
[semantic model](semantic-model.md). No semantic rule exists yet.
