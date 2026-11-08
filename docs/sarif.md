# SARIF 2.1.0

`verse-lint . --output-format sarif` emits one SARIF run on stdout, without ANSI,
progress text, source contents, timestamps, random IDs or edit suggestions.
Output is deterministic for the same inputs/configuration. This command has no
network access: it does not upload anything, enable GitHub code scanning or
invoke external services.

- Driver metadata contains all eight rule IDs, stable names, English help,
  default enabled/level settings and safe-fix capability. Names, levels and
  default-enabled flags come from the `rules::REGISTRY` entry, not a parallel
  positional list; each result's `ruleIndex` is resolved from its `ruleId`, so
  registry order never decides which rule a result belongs to. Actual selected
  rules determine results; metadata never implies unselected rules were
  executed. An opt-in rule such as V3001 therefore appears with
  `defaultConfiguration.enabled: false`, and no result references it unless it
  was selected.
- Results retain JSON/text ordering, IDs, severity and messages. Result `level`
  is the effective severity after `[lint.severity]`, while
  `defaultConfiguration.level` keeps the registry default because it describes
  the tool, not this project's policy. Suppressed
  findings are omitted from `results`; run properties preserve the same summary
  counts. `--show-suppressed` (see [CLI](cli.md#suppressed-findings)) makes them
  visible in text, JSON and [GitHub
  annotations](cli.md#github-annotations) only: SARIF has one representation for
  a suppressed result, so a run would have to choose between omitting and
  marking. This tool omits, which means a SARIF `results` array is always active
  findings, and `properties.summary.suppressed` carries the count of what was
  removed. No custom property duplicates what SARIF `suppressions` would mean.
- `columnKind` is `unicodeCodePoints`: one-based scalar columns, not visual
  tab width or UTF-16 units. Regions include start/end lines/columns plus
  zero-based original UTF-8 byte offsets and lengths including BOM. EOF
  diagnostics are valid empty ranges. Fix results use saved-source positions.
- Relative artifact URIs are UTF-8 percent-encoded and based at `%SRCROOT%`.
  The absolute file URI for that config/CWD root is included only when required
  to resolve relative artifacts. Windows drive/UNC/verbatim paths are handled
  explicitly; spaces, Unicode, `%`, `#`, `?` and relative colons are encoded.
- Stdin provenance is explicit, not guessed from a filename. A real or virtual
  stdin name gets a descriptive non-file artifact, indexed region and **no URI**.
  No `file://` link is fabricated for an input stream, including virtual absolute
  Windows paths. No source contents are embedded.
- Inspection/config/I/O/CLI failures are `toolExecutionNotifications` at error
  level and set `invocations[].executionSuccessful=false`. Ordinary violations
  and denied warnings still mean successful execution, but retain exitCode 1.
  Partial inspection keeps valid results plus failure notifications, exit 2.
- Safe edits are intentionally not exported as SARIF fixes. Consumers must not
  apply unchecked byte patches from this report; use guarded CLI `--fix`.

The unmodified official Standard schema is pinned in
`tests/schemas/sarif-schema-2.1.0.json`; source/hash and full OASIS notices are
adjacent. `scripts/verify-json.ps1` verifies its hash and validates actual
release-binary outputs with PowerShell 7 Test-Json. Binary tests additionally
check URI semantics, code-point ranges, stdin, no edit payloads, deterministic
ordering and text/JSON/SARIF count agreement. Invalid schema-version probes
verify the validator is enforcing the schemas rather than merely parsing JSON.

If you only need pull-request annotations and not a machine-readable report,
`--output-format github` writes [workflow commands](cli.md#github-annotations)
straight to stdout and needs no code-scanning setup at all.

Reference: https://docs.oasis-open.org/sarif/sarif/v2.1.0/os/sarif-v2.1.0-os.html

## Upload policy: none

By [ADR 0001](adr/0001-sarif-upload-policy.md) this project **does not upload**
SARIF to GitHub code scanning, and ships no upload workflow, no code-scanning
configuration and no `security-events` permission. Emitting SARIF and uploading
SARIF are separate concerns: this tool owns the first, the consumer owns the
second.

The ADR argues what an upload would make public, the `security-events: write`
permission such a step requires, the result volume a style linter produces,
false positives becoming visible to third parties, and the fact that
error-level style findings would read as security alerts. This page only records
the outcome. `tests/sarif-upload-guard.rs` fails verification if any workflow
under `.github/workflows/` ever gains an `upload-sarif` step or that permission.

If you decide to upload in your own repository, that is your workflow, your
permissions and your licence — follow GitHub's own documentation for
`github/codeql-action/upload-sarif` rather than expecting a maintained recipe
here. Expect a large alert volume, and be aware that these findings are style
diagnostics, not security results.
