# Scope graph and semantic model

This is the canonical description of the semantic layer verse-lint builds on top
of its retained CST. It covers what a scope is, what a binding is, which CST
nodes are modelled, what `isComplete: false` means, and the invariant every
semantic rule must obey. It does not describe a lint rule: this layer adds none.

## Why the tree is retained

`syntax::Document` used to parse the CST, compute the `shape` fingerprint and
drop the tree. Every rule therefore worked from the flat token list in
`src/lex.rs`, and no rule could ask what an identifier refers to. `Document` now
keeps the parsed `tree_sitter::Tree` it already validated, so `src/semantic.rs`
can walk it.

Retaining the tree changes **no** guard. `Document::parse` keeps its signature
and its failure modes: the lossless lexical scan, the coverage validation, the
adjacent-statement guard, the depth/token/parse budgets, the exact
block-comment boundary match and the `shape` fingerprint all still run, in the
same order, and still fail closed. The `Tree` is owned and lifetime-free, so it
costs no signature change. See [parser design](parser-design.md) and
[syntax snapshot](syntax-snapshot.md).

The claim that rule output is unchanged is enforced, not asserted:
`scripts/verify-baseline-parity.ps1` compares the pre-slice binary against HEAD
byte-for-byte over the fixture corpus in text, JSON and SARIF, and
`scripts/verify.ps1` fails when any byte differs. See
[baseline output parity](parser-design.md#baseline-output-parity).

What retention costs: one CST per source is held for the duration of that
source's inspection instead of being freed after fingerprinting. The bounds are
the existing ones — 8 MiB per source, CST depth 512, 1,000,000 fingerprint
events, 250,000 lexical tokens — so the retained tree is bounded by the same
limits that already gated the parse. Nothing else is kept.

## Scopes

A scope is an owning region. Every scope has a kind and a parent link; only the
file scope has no parent.

| kind | CST node it comes from |
|---|---|
| `file` | `source_file` |
| `module` | `module_definition` |
| `class` | `class_definition`, `struct_definition`, `interface_definition` |
| `function` | `function_definition`, `extension_function_definition` |
| `block` | `block_expression`, `brace_block`, `colon_block`, `indented_block`, `dot_block` |
| `for` | `for_expression` |
| `if` | `if_expression` |

Scopes are stored in creation order, which is the order a single depth-first
walk of the CST reaches them. A `class` scope is shared by the type's fields and
members; function signature parts that cannot see the parameters (specifiers,
effect specifiers, return type) are walked in the *enclosing* scope, and a
class base type is evaluated in the enclosing scope too, because a base name is
not a member of the type it derives from.

## Bindings

A binding is a declaration. It records its name, the scope that owns it, the
byte range of the declaration name (BOM included), a kind, and a `mutable` flag.

| kind | CST node |
|---|---|
| `variable` | `var_declaration` (`mutable: true`) |
| `constant` | `constant_declaration`, `field_declaration`, and the immutable `Name := expression` form of `type_definition` |
| `parameter` | `named_parameter`, `positional_parameter`, `receiver_parameter` |
| `function` | the `name` of `function_definition` / `extension_function_definition` |
| `class` / `struct` / `interface` / `enum` | the `name` of a `type_definition` whose value is that definition |
| `moduleImport` | the last segment of `local_module_path` inside `using_declaration` |
| `forBinding` | a `for_iterator` `key` / `value` / `variable` |

`typeAlias` exists in the enum but is currently unreachable: the pinned grammar
represents both `Name := class` and an immutable value binding as
`type_definition`, and nothing in the CST distinguishes a real alias. Rather
than mislabel one, the model binds the `constant` kind and leaves the alias
variant reserved.

A declaration whose name is not a plain `identifier` — a scoped name, an
operator function name — is **not** silently reclassified; it is recorded as an
unsupported kind, which makes the graph incomplete.

## References and resolution

A reference is an `identifier` occurrence that is not itself a declaration name.
It records its name, byte range and the scope it appears in. Two identifier
positions are deliberately *not* references: the `name` of a declaration (it is
the binding) and the attribute name (it is an annotation keyword).

`resolve()` walks from the reference's scope outward through `parent` links. In
each scope the **last** declared binding with that name wins, and a binding only
counts once its declaration starts before the reference. That gives three
properties worth stating because rules will depend on them:

- an inner block shadows an outer binding only from its declaration onward;
- a name used before a redeclaration keeps the outer binding;
- a name that is not visible anywhere returns `None`.

`None` means *unresolved*, not *does not exist*. An API bucket, a module
package path, or another file is invisible to this model. Resolution is
deterministic: it reads ordered `Vec`s only, never a `HashMap`, so the same
source always yields the same graph and the same indices.

`self.Field` is resolved against the enclosing class scope. Member lookup starts
**at the class scope**, not at the reference's lexical scope, so a same-named
local in an inner block cannot capture it, and a field is visible from anywhere
in the type even when it is declared after the method that reads it — member
lookup is not a forward-only source scan. The reference still records the
lexically enclosing scope, plus the class it was resolved through, so a rule can
tell both where the reference sits and what it names. `self` outside a class is
recorded as unsupported rather than resolved against an unrelated local. Any
other `object.Member` is *not* guessed — the member belongs to a type this model
cannot see, so the construct is recorded as unsupported instead.

## Incompleteness and the invariant

> A semantic rule may report only from a graph it can prove complete.

The walker does not quietly skip a node it cannot classify. It counts skips by
CST node kind in a bounded map (at most 64 kinds; overflow folds into `other`,
so the map stays bounded without ever hiding that unsupported constructs exist).
If that map is non-empty, `is_complete()` is `false`.

Constructs recorded as unsupported today:

| node kind | why it is not modelled |
|---|---|
| `member_access` on a non-`self` object | the member belongs to an unknown type |
| `field_initializer` | object-construction field names target an unknown type |
| `named_argument` | argument names are not binding positions |
| `case_arm` | patterns match names this model does not bind |
| `where_clause`, `constrained_type` | generic/constraint names are outside the working set |
| `module_scoped_identifier`, `qualified_type` | cross-file scope is out of scope |
| `module_path` | a package path binds names this model cannot see |
| `enum_variant` | variants are enum members, not bound in the working set |
| `local_module_path` with more than one segment | the enclosing module is unresolvable |
| any unrecognised node kind | fail closed rather than guess |

A semantic rule must obtain its graph through `ScopeGraph::reportable()`, which
returns `None` for an incomplete graph. `None` is a documented no-op: the rule
emits nothing — not a diagnostic, not a warning, not an execution error. This is
the whole point of the layer. Emitting "name is unused" from a graph that could
not see a use would instruct a user to delete live code, which is worse than
silence.

Because every recorded skip clears `is_complete()`, a grammar shape added in a
future slice cannot silently start producing confident findings: it makes the
file unreportable until the model is taught to classify it.

## The `--dump-scopes` diagnostic surface

`verse-lint --dump-scopes <path>` prints the graph as JSON and exits 0 without
running any rule. Output is versioned (`schemaVersion`), deterministic, and
contains scopes (kind, parent), bindings (name, kind, scope, byte range,
mutable), references (name, byte range, scope, resolved binding index or null),
and the skipped-kind counts with `isComplete`. See [CLI](cli.md).

**`--dump-scopes` is an unstable diagnostic surface for rule authors, not a
stable public contract.** Its shape may change without notice and without a
schema-version bump in a release note. Do not build tooling against it.

The path changes no guard. It reads each source under the same byte and total
limits as a lint run, requires the same `Document` coverage validation, and
fails closed — exit 2, no graph — on inputs the linter already refuses to
inspect. A refused file never contributes a partial entry that could look
complete.

## Non-goals

- No type inference, effect inference, or cross-file resolution.
- No rule in this layer; V3001 and its siblings come later and must all consume
  this graph through `reportable()`.
- No replacement of the lossless lexer or of strict coverage validation with
  tree-sitter error recovery.
- Coverage counts describe the *tool's* coverage. They are not a code-quality
  score and must not be presented as one.

## References

- [Parser design](parser-design.md)
- [Syntax snapshot](syntax-snapshot.md)
- [CLI, configuration and JSON](cli.md)
