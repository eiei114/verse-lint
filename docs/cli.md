# CLI, configuration and JSON

Shared toolchain contract version **1**. Seven rules and
[suppression](suppressions.md) are implemented. Defaults are V1001/V1002/V1003;
V2001/V2002 are opt-in warnings, and V3001/V3002 are opt-in semantic rules that
read the [scope graph](semantic-model.md). `--fix` applies only validated V1001/V1002
edits using guarded Windows replacement; see [fixes](fixes.md). [SARIF
2.1.0](sarif.md) is supported and schema-validated, and `--output-format github`
emits GitHub Actions annotations for a pull-request job. Compiler/UEFN acceptance
is not established.

`verse-lint` defaults to CWD; file and directory paths can be combined. `-` must
be used alone; `--stdin-filepath` is a virtual Unicode path, not a write target.
`--select V1001` replaces configured selection; `--ignore V1001` replaces
configured ignore, then subtracts from selection. Each rule has a registry
default severity; `[lint.severity]` overrides it per project.
`--deny-warnings` promotes remaining warnings to exit 1 without changing severity.
`--output-format text|json|sarif|github` selects the diagnostic format; see
[GitHub annotations](#github-annotations) for the fourth.
`--show-suppressed` also lists findings a suppression directive removed; see
[suppressed findings](#suppressed-findings). `--no-ignore-file` skips
`verse-lint.ignore` for one run; see [ignore file](#ignore-file).
`--fix --diff` previews the safe edits a write run would make and writes
nothing; see [fix preview](#fix-preview-with---fix---diff).

### Rule selection and family prefixes

`select`/`ignore` entries — on the command line and in the `[lint]` arrays — are
either a complete rule ID (`V1001`) or a rule-family prefix: `V` followed by zero
or more ASCII digits (`V`, `V1`, `V2`). A prefix expands to every registered rule
whose ID starts with it, so `--select V1` is the V1001/V1002/V1003 family and
`--select V` is the whole registry. Expansion reads the live registry, so a rule
added in a later release becomes reachable through an existing prefix without
editing the selection.

Both lists are expanded before subtraction, so `--select V --ignore V2` keeps the
V1 family: an `ignore` prefix removes the rules it names, never a literal string.
Duplicates and overlapping entries collapse to one effective set, which is still
ordered by ID. `--show-config` always prints the expanded complete IDs, never a
prefix, so the effective set is unambiguous.

Anything that cannot be resolved is a configuration error with exit 2 before any
source is inspected: a prefix matching zero registered rules (`V9`) names the
prefix and that no registered rule matched; a malformed entry (lowercase `v1`,
`V1001x`, a bare number `1001`, `--select ""` or a trailing comma) names the
offending entry and the accepted forms. A typo is never silently ignored, and an
effective set that subtracts to zero rules is still an error. Prefixes are
accepted only for `select`/`ignore`: `[lint.severity]` keys and
[suppression](suppressions.md) directives keep requiring complete IDs.

## Configuration

```toml
schema-version = 1
[files]
exclude = ["Vendor/**"]
[lint]
select = ["V1001", "V1002", "V1003", "V2001"] # V2001 also has to be selected
ignore = []
max-line-length = 120 # used only when V2001 is selected
external-names = [] # known external names V3002 should leave alone
[lint.severity]
V2001 = "error"
V1001 = "warning"
```

`--config` wins. Otherwise search CWD upwards, including but not crossing the
first `.git` boundary. With virtual stdin, start at the virtual parent. One
config per invocation, no nested/global merging. Exclude/label root is config
parent or CWD. Unknown top-level, files, lint and `[lint.severity]` keys fail;
sibling `[format]` is required to be a table but its own keys/values are
deliberately not validated. `max-line-length` must be 1..1,000,000.
`--show-config` shows effective CLI rule overrides, the expanded complete rule
IDs behind `select`/`ignore`, the effective severity of every registered rule,
configured `external-names`, config source and root without inspecting inputs.

### `[lint] unused-parameters`

`unused-parameters` is a boolean, `false` by default, and it changes only
[V3001](rules/v3001.md): with `false` an unused function parameter is never
reported, and with `true` it is reported under the same exclusions as an unused
local. `_`-prefixed parameters are never reported in either mode. The switch does
nothing unless V3001 is selected. `--show-config` prints the resolved value
(`unused-parameters = false`).

### `[lint] external-names`

`external-names` is a list of exact, case-sensitive strings, empty by default.
It only affects [V3002](rules/v3002.md), and only when the name is otherwise
unresolved in a complete local scope graph. Add identifiers supplied by
another file or an API that this linter cannot model:

```toml
[lint]
select = ["V3002"]
external-names = ["ProjectApi", "ImportedHelper"]
```

V3002 has no Verse/UEFN API database. **Start with a report-only job using
`--select V3002`**, review its results, and add confirmed external names here;
do not assume that a V3002 result proves the compiler will reject the name. The
short built-in allowlist is documented in [the V3002 rule page](rules/v3002.md).

### V3001 and V3002 on an incomplete graph

Both semantic rules report nothing at all for a file whose graph is incomplete
— no diagnostic, no execution error, exit 0 for that file — under every
configuration, including `--select V3001`, `--select V3002`, `--select V`, and
`unused-parameters = true`. Incompleteness is the model's, not the file's
cleanliness: `--dump-scopes` shows it as `isComplete: false` together with the
unsupported CST kinds. The alternative — claiming a declaration is unused or a
reference unresolved when the graph could not see all names — risks deleting or
renaming live code, so silence is the documented behavior rather than a gap.
See [semantic model](semantic-model.md), [V3001](rules/v3001.md), and
[V3002](rules/v3002.md).

`[lint.severity]` promotes or demotes an already-registered rule for this
project; it never adds a rule. Keys must be complete rule IDs that appear in the
registry — an unknown or abbreviated ID (`V1`, `V9999`) is a configuration error.
Unlike `select`/`ignore`, a prefix such as `V1` is never expanded here: a severity
override has to name one rule.
Values must be the exact lowercase literals
`"error"` or `"warning"`; anything else, including `"Error"`, an empty string or
a non-string TOML value, is a configuration error that names the offending key.
Both fail the run with exit 2 before any source is inspected, so stdout carries
no diagnostics.

The override changes only how a finding is reported. Selection stays independent:
an overridden rule that is not selected still produces nothing, and a selected
rule that is not overridden keeps its registry severity. Suppression directives,
`--deny-warnings` and safe fixes behave as before. Text output keeps the
`path:line:column: severity rule-id message` shape, and JSON `errors`/`warnings`,
SARIF `results[].level` and the SARIF run summary all count the effective
severity. A `github` annotation follows it too: `::error` for error severity and
`::warning` for warning severity. SARIF `tool.driver.rules[].defaultConfiguration.level` deliberately
keeps the registry default: it documents the tool default, not this project's
policy.

Local parent/nested `.gitignore` rules and negation apply during discovery;
global Git ignores do not. Hidden entries, `.git`, `Saved`, `Intermediate` and
`*.digest.verse` are skipped. An explicit file bypasses gitignore/hidden rules,
not generated-file or config exclusion protections. Recursive links/junctions
are skipped; explicit reparse paths or parents fail. Repeated canonical paths
are deduplicated (including tested Windows case aliases).
Distinct hard-link paths are separate read-only inputs; `--fix` refuses hard-linked
sources before writing.
There is no CLI glob expansion. Zero sources is exit 2, not successful inspection.
`--verbose` explains exclusions on stderr, naming the source that excluded each
path (`verse.toml`, `verse-lint.ignore` or `.gitignore`).

### Ignore file

`verse-lint.ignore` is one shared exclusion list for a project that needs the same
skips in several repositories. It is read from **one** location: the directory
owning the resolved `verse.toml`, or CWD when no configuration was found. There is
no parent-directory search, no nested per-subdirectory ignore file and no
user-level ignore file.

```text
# generated output and vendored dependencies
Content/Generated/**
Vendor/**
Saved_*/Intermediate/**
```

One relative forward-slash glob per line. A blank line, and a line whose first
non-blank character is `#`, is skipped; each entry is trimmed. A `#` that is not
leading belongs to the pattern, so `build#1.verse` is a pattern, not a comment.
Accepted entries follow exactly the `[files] exclude` grammar, so the same forms
are rejected as a configuration error with exit 2 and the offending line number:

- negation (`!name.verse`)
- an absolute path (`/Content/**`)
- a drive letter or colon (`C:/project/**`)
- a backslash (`Content\**`)
- an entry longer than 4,096 bytes

The file may hold at most 256 effective patterns and be at most 256 KiB; comments
and blank lines do not consume the pattern budget. Both numbers are
machine-checked in [Limits](#output-contract) below. Exceeding either is exit 2,
reported before any source is inspected and before any fix is written.

`[files] exclude` and `verse-lint.ignore` combine as a union: a path either source
matches is skipped. Neither overrides the other, and there is no "last match
wins" rule. An explicit file argument bypasses `verse-lint.ignore` exactly as it
bypasses `[files] exclude`; generated-file protections (`Saved`, `Intermediate`,
`*.digest.verse`, `.git`) stay non-bypassable. A missing ignore file is not an
error; an unreadable one is exit 2 with a message naming the path.

`--no-ignore-file` disables the file for one invocation without touching it, which
restores the behaviour of a directory that has none. `--show-config` reports in a
header comment whether an ignore file was found, its path, and the merged
effective pattern count (its own patterns plus `[files] exclude`), so the number
that actually excludes is visible in one place. `--verbose` keeps the
attribution per path, so `verse.toml`, `verse-lint.ignore` and `.gitignore`
exclusions remain distinguishable.

## Suppressed findings

Suppression directives ([suppressions](suppressions.md)) remove findings from
ordinary output. `--show-suppressed` lists them again, so a suppression written
for code that has since changed can be reviewed without editing the source first.
The flag changes reporting only:

- Without it, output is exactly as before: suppressed findings are not printed
  and `summary.suppressed` is their only trace.
- With it, suppressed findings are reported alongside active ones, marked as
  suppressed, and sorted by the same path/byte-offset/rule-ID key, so a mixed
  list still reads in source order.
- Text output appends ` (suppressed)` after the message. The marker is stable: no
  rule message ends with it, so a line without it is an active finding.
- JSON sets the per-diagnostic `suppressed` boolean, present on every diagnostic
  in every run. `summary.suppressed` keeps its meaning and still counts these
  findings; `errors`, `warnings` and SARIF `results` count only active ones.
- [GitHub annotations](#github-annotations) report a shown suppressed finding as
  `::notice`, which cannot fail a check run or change the exit code.
- SARIF omits suppressed findings from `results` (see [SARIF](sarif.md)); the run
  summary still reports the `suppressed` count.

Suppressed findings never change the exit code, with or without the flag, and
`--deny-warnings` ignores them. They still count toward the 10,000-diagnostic
budget, as before. `--show-suppressed --show-config` is rejected like every other
`--show-config` conflict (exit 2).

With `--fix`, a suppressed finding is reported at its position in the **saved**
source, consistent with how active diagnostics are reported after a successful
write. A file the run could not save keeps inspected-original positions, which
the execution error states explicitly; the same applies without the flag.

The JSON addition is additive: existing fields keep their meaning and the new
boolean is always present, so a consumer that ignores unknown fields keeps
working. `schemaVersion` therefore stays `1`, and
[diagnostics.schema.json](diagnostics.schema.json) documents `suppressed` as
required in the same release.

## Fix preview with `--fix --diff`

`--fix --diff` runs the complete safe-fix pipeline — candidate collection, span
and hash validation, reparse, token/structure comparison and one-pass convergence
— and then prints the planned change instead of writing it. No byte of any file
changes, `summary.filesChanged` stays `0`, and the plan is the same one a write
run would apply. See [safe fixes](fixes.md#preview-with---fix---diff) for the
validation contract and the display-only normalisations.

```text
verse-lint Content --fix --diff
```

```diff
--- a/Content/device.verse
+++ b/Content/device.verse
@@ -1,3 +1,3 @@
 A := 1
-B := 2  
+B := 2
 C := 3
```

stdout carries the unified diff and nothing else; the text diagnostics for
findings that survive the plan move to stderr in this mode, so stdout is never
two formats at once. The exit code is the ordinary one: remaining errors exit 1,
incomplete inspection exits 2, and a failed plan prints no partial diff. Output
is line-oriented with 3 lines of context, one `--- a/<path>` / `+++ b/<path>`
pair per changed file, `/`-separated config-root-relative paths, and no
timestamps or index hashes. A CRLF source prints without its CR and a leading BOM
is not printed; both stay in the bytes a real `--fix` writes, and every other
control character except TAB is escaped as `\u{XXXX}`.

Six combinations are rejected with exit 2 before any source is analysed, and
none of them changes a file:

| Combination | Why |
| --- | --- |
| `--diff` without `--fix` | the preview renders the fix plan, so there is nothing to preview |
| `--fix --diff` with stdin | `--fix` already refuses stdin, which is not a write target |
| `--fix --diff --output-format json` | the diff has no JSON representation in this release |
| `--fix --diff --output-format sarif` | same: one format on stdout |
| `--fix --diff --output-format github` | same: one format on stdout |
| `--fix --diff --show-config` | `--show-config` is not a source run |

## Output contract

Text stdout: `path:line:column: severity rule-id message`, no success banners.
Execution errors go to stderr. `--color auto|always|never` affects text only;
auto respects terminal detection and `NO_COLOR`.
JSON stdout conforms to [diagnostics.schema.json](diagnostics.schema.json).
`schemaVersion` is 1. Every run contains `tool`, `diagnostics`, `executionErrors`
and `summary`, including recoverable argument/config failures when a valid JSON
output request can be identified. Invalid output format itself uses stderr only.
Help/version bypass config. JSON never contains ANSI color or progress logs.

- Every diagnostic has path, ruleId, severity, message, range, fixable and
  suppressed. `severity` is the effective value after `[lint.severity]`.
  `suppressed` is always present: `false` for a reported finding and `true` for
  a finding `--show-suppressed` listed after a directive removed it.
- Range is half-open. Start/end contain zero-based UTF-8 byteOffset including
  BOM, and one-based line/Unicode-scalar column excluding initial BOM. Columns
  are not visual widths or UTF-16 units. CRLF counts as one line ending.
- `fixable` marks the rule's safe-edit capability, not that an edit was applied.
  Successful `--fix` reports remaining diagnostics at their saved-source positions.
- Paths use `/`, relative to config root; another Windows volume may require
  an absolute path. Stdin uses its virtual path or `<stdin>`, never a fake file.
- Diagnostics sort by path, byte offset, then rule ID. No timestamps or unstable
  run IDs. Same arguments/input yield identical JSON apart from external errors.
- Execution errors contain a message and optional path. They are not lint rules.
- `filesChecked` counts successfully analyzed files, not failed attempts.
  `errors`/`warnings` count active diagnostics only, never suppressed ones.
  `filesChanged` counts successful writes only; `suppressed` counts otherwise
  emitted active-rule diagnostics removed by directives, whether or not
  `--show-suppressed` listed them. Text mode reports nonzero changed/suppressed
  counts on stderr. `complete=false` if any execution error occurred.
- Partial inspection retains diagnostics from successfully analyzed files but
  exits 2. Otherwise errors (or warnings with deny-warnings) exit 1; clean is 0.
  Failure to write stdout also exits 2.

Limits:

- 8 MiB per source file
- 64 MiB total successfully read input
- 10,000 source files
- 100,000 visited entries
- 128 directory levels
- 256 KiB config file
- 256 KiB ignore file
- 256 exclude globs
- 256 ignore patterns
- 4,096 bytes per exclude glob
- 10,000 diagnostics per invocation

The diagnostic count includes suppressed findings. An exceeding file produces an
execution error instead of claiming its partial diagnostics complete. Previously
completed files remain in output. No fixing begins on limit failure. Parser
limits are in [parser-design.md](parser-design.md).

Each bullet above is machine-checked against the constant that enforces it by
`src/doc-drift-tests.rs`, so the prose cannot advertise a number the code no
longer uses. Keep one limit per bullet.

### Limit coverage

Every limit above is covered at its boundary and one step past it by
`tests/limits-cli.rs` and the unit tests in `src/`. Past-limit runs assert exit 2,
an execution-error classification (never a lint rule), `complete: false`, and that
`--fix` wrote nothing. Walk limits also assert that files discovered before the
failure keep their diagnostics.

Two limits are only proven at the predicate level in the ordinary verification
path, because reaching them literally needs a heavy fixture. Both have a full
end-to-end test gated behind `VERSE_LINT_HEAVY_LIMIT_TESTS=1`; see
[integration testing](integration-testing.md).

- 64 MiB total read input: `files::total_bytes_exceeded` is unit tested on both
  sides of the bound, and `tests/limits-cli.rs` gates the 64 MiB aggregate run.
- One million fingerprint events: `syntax::tree_limit` is unit tested on both
  sides of the bound, and `tests/limits-cli.rs` gates the full-size input.

Two walker limits are proven at the walker level rather than end to end, because
building the input would dominate the verification path:

- 100,000 visited entries: `files::MAX_VISITED_ENTRIES` is tested at its boundary
  and one past it through the walker's `visit` entry point, which is the same
  comparison production runs; the deep-nesting cases in `tests/limits-cli.rs`
  prove the shared "directory traversal resource limit" failure end to end.
- 10,000 source files: the walker test proves the 10,000th file is accepted and
  the next refused; a CLI run also covers the refusal with `--fix` in
  `tests/fix-cli.rs`.

One documented limit is **not** testable as written and is recorded here rather
than silently left unverified: the two-second parse budget is enforced by a
wall-clock `progress_callback`. A real parse that exceeds two seconds would make
the test flake on a loaded CI runner, so the budget predicate
(`syntax::parse_budget_exceeded`) and a zero-budget refusal are tested instead,
and the two-second duration itself is asserted as a constant. No test forces a
real parse to take two seconds.

`scripts/verify-json.ps1` validates real release-binary JSON and SARIF for exit
0/1/2, fixes, suppression, Unicode paths, virtual stdin, I/O and argument failure
with PowerShell 7's JSON Schema validator. An invalid version must be rejected.

`verse-lint.ignore` is covered end to end by `tests/ignore-file-cli.rs`: discovery
exclusion, the union with `[files] exclude`, comment/whitespace handling, every
rejected entry form with its line number, both caps at their boundary and one past
it, `--no-ignore-file`, explicit-argument bypass, `--verbose` attribution and
`--show-config` output. The parser and loader are also unit tested in
`src/files.rs`.

## `--dump-scopes` (unstable, for rule authors)

`--dump-scopes` prints the per-file scope graph as JSON to stdout and exits 0
without running any rule. It is a diagnostic surface for people writing rules,
**not a stable public contract**: its shape may change without notice, and
nothing should depend on it programmatically.

```text
verse-lint Content/device.verse --dump-scopes
verse-lint - --dump-scopes < device.verse
```

The document is versioned (`schemaVersion`) and deterministic for the same input.
It contains `files`, each with `path`, `scopes` (kind, parent), `bindings`
(name, kind, scope, byte range, mutable), `references` (name, byte range, scope,
`resolved` as an index into `bindings` or `null`), `skipped` (unsupported CST
kinds, by count) and `isComplete`. Ranges are half-open byte offsets with the
same conventions as diagnostic ranges.

`isComplete: false` means the model could not classify every name-bearing
construct, so a semantic rule must not report from that file. See
[semantic model](semantic-model.md).

`--dump-scopes` conflicts with `--fix` and `--show-config`, and with every
`--output-format` other than the default text setting (naming `text` explicitly
is accepted). Each conflict exits 2. A requested `json` or `sarif` format still
reports the conflict through its normal envelope rather than printing a graph;
`github` reports it on stderr with the text-format wording, like any execution
error.
The flag counts toward the existing per-source byte, nesting and token limits and
fails closed — exit 2 with no graph — on inputs that today fail coverage
validation. It never runs a rule, so it needs no active rule selection.
File discovery is the same as a lint run: `[files] exclude` and
`verse-lint.ignore` apply, and `--no-ignore-file` skips the ignore file.

## GitHub annotations

`--output-format github` turns each reported diagnostic into one
[GitHub Actions workflow command](https://docs.github.com/actions/reference/workflow-commands-for-github-actions)
on stdout, so a pull-request job annotates the diff with no code-scanning setup
and no upload:

```text
::error file=Content/device.verse,line=12,col=7,endLine=12,endColumn=9,title=V1001 TrailingWhitespace::Trailing whitespace
```

```powershell
verse-lint Content --output-format github
```

GitHub shows at most **10 warning and 10 error annotations per step**; further
workflow commands still run but are not attached to the diff. Use
`--output-format json` or `sarif` when you need the complete list. Because the
count is per step, splitting a run does not raise it: select the rules that
matter for the PR job, or keep the annotations for review and the JSON for
reporting.

- Command level follows the finding, not the rule family: `::error` for an
error-severity diagnostic, `::warning` for a warning-severity one.
- Region properties are the same numbers JSON and SARIF report for that input —
one-based `line`/`col`/`endLine`/`endColumn` with Unicode scalar columns, not
visual widths — so an annotation, a JSON `range` and a SARIF `region` describe
the same bytes.
- Paths are the same `/`-separated, config-root-relative paths text and JSON use,
so they line up with SARIF artifacts. Escaping is applied on top; it changes the
annotation only, never the reported path.

  GitHub resolves `file=` against the **repository root**, so an annotation
aligns only when the resolved configuration root is that root — the ordinary
case of a `verse.toml` (or CWD) at the top of the checkout. When the
configuration lives in a subdirectory, the path is still relative to it and
GitHub cannot find the file: for a nested `Content/verse.toml`, a finding in
`Content/Sub/device.verse` is emitted as `file=Sub/device.verse`. This format
does not discover the checkout root to rewrite it, because that would report a
path text and SARIF never emit and this slice adds no repository discovery.
Changing the job's working directory does not help: from `Content` the same
finding is still `file=Sub/device.verse`, because the path stays relative to the
configuration root while GitHub still resolves it against the repository root.
What aligns annotations is making the configuration root the repository root:
keep `verse.toml` at the top of the checkout, or, with no `verse.toml` there, run
from the top of the checkout without `--config` (CWD is then the root; a nested
`Content/verse.toml` is not found by the upward search and does not apply). A run
from a subdirectory with no configuration above it makes that subdirectory the
root and misaligns the same way. JSON and SARIF are unaffected either way: their
paths are explicitly based at the config root.
- Ordering is the shared diagnostic order (path, byte offset, rule ID), so the
same input yields the same bytes.
- Nothing else goes to stdout: no summary line, no banner, no `::group::`,
`::set-output` or `::add-matcher`. Execution errors keep the text-format stderr
wording, and a run with them still exits 2. Exit codes are unchanged: 0 clean,
1 violations (or warnings under `--deny-warnings`), 2 incomplete or
configuration failure.
- `--color` is accepted and inert here. A workflow command never carries ANSI,
so `auto`, `always` and `never` produce identical bytes.
- No network access, no GitHub API call, no token handling and no `GITHUB_*`
sniffing. The format is explicit: there is no `GITHUB_ACTIONS`
auto-detection mode.

Escaping is mandatory, because a raw `%`, CR, LF, `:` or `,` inside a value
would otherwise terminate the property list and silently truncate or
mis-attribute the annotation. It is applied in exactly two places:

| Field | Escaped sequences |
| --- | --- |
| `message` (after `::`) | `%` → `%25`, CR → `%0D`, LF → `%0A` |
| Every property value (`file`, `title`) | the same, plus `:` → `%3A`, `,` → `%2C` |

A message keeps its colons and commas, so ordinary prose is unchanged; a path
containing `,`, `:` or a newline stays one property value. Non-ASCII characters
pass through unchanged — the annotation is UTF-8 and only the five reserved
characters need a sequence. Escaping is lossless and single-pass: an already
escaped `%25` becomes `%2525`, and the runner decodes it back.

Suppressed findings (see [suppressed findings](#suppressed-findings)) are
reported as `::notice`, not `::error`, when `--show-suppressed` asks for them:
GitHub's command vocabulary has no per-finding suppression field, and inventing
a non-standard property would make the annotation depend on a consumer that does
not exist. `::notice` is the documented choice because a notice cannot fail a
check run, which matches the rule that a suppressed finding never changes the
exit code. Without the flag they are not annotated at all.

An example workflow step in your own repository:

```yaml
- name: Lint Verse
  shell: pwsh
  run: ./verse-lint.exe Content --output-format github
```

Run it in the pull-request job that already builds or tests the project. The
step annotates the files in that PR because GitHub associates a workflow command
with the run, not with an uploaded report. This repository ships no such
workflow: see [SARIF](sarif.md#upload-policy-none) and
[ADR 0001](adr/0001-sarif-upload-policy.md) for why no upload path exists.
