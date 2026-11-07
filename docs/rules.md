# Style rules

These are tool policies, not compiler/type/effect diagnostics. Literal/comment
protection and syntax coverage validation apply before any rule runs.

| Rule | Default | Severity | Safe fix |
|---|---|---|---|
| [V1001 trailing-whitespace](rules/v1001.md) | On | error | Remove trivia |
| [V1002 missing-final-newline](rules/v1002.md) | On | error | Add newline |
| [V1003 tab-indentation](rules/v1003.md) | On | error | None |
| [V2001 line-too-long](rules/v2001.md) | Off | warning | None |
| [V2002 bare-todo-comment](rules/v2002.md) | Off | warning | None |
| [V3001 unused-declaration](rules/v3001.md) | Off | warning | None |

`src/rules.rs` `REGISTRY` is the single source of truth for rule ID, SARIF
`name`, severity, message, fixable flag and default selection: this table, the
default `select` list, CLI/config validation and the SARIF driver metadata are
all derived from or checked against it. Detectors dispatch on the `V####` ID
constants (`rules::V1001` …), and SARIF `ruleIndex` is resolved from `ruleId`
when rendering, so reordering `REGISTRY` cannot rebind a finding to another
rule ID. Adding a rule means one registry entry, its `rules/<id>.md` document
and one row here — never a parallel positional array. The severity column is the
registry default; a project may override it without changing this table.

Select a complete ID (`V1001`), or a rule-family prefix of `V` plus digits: `--select
V2` enables every registered V2 rule and `--ignore V2` removes that family. Prefixes
expand over the live registry before subtraction, and `--show-config` prints the
resulting complete IDs. A prefix matching nothing, a malformed entry and an empty
effective set are configuration errors (exit 2). CLI selections replace config
selections; ignores then subtract from them. Warnings exit 0 unless `--deny-warnings`
is set. Execution errors always exit 2 and cannot be suppressed. Prefixes apply
only here — `[lint.severity]` keys and suppression directives still need complete
IDs. See [suppression syntax](suppressions.md), [configuration](cli.md) and
[fixes](fixes.md).

## Per-project severity

The table above is the tool default. A project may promote or demote an already
registered rule through `[lint.severity]` in `verse.toml`:

```toml
[lint]
select = ["V1001", "V1002", "V1003", "V2001"] # V2001 has to be selected as well
[lint.severity]
V2001 = "error"   # opt-in policy rule becomes a hard CI failure
V1001 = "warning" # default error is demoted for this project only
```

Only complete registered IDs are accepted, and only the exact values `"error"`
and `"warning"`; a prefix, an unknown ID, an unknown value or a non-string value
is a configuration error (exit 2) that names the offending key. The override
changes
reported severity — text, JSON `severity`, JSON `errors`/`warnings`, SARIF
`results[].level` and the GitHub annotation command level — and nothing else. It
does not select a rule, suppress a finding, disable a safe fix or bypass
`--deny-warnings`, and SARIF
driver `defaultConfiguration.level` still reports the tool default. `--show-config`
prints the resolved map for every rule in stable ID order.

## Semantic rules

V3001 is the first rule that reads the [scope graph](semantic-model.md) instead of
the token list, so it obeys one rule no style rule needs:

> A semantic rule may report only from a graph it can prove complete.

When a file's model records any construct it could not classify, the graph is
incomplete and **V3001 reports nothing for that file — no diagnostic and no
execution error — under every configuration**. That is a documented no-op, not an
error, because a graph that could not see a use cannot prove a declaration is
unused, and an `unused` claim on live code is worse than silence. The rule still
applies every other policy: severity override, selection and prefixes,
suppression, `--ignore` and `--show-suppressed` work exactly as for a style rule.
Its false-positive surface is the primary design constraint, so it excludes
exported and `_`-prefixed names, `using` imports and `for`/`if` bindings, and does
not report parameters unless `[lint] unused-parameters` turns them on. See
[V3001](rules/v3001.md).
