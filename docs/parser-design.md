# Parser coverage and safety

Linter uses the pinned snapshot described in [syntax-snapshot.md](syntax-snapshot.md).
A strict lossless UTF-8 lexer protects strings, interpolation, characters,
line comments and nested block comments before the pinned CST is accepted.
Reject ERROR/MISSING nodes, unterminated spans, mixed line endings, NUL, UTF-16
and invalid UTF-8. Limits: 8 MiB/source, nesting 256, 250,000 tokens, two-second
parse budget, CST depth 512 and one million fingerprint events.

Initial supported examples include assignments, creative devices, class fields,
methods/specifiers, using paths and nested literal/comment spans. Known rejected
forms include `<#>` indented comments, markup, quoted/non-ASCII identifiers,
multiline strings outside interpolation and top-level typed `Count:int=1`.
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
