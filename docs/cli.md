# CLI, configuration and JSON

Shared toolchain contract version **1**. Five [style rules](rules.md) and
[suppression](suppressions.md) are implemented. Defaults are V1001/V1002/V1003;
V2001/V2002 are opt-in warnings. `--fix` applies only validated V1001/V1002 edits
using guarded Windows replacement; see [fixes](fixes.md). [SARIF 2.1.0](sarif.md)
is supported and schema-validated. Compiler/UEFN acceptance is not established.

`verse-lint` defaults to CWD; file and directory paths can be combined. `-` must
be used alone; `--stdin-filepath` is a virtual Unicode path, not a write target.
`--select V1001` replaces configured selection; `--ignore V1001` replaces
configured ignore, then subtracts from selection. Duplicates collapse; unknown
or empty-string IDs, prefixes and zero effective rules fail. Severity is fixed.
`--deny-warnings` promotes remaining warnings to exit 1 without changing severity.

## Configuration

```toml
schema-version = 1
[files]
exclude = ["Vendor/**"]
[lint]
select = ["V1001", "V1002", "V1003"]
ignore = []
max-line-length = 120 # used only when V2001 is selected
```

`--config` wins. Otherwise search CWD upwards, including but not crossing the
first `.git` boundary. With virtual stdin, start at the virtual parent. One
config per invocation, no nested/global merging. Exclude/label root is config
parent or CWD. Unknown top-level, files and lint keys fail; sibling `[format]`
is required to be a table but its own keys/values are deliberately not validated.
`max-line-length` must be 1..1,000,000. `--show-config` shows effective CLI rule
overrides, config source and root without inspecting inputs.

Local parent/nested `.gitignore` rules and negation apply during discovery;
global Git ignores do not. Hidden entries, `.git`, `Saved`, `Intermediate` and
`*.digest.verse` are skipped. An explicit file bypasses gitignore/hidden rules,
not generated-file or config exclusion protections. Recursive links/junctions
are skipped; explicit reparse paths or parents fail. Repeated canonical paths
are deduplicated (including tested Windows case aliases).
Distinct hard-link paths are separate read-only inputs; `--fix` refuses hard-linked
sources before writing.
There is no CLI glob expansion. Zero sources is exit 2, not successful inspection.
`--verbose` explains exclusions on stderr.

## Output contract

Text stdout: `path:line:column: severity rule-id message`, no success banners.
Execution errors go to stderr. `--color auto|always|never` affects text only;
auto respects terminal detection and `NO_COLOR`.

JSON stdout conforms to [diagnostics.schema.json](diagnostics.schema.json).
`schemaVersion` is 1. Every run contains `tool`, `diagnostics`, `executionErrors`
and `summary`, including recoverable argument/config failures when a valid JSON
output request can be identified. Invalid output format itself uses stderr only.
Help/version bypass config. JSON never contains ANSI color or progress logs.

- Every diagnostic has path, ruleId, severity, message, range and fixable.
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
  `errors`/`warnings` count reported diagnostics. `filesChanged` counts successful
  writes only; `suppressed` counts otherwise emitted active-rule diagnostics
  removed by directives. Text mode reports nonzero changed/suppressed counts on
  stderr. `complete=false` if any execution error occurred.
- Partial inspection retains diagnostics from successfully analyzed files but
  exits 2. Otherwise errors (or warnings with deny-warnings) exit 1; clean is 0.
  Failure to write stdout also exits 2.

Limits: 8 MiB/source, 64 MiB total successfully read input, 10,000 source files,
100,000 visited entries, 128 directory levels, 256 KiB config/ignore file,
256 exclude globs of up to 4,096 bytes. At most 10,000 diagnostics per invocation;
including suppressed findings. An exceeding file produces an execution error instead of claiming its partial
diagnostics complete. Previously completed files remain in output. No fixing
begins on limit failure. Parser limits are in [parser-design.md](parser-design.md).

`scripts/verify-json.ps1` validates real release-binary JSON and SARIF for exit
0/1/2, fixes, suppression, Unicode paths, virtual stdin, I/O and argument failure
with PowerShell 7's JSON Schema validator. An invalid version must be rejected.
