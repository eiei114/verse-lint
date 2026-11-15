# Control-flow plan for V3004 `unreachable-code`

This is a written plan, not a rule. **V3004 is not implemented, not registered and
not selectable**: there is no registry entry, no CLI flag, no detector and no
document under `docs/rules/`. The plan exists so a later wave can decide whether
to build the rule on evidence instead of starting from a placeholder.

[`docs/semantic-model.md`](semantic-model.md) states the invariant this plan
inherits: a semantic rule may report only from a model it can prove complete.
The scope graph in `src/semantic.rs` answers *what a name refers to*; it does not
describe *which statement executes*. V3004 needs a second, narrower description
of a block's statement sequence, and that description is what this plan bounds.

## What the pinned snapshot already gives us

The pinned grammar and adapter are the ones recorded in
[`docs/syntax-snapshot.md`](syntax-snapshot.md): `Document::parse`
(`src/syntax.rs`) retains the validated `tree_sitter::Tree` and exposes it
through `Document::root()`. `ScopeGraph::build` already walks every node kind
named below, so a slice-1 control-flow model introduces **no new skipped kind**
and can reuse the existing `ScopeGraph::is_complete()` gate.

Statements are the *named children* of a block node. The grammar builds them
from `_block_item`: `attribute`, `var_declaration`, `constant_declaration`,
`set_statement`, `function_definition`, `type_definition` and `_expression`.

| CST node | fields the model needs | can hold several statements? |
|---|---|---|
| `brace_block` | `{ … }` | yes |
| `colon_block` | `: INDENT … DEDENT` | yes |
| `indented_block` | function/method body after `=` | yes |
| `dot_block` | a single `_expression` | **no** — never has a sibling, so it can never contain an unreachable statement |
| `source_file` | file scope statements | yes |

Terminator-shaped nodes the walker already classifies:

| CST node | fields | note |
|---|---|---|
| `return_expression` | optional `value` | leaves the enclosing function |
| `break_expression` | optional `value` | leaves the enclosing loop/for body |
| `yield_expression` | optional `value` | **not** a terminator here: it resumes, so the next statement runs |

Nesting nodes the model must descend through are the same ones the walker
already handles: `if_expression`, `for_expression`, `loop_expression`,
`block_expression`, `sync_expression`, `race_expression`, `branch_expression`,
`when_expression`, `case_expression`, `spawn_expression`.

### The fact that shapes the whole contract

Newlines and whitespace are insignificant *extras* in the pinned grammar, and
there is no statement separator token. The adapter's adjacent-statement guard
(`check_adjacent_statements` in `src/syntax.rs`) only rejects two top-level
statements that *share a line*; it does not require a terminator between
statements on different lines.

The consequence, confirmed by parsing the snippets in
[Appendix A](#appendix-a-observed-cst-shapes), is that a **bare** `return` or
`break` swallows the next expression as its own `value`:

```text
F():void =
    return
    Print("after")
```

parses as one `return_expression` whose `value` is `Print("after")` — not as a
statement following a terminator. The same is true of a bare `break` in a loop
body. So the shape the issue describes ("a statement directly after
`return`/`break`") **only exists in the CST when the terminator already carries
its own `value` on its line**, or when a second terminator follows. A
bare-terminator-then-declaration text (`return` … `X := 1`) is refused by the
grammar outright (`unsupported or incomplete Verse syntax`, exit 2), because a
declaration is not an expression and cannot become a `value`.

That is why the conservative contract below is phrased in terms of the *CST
shape* rather than the prose "a statement after `return`": denying a shape the
grammar cannot produce is safer than a rule that silently never fires.

## What the snapshot does not cover

These constructs would each need **owner-approved snapshot expansion**, which is
out of scope for this plan and excluded from the first slice. Listing them is the
deliverable, not proposing it:

| construct | current behaviour | why a control-flow model would want it |
|---|---|---|
| `defer:` blocks | no grammar rule at all: every `defer` is exit 2 | `defer` runs at scope exit, so it is the counter-example that must never be reported as unreachable; the model cannot even see it |
| a `case_arm` body holding more than one statement | only a single `_expression` is allowed; `1 => return` followed by `2 => …` is exit 2 | "all arms terminate" analysis is impossible without statement sequences in arms |
| `if (cond) then: <block>` | refused; only `if (cond): <block>` and `if (cond) then <expr>` parse | an `then`-block is another block shape to model |
| `;` as a statement separator, and two same-line top-level statements | refused by `check_adjacent_statements` | separator-aware sequencing would let the model distinguish `return; stmt` from `return stmt` |

Anything on this list stays "needs owner-approved snapshot expansion" and is
excluded from every slice below. A snapshot change also moves `parity.json`,
the vendor hashes and the paired corpus (`docs/syntax-snapshot.md`,
`docs/integration-testing.md`), so it is never a side effect of a rule slice.

## The conservative contract

**Report unreachable code only when the retained CST proves it; otherwise stay
silent.** Nothing here is inferred from text, from the token list or from Verse
semantics the model cannot see.

Reportable shape, all conditions required:

1. The node is a `return_expression` **that has a `value`**, or a
   `break_expression` **that has a `value`**. A value-less terminator is not a
   statement boundary in this grammar (see above), so it is never a proof.
2. The terminator is a named child of a block node (`brace_block`,
   `colon_block`, `indented_block`, `source_file`). A `dot_block` child is its
   only statement, so it is skipped.
3. A later named sibling exists in the *same* block. Statements that follow a
   terminator inside a nested block are **not** reported from the outer block:
   if only one branch returned, the outer continuation is still reachable.
4. For `break_expression`, an enclosing `loop_expression` or `for_expression`
   body is reachable through `parent` links **within the same function**. A
   top-level `break` parses (`Appendix A`, A7) but proves nothing.
5. `attribute` siblings are treated as part of the declaration they annotate, not
   as an independent statement: the finding for `return X` … `@editable` …
   `Z := 1` is anchored at the declaration, never at the attribute line.

Silence is the answer for everything else: `yield`, `when`/`branch`/`sync`/
`race`/`spawn` bodies, arms of `case`, `loop` bodies without an enclosing
terminator, `if` consequences and alternatives when only one of them terminates,
and any statement whose shape the retained CST cannot classify.

The completeness contract is the same one V3001, V3002 and V3003 obey:
`ScopeGraph::reportable()` is the only constructor of a `Reportable` graph, so an
incomplete graph produces **no diagnostic and no execution error** — the
documented no-op described in [`docs/cli.md`](cli.md),
"V3001, V3002 and V3003 on an incomplete graph". A control-flow query must be
reached through that same `Some` arm in `src/rules.rs`, and its result must be
empty on an incomplete graph, re-checking `is_complete()` internally exactly like
`unused()`, `unresolved()` and `shadowing()` do today. `--require-complete-semantics`
already turns that silence into exit 2 for a strict project; V3004 must inherit
that behaviour rather than invent a second switch.

No safe fix. Deleting statements is a semantic change a file-local byte edit
cannot verify, so the rule would be `fixable: false` like every other semantic
rule.

## Data-model sketch

Prose and type names only; nothing here is implemented. The model is a second
pass over the tree that `Document::root()` already returned, ordered by
`Vec` indices and never by a hashed container, so the same source yields the
same indices (`--dump-scopes` determinism relies on that today).

```text
BlockId, StatementId, TerminatorId: usize indices into Vecs

Block {
    kind: Brace | Colon | Indented | File,   // dot_block is not modelled: it has one statement
    parent: Option<BlockId>,
    statements: Vec<StatementId>,            // in source order, attributes folded in
}

Statement {
    block: BlockId,
    range: Range<usize>,                     // anchored at the statement start, BOM included
    // Only what the proof needs: whether this statement is a terminator and
    // whether it already owns a value. No expression kinds, no types.
    terminator: Option<TerminatorKind>,
}

TerminatorKind = Return | Break                  // yield is deliberately absent

Unreachable {
    block: BlockId,
    statement: StatementId,                  // first unreachable statement after the terminator
    proven_by: TerminatorId,
    range: Range<usize>,
}

ControlFlow {
    blocks: Vec<Block>,
    statements: Vec<Statement>,
    // The query the rule layer would call, mirroring the existing three.
    fn unreachable(&self) -> Vec<&Unreachable>;   // empty when the graph is incomplete
}
```

Bounds: the model adds three `Vec`s over nodes the parse already bounded (CST
depth 512, one million fingerprint events, 250,000 tokens). Any new explicit cap
(suggested: 10,000 blocks, 100,000 statements, 250,000 terminators, matching the
scope-graph caps) must be added to `limits::LIMITS` **and** to the "Limits:" list
of `docs/cli.md` or `docs/parser-design.md`, because
`src/doc-drift-tests.rs` fails a constant that no document advertises. An overrun
fails the file closed (exit 2), never truncates a proof.

Deliberate omissions: no expression evaluation order, no `then`/`else` branch
joining, no condition values, no loop trip counts, no effect or failable-call
reasoning, and no cross-block propagation in the first slice. Those are what
later slices (below) buy, each with its own false-positive argument.

## Coverage and the diagnostic surface

`--semantic-coverage` is a *stable* contract (`docs/coverage.schema.json`,
`COVERAGE_SCHEMA_VERSION` in `src/semantic.rs`), built by
`FileCoverage::new` and `CoverageSummary::from_files`, and printed by
`dump::run`. A control-flow model should be measurable the same way, as an
**addition** to that document:

```json
{
  "controlFlow": {
    "blocksModeled": 12,
    "terminatorsByKind": { "return": 3, "break": 1 },
    "unreachableStatements": 2,
    "complete": true
  }
}
```

`complete: false` means the model met a construct it could not classify; the
rule must then report nothing, exactly as an incomplete scope graph does today.
An incomplete count is information, not a failure: the run still exits 0, and
`--require-complete-semantics` remains the only switch that turns it into exit 2.

Adding this block changes the published `coverage.schema.json` contract, so it
needs an owner decision (D2): either a `schemaVersion` bump to 2 with a
`docs/cli.md` and `docs/coverage.schema.json` update, or a separate
`--control-flow-coverage` surface that leaves version 1 untouched. Both are
written here as options; neither is chosen.

`--dump-scopes` needs no new decision: it is already documented as an unstable
diagnostic surface in [`docs/semantic-model.md`](semantic-model.md) and
[`docs/cli.md`](cli.md). Its stability note extends by naming one more per-file
section, `controlFlow` (blocks, terminators by kind, and the unreachable
statements with their byte ranges), and by restating that this, too, may change
without notice and without a schema-version bump. `SCHEMA_VERSION` in
`src/semantic.rs` stays the "unstable" version; only the coverage document
carries a stability promise.

## Slices for later waves

Each slice is one PR, cannot start without the owner's D1 answer, and keeps the
rule's silence behavior as its primary acceptance test.

| # | slice | size | acceptance sketch | false-positive risk |
|---|---|---|---|---|
| C1 | Shadow model only: `src/control_flow.rs` with `Block`/`Statement`/`Terminator`/`unreachable()`, unit tests over the Appendix A shapes. No registry entry, no flag, no output surface. | S | the five conditions of the contract hold; `ScopeGraph::is_complete() == false` yields an empty result; determinism across repeated builds | low: nothing user-visible ships |
| C2 | Register V3004 `unreachable-code`: one `REGISTRY` entry appended last (SARIF `ruleIndex` stability, see the note above `REGISTRY`), `docs/rules/v3004.md`, one `docs/rules.md` row, CLI/README mention. | M | one finding per proven statement; suppression, `--select V3`, severity override and `--show-suppressed` work through the shared machinery; exit 0 on an incomplete graph; baseline parity shows only the documented SARIF driver-rule exception | medium: a statement that looks dead but is a `value` of a terminator would be misreported if condition 1 is ever weakened |
| C3 | Coverage and dump: `controlFlow` in `--semantic-coverage` and `--dump-scopes` per D2's outcome. | M | schema and doc-drift tests pass; counts sum to the per-file totals; low coverage still exits 0 | low |
| C4 | Branch joining: an `if` whose consequence **and** alternative both terminate makes the following sibling unreachable; `else if` chains are read as the repeated `condition`/`consequence` fields one `if_expression` already carries. | L | every-arms-terminate only; a missing `else` reports nothing; `case` stays out | high: `if` conditions can be failable/`option`-shaped, and an arm that "returns" through a `when`/`branch` body is not a terminator |
| C5 | Optional `defer` awareness (only if D5-approved snapshot expansion lands): statements after a `defer:` block are reachable, and `defer` bodies must never inherit a terminator proof. | S | refused today, so this slice is blocked on the snapshot | medium: wrong `defer` scope would report live cleanup code |

Ordering is C1 → C2, with C3/C4 independent and C5 last. Nothing in C1–C3 needs
the grammar to change, which is the point of the contract above.

## Decisions that need the owner

- **D1 — build V3004 at all?** The plan is the gate `docs/semantic-model.md` asks
  for. Approving C1+C2 is what authorizes the rule; approving only C1 authorizes
  a model with no output.
- **D2 — coverage contract:** bump `coverage.schema.json` to version 2 and add
  `controlFlow`, or add a separate surface that keeps version 1 stable?
- **D3 — finding granularity:** one finding per unreachable statement (proposed,
  matches the 10,000-diagnostic budget and suppression per line) or one per
  unreachable run with a range spanning it?
- **D4 — `break` scope:** require an enclosing `loop`/`for` body within the
  function (proposed) or accept a top-level `break` as a proof?
- **D5 — snapshot expansion:** is any of `defer`, multi-statement `case_arm`
  bodies, `if … then: <block>` or `;` separators worth an owner-approved
  snapshot/parity slice? None is assumed here.
- **D6 — policy:** opt-in `warning`, not fixable, off by default (proposed), with
  `--require-complete-semantics` as the only strict switch.
- **D7 — attributes:** anchor the finding at the annotated declaration (proposed)
  or at the attribute line?

## Not doing

- No type inference, no effect inference, no failable-call validity.
- No cross-file analysis, no module resolution, no export graph.
- No Verse/UEFN API database.
- No grammar, scanner or snapshot change; no `parity.json` or corpus change.
- No new CLI flag, config key, suppression form or `--fix` support: selection,
  severity, suppression and JSON/SARIF come from the existing machinery.
- No rule, registry entry or `docs/rules/v3004.md` in this plan's PR.
- No claim that UEFN would accept or reject the code being reported; the tool
  only knows the retained CST.
- No dead-code claims from `when`, `branch`, `sync`, `race`, `spawn`, `case`
  arms, `yield`, `defer` or a loop that never terminates.

## Appendix A: observed CST shapes

Produced while writing this plan by parsing each snippet with the pinned
snapshot (`Document::parse`, `Document::root().to_sexp()`), through a temporary
probe that was removed before this PR; no source file changed. Shapes omit
`function_definition`/`return_type` detail.

| # | snippet (inside a `F():void =` body) | observed shape | consequence |
|---|---|---|---|
| A1 | `return X` / `Print("after")` | `(indented_block (return_expression value: (identifier)) (call_expression …))` | reportable: terminator has a value, sibling follows |
| A2 | `return` / `Print("after")` | `(indented_block (return_expression value: (call_expression …)))` | not reportable: the call is the return value |
| A3 | `return` / `X := 1` | exit 2, `unsupported or incomplete Verse syntax` | bare terminator never precedes a declaration |
| A4 | `loop:` / `break X` / `Print("a")` | `(colon_block (break_expression value: (identifier)) (call_expression …))` | reportable: break in a loop body |
| A5 | `if (true):` / `return X` / `Print("after")` (outer sibling) | `(indented_block (if_expression …) (call_expression …))` | not reportable: only one branch terminated |
| A6 | `return X` / `@editable` / `Z := 1` | `(indented_block (return_expression value: …) (attribute name: (identifier)) (type_definition …))` | attribute folds into the annotated declaration (D7) |
| A7 | `F():void =` / `break` / `Print("a")` | `(indented_block (break_expression value: (call_expression …)))` | top-level break parses; needs the enclosing-loop guard (D4) |
| A8 | `defer:` / `Print("d")` | exit 2, no `defer` grammar rule | needs owner-approved snapshot expansion |
| A9 | `case (X):` / `1 => return` / `2 => Print("b")` | exit 2 | needs owner-approved snapshot expansion |
| A10 | `if (true) then:` / `return` / `Print("after")` | exit 2 | needs owner-approved snapshot expansion |
| A11 | `if (true):` / `return;` / `Print("after")` | exit 2 | `;` handling needs owner-approved snapshot expansion |
| A12 | `if (true):` … `else if (false):` … `else:` | one `if_expression` with repeated `condition`/`consequence` fields and one `alternative` | C4 must read repeated fields, not assume one pair |
| A13 | `loop. Print("a")` | `(dot_block (call_expression …))` | a dot block has one statement, so no unreachable statement inside it |
| A14 | `loop:` / `if (true):` / `break` / `Print("a")` / `Print("outer")` | the bare `break` takes `Print("a")` as its `value` inside the `if` consequence block; `Print("outer")` is a sibling in the loop body | only a terminator with a later sibling **in its own block** is a proof; a nested block's terminator says nothing about the enclosing block |

## References

- [Scope graph and semantic model](semantic-model.md)
- [Independent syntax snapshot](syntax-snapshot.md)
- [Parser design](parser-design.md)
- [CLI, configuration and JSON](cli.md)
- [Rules](rules.md)
