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

Select complete IDs only. CLI selections replace config selections; ignores
then subtract from them. Warnings exit 0 unless `--deny-warnings` is set.
Execution errors always exit 2 and cannot be suppressed. See
[suppression syntax](suppressions.md), [configuration](cli.md) and [fixes](fixes.md).
