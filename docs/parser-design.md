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
forms include `<#>` indented comments, markup, quoted/non-ASCII identifiers,
multiline strings outside interpolation, and a statement line that starts with
unary `+`/`-` after another expression (the grammar would join it as a binary
operand, so the same guard as the formatter refuses it). Local dotted imports, initialized
typed constants, comma-separated braced enum variants, empty class base lists,
if-binding conditions (one item per line in the indented `if:` form), failable indexed `set` conditions, key/value iterators, block-bodied `case` arms, single-expression indented `spawn:`,
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

### Fixed: inline binary function bodies

A top-level inline binary body such as `Add(X:int,Y:int):int = X+Y` used to be
split by the pinned grammar into a function returning `X` plus a sibling unary
`+Y` **without** any `ERROR` node. The cause was the precedence
`_function_body` carried so that a `(` on the next line could not continue an
inline body as a call: the same precedence also ended the body before every
operator.

Naming `$.binary_expression` as its own `_function_body` alternative was tried
first. It reduced `X+Y` but changed which *other* adjacent-statement shapes the
grammar produced, so the guard would have had to weaken to keep `= X+Y+Z`
refused, and it was withdrawn.

The paired formatter change moves the boundary into the scanner instead. The
external `_call_open` token matches a `(` only when no newline separates it from
the previous token, so a `(` on the next line always starts a new item, as in
the official grammar, and the body precedence no longer has to stop at
operators. `= X+Y`, `= X+Y+Z`, `= point{...}` and `= external {}` are each one
body. The adjacent-statement guard is unchanged: it still refuses unseparated
statements on one line (`A := 1 B := 2`), and its message keeps the
`ambiguous adjacent top-level statements` prefix.

`tests/coverage-gaps.rs` pins both sides: the refused unseparated pairs, the
inspected inline bodies, and the cross-line `(` and `.` neighbours.

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
the whole live registry, because the baseline binary does not know V3001, V3002,
V2003, V1004 or V1005: a selection naming a rule the baseline has not registered
is a configuration failure there and the comparison would fail for the wrong
reason. All five are off by default, so the default-selection text and JSON runs
stay byte-identical; the default-selection SARIF runs may instead gain their
registry-derived driver-rule entries. Those rules' own non-regression is covered
by `tests/v3001-cli.rs`, `tests/v3002-cli.rs`, `tests/v2003-cli.rs`,
`tests/v1004-cli.rs` and `tests/v1005-cli.rs` instead.

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

When an indented for header starts with `Item : Read(Values)`, the adjacent
opener keeps the call inside the first iterator. The clause boundary guard
continues to reject a postfix attached across a newline.
