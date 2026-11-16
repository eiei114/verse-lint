# Editor, pre-commit and CI integration

`verse-lint` exposes three machine-readable surfaces and one virtual-input mode.
All of them are plain command output, so an editor, a commit hook or a CI job only
has to run the binary — **there is no language server and no editor extension**,
in this release or planned in it:

- text stdout: `path:line:column: severity rule-id message`
  ([output contract](cli.md#output-contract));
- JSON stdout, conforming to [diagnostics.schema.json](diagnostics.schema.json);
- `--output-format github`: one [GitHub Actions workflow
  command](cli.md#github-annotations) per finding;
- `verse-lint - --stdin-filepath <file>`: lint a buffer that is not on disk.

The recipes below are copy-pasteable and test-enforced. `tests/integration-doc-examples.rs`
extracts the problem-matcher pattern from this document, runs the real binary on
a fixture with a known finding and asserts the pattern captures the file, line,
column, severity, rule ID and message the JSON report describes — including a
`C:/...`-style absolute path. It also runs both hooks below against a temporary
repository — a clean staged file, a staged finding, a denied warning, an
unreadable staged blob and a path with a non-ASCII byte — and it lints a project
twice, as a directory walk and as a piped buffer, so the difference the next
section describes cannot drift.

Every recipe keeps stdout machine-readable. Exit codes are the ones in
[the CLI reference](cli.md#output-contract) and mean exactly this:

| Exit | Meaning |
| --- | --- |
| `0` | Inspection completed; no selected errors (and, with `--deny-warnings`, no warnings either). |
| `1` | Lint violations, or warnings under `--deny-warnings`. Findings were reported. |
| `2` | Incomplete inspection: a source the tool refuses, a limit, an I/O failure or a configuration failure. Nothing may be read as "clean". |

`2` is not a bigger `1`. It says part of the input was never inspected, and it
prints execution errors on stderr in every format (in JSON they also appear in
`executionErrors`). Treating `2` as success is the one mistake that makes a hook
or a job worthless.

A hook or a task never rewrites your files unless you add `--fix` yourself.
`--fix` is a write run, not a formatter: it applies validated V1001/V1002 edits
through the guarded Windows writer described in [safe fixes](fixes.md). Preview
it with `--fix --diff` before wiring it into anything automatic.

## Editor: a VS Code task with a problem matcher

VS Code cannot parse arbitrary compiler output. A task plus a
[problem matcher](https://code.visualstudio.com/docs/debugtest/problem-matchers)
is the built-in way to turn a command's text output into clickable entries in the
Problems panel, and it is the only integration VS Code needs here.

Put this in `.vscode/tasks.json`:

```json
{
  "version": "2.0.0",
  "tasks": [
    {
      "label": "verse-lint: lint workspace",
      "type": "process",
      "command": "${workspaceFolder}/target/x86_64-pc-windows-msvc/release/verse-lint.exe",
      "args": [".", "--output-format", "text", "--color", "never"],
      "options": { "cwd": "${workspaceFolder}" },
      "problemMatcher": {
        "owner": "verse-lint",
        "source": "verse-lint",
        "fileLocation": ["relative", "${workspaceFolder}"],
        "pattern": {
          "regexp": "^((?:[A-Za-z]:)?[^:]+):(\\d+):(\\d+): (error|warning) (V\\d+) ([^\\r\\n]*)$",
          "file": 1,
          "line": 2,
          "column": 3,
          "severity": 4,
          "code": 5,
          "message": 6
        }
      }
    }
  ]
}
```

Read as a plain regular expression (JSON doubles every backslash), the pattern
is `^((?:[A-Za-z]:)?[^:]+):(\d+):(\d+): (error|warning) (V\d+) ([^\r\n]*)$`.
In your own repository the binary is already installed, so only `command` and
`options.cwd` change.

Why each part is there:

- `--color never` keeps ANSI out of the output. VS Code pipes the task's stdout,
  so `auto` would already disable color, but a fixed value makes the bytes
  independent of the terminal and of `NO_COLOR`.
- `"type": "process"` runs the binary directly. A shell task would add quoting
  rules for a path that may contain spaces, and VS Code's own output would mix
  with the findings.
- `--output-format text` is the default and is named anyway, because the pattern
  parses exactly that shape: `path:line:column: severity rule-id message`.
- The first capture group is one group, not two, because a problem matcher has
  exactly one `file` index. `(?:[A-Za-z]:)?` lets a drive letter stay part of
  the path and `[^:]+` takes the rest, so the colon that separates the path from
  the line number is the last colon on the line. That is what makes
  `C:/Users/you/Content/device.verse:12:7: ...` one finding, while
  `Content/device.verse:12:7: ...` and `../shared/x.verse:12:7: ...` match too.
- A Windows file name cannot hold a colon, so the pattern covers every real
  path. The one path that can is the **virtual** `--stdin-filepath` label of the
  next section. A label such as `C:/Program:Data/buffer.verse` would not match:
  give it a real path, or skip the matcher and read JSON when you need such a
  name.
- `fileLocation` is `["relative", "${workspaceFolder}"]` because the tool's paths
  are `/`-separated and relative to the **configuration root** (the directory
  owning `verse.toml`, or CWD when there is none) — see
  [output contract](cli.md#output-contract). Keep `verse.toml` at the workspace
  root and this prefix is right. If your configuration lives in `Content/`,
  the same paths are `Content/`-relative: use
  `["relative", "${workspaceFolder}/Content"]`, or run the task with
  `"options": { "cwd": "${workspaceFolder}/Content" }`.
- A path on another Windows volume is reported as an absolute path. Only that
  case needs `"fileLocation": "absolute"`, and it is the case a workspace-relative
  prefix cannot resolve.

Run the task with **Terminal → Run Task** (or bind a key to
`workbench.action.tasks.runTask`). VS Code has no built-in "run a task on save",
so this task lints the files as they are on disk. Unsaved buffers are covered by
the next section.

## Save-time linting through stdin

```powershell
verse-lint - --stdin-filepath Content/device.verse
```

`-` reads UTF-8 source from stdin and must be the only path. `--stdin-filepath`
is a **virtual** path: it is never written, it only tells the tool where the
buffer came from.

- Configuration discovery starts at the virtual file's parent and walks upward,
  stopping at the first directory containing `.git`
  ([configuration](cli.md#configuration)). Without `--stdin-filepath` it starts
  at CWD and the finding is labelled `<stdin>`; with it, a buffer whose file is
  not on disk still gets the project's `verse.toml` — its `[lint]` selection,
  options and severity overrides — and the `verse-lint.ignore` at that root is
  located and validated, so an invalid one still fails with exit `2`.
- **No exclusion pattern applies to a buffer.** `[files] exclude` and the
  entries of `verse-lint.ignore` are consulted by file discovery, and a stdin
  run never performs discovery, so a buffer whose virtual path the project
  excludes is still linted. The note below spells out the consequence.
- The virtual path is also the reported path, so a matcher or a hook sees the
  path the editor knows, not a temporary file.
- Diagnostics are ordinary text, JSON or GitHub annotations. Only the input
  mode differs.

**Stdin never fixes.** `--fix` is refused with stdin — the run exits `2` with
`--fix cannot be used with stdin` — because a write run needs a file it can
stage, lock, validate and replace ([safe fixes](fixes.md)). A save hook that
pipes a buffer therefore never edits your source; if you want fixes, run
`--fix` (or `--fix --diff` first) as its own deliberate command.

Wiring this to a save event needs a program that can pipe the buffer. VS Code
alone cannot: a task runs a command, it cannot capture an unsaved editor buffer.
Editors and wrappers that can pipe text use the command above. Three practical
details:

- Piping a buffer through a shell can change its bytes. Windows PowerShell 5.1
  re-encodes text line by line **and appends a final newline**, so a buffer without
  one would stop reporting V1002. PowerShell 7.0–7.3 convert a native program's
  output to `.NET` strings before piping or redirecting it, which appends the same
  final newline and would hide the same finding; `PSNativeCommandPreserveBytePipe`
  only became mainstream in 7.4 ([about_Pipelines](https://learn.microsoft.com/powershell/module/microsoft.powershell.core/about/about_pipelines)).
  Run the pipeline under `pwsh` **7.4 or later**, or under a POSIX shell, and
  `$OutputEncoding` does not prevent that conversion. The pre-commit script below
  refuses to run under anything older than 7.4 for that reason.
- `--stdin-filepath` decides the label and where configuration discovery
  starts; it does not make the exclusions apply either. A project that excludes
  `Vendor/**` still reports `Vendor/generated.verse:1:11: error V1001` when that
  buffer is piped in, while the same file on disk is skipped with
  `Vendor/generated.verse: excluded (verse.toml exclude)` under
  `verse-lint . --verbose`. A hook that lints only staged files therefore sees
  files a directory run leaves out — the same bypass an explicit path argument
  has ([ignore file](cli.md#ignore-file)).

## Pre-commit hook: lint only the staged files

A commit hook should lint **what is about to be committed**, not the working
tree. A partially staged file, or an editor that saved something you did not
stage, would otherwise be reported at line numbers that do not exist in the
commit. Both scripts below read the staged blob with `git show :<path>` and pipe
it through stdin, which is why the stdin mode above matters.

Staged `.verse` blobs only — `git diff --cached --diff-filter=ACMR` skips
deleted files and every non-Verse file. Every staged blob is linted, including
one that `[files] exclude` or `verse-lint.ignore` keeps out of a directory run:
stdin is not file discovery, so the hook never skips a file you staged.

### POSIX `sh`

`.git/hooks/pre-commit` (make it executable), or any file a `core.hooksPath`
points at:

```sh
#!/bin/sh
# Lint the staged content of added/copied/modified/renamed .verse files.
# Set VERSE_LINT to the binary's path if it is not on PATH;
# set VERSE_LINT_DENY_WARNINGS=1 to also fail on warnings.
set -u

binary=${VERSE_LINT:-verse-lint}
if ! command -v "$binary" >/dev/null 2>&1; then
    printf 'verse-lint was not found at %s; set VERSE_LINT to its path\n' "$binary" >&2
    exit 2
fi
deny=${VERSE_LINT_DENY_WARNINGS:+--deny-warnings}

blob=$(mktemp) || { printf 'verse-lint: could not create a temporary file\n' >&2; exit 2; }
trap 'rm -f "$blob"' EXIT HUP INT TERM

# `core.quotePath=false` is what makes a non-ASCII path usable: with Git's
# default quoting, Content/cafe-and-a-diacritic.verse comes back as the C-quoted
# label "Content/caf\303\251.verse", which `git show` and `--stdin-filepath` both
# reject. That label blocked every commit touching such a path, and exited 2 on
# a clean one.
if ! files=$(git -c core.quotePath=false diff --cached --name-only \
    --diff-filter=ACMR -- '*.verse'); then
    printf 'verse-lint could not list the staged files\n'
    exit 2
fi
[ -n "$files" ] || exit 0

status=0
while IFS= read -r file; do
    [ -n "$file" ] || continue
    # A staged blob that cannot be read is an incomplete inspection, not a clean
    # file: `git show` writes nothing on failure and the tool would exit 0 on an
    # empty buffer. Piping `git show` straight into the tool only reports the
    # tool's status — `sh` has no `pipefail` — so the blob goes to a file first
    # and `git show` is checked before anything is linted.
    if ! git show ":$file" > "$blob" 2>/dev/null; then
        printf 'verse-lint could not read the staged content of %s\n' "$file"
        status=2
        continue
    fi
    output=$({ "$binary" - --stdin-filepath "$file" \
        --output-format text --color never $deny < "$blob" ; } 2>&1)
    code=$?
    case $code in
        0) ;;
        1) printf '%s\n' "$output"; [ "$status" -eq 2 ] || status=1 ;;
        2) printf '%s\n' "$output"; status=2 ;;
        *) printf 'verse-lint exited %s for %s\n' "$code" "$file"; status=2 ;;
    esac
done <<EOF
$files
EOF

if [ "$status" -ne 0 ]; then
    printf 'commit blocked: verse-lint exited %s (0 clean, 1 violations, 2 incomplete)\n' "$status"
fi
exit "$status"
```

Three deliberate choices: the file list is captured before the loop, so the exit
status is not lost in a pipe's subshell and Git's quoting never reaches `git
show`; the staged blob goes through a temporary file so `git show`'s status is
checked **before** the tool runs, instead of being overwritten by the linter's;
and the loop body records the worst exit, so a later clean file cannot hide an
earlier `2`.

The loop is newline-delimited because a file name containing a newline cannot
survive it. Where such names are possible, use `git diff --cached -z` and a
`read -d ''` loop, which needs `bash`/`ksh`/`zsh` instead of a plain `sh`.

### PowerShell

Git runs hooks with `sh`, so a PowerShell script needs a two-line shim. Save the
script as `.githooks/pre-commit.ps1` and point `core.hooksPath` at `.githooks`
(`git config core.hooksPath .githooks`); then `.githooks/pre-commit` is:

```sh
#!/bin/sh
exec pwsh -NoProfile -File .githooks/pre-commit.ps1
```

The script itself:

```powershell
# Lint the staged content of added/copied/modified/renamed .verse files.
# Set VERSE_LINT to the binary's path if it is not on PATH;
# set VERSE_LINT_DENY_WARNINGS=1 to also fail on warnings.
$ErrorActionPreference = 'Stop'
# A native byte pipeline only arrived in PowerShell 7.4: before it, piping or
# redirecting a native program's output converted that output to .NET strings,
# which appends a final newline and would hide a V1002 finding. Windows
# PowerShell 5.1 pipes text line by line for the same reason. Run this hook
# with pwsh, which the sh shim does.
if ($PSVersionTable.PSVersion -lt [version]'7.4') {
    Write-Host 'This hook needs pwsh (PowerShell 7.4+): earlier versions pipe native output as text lines and would hide a missing final newline'
    exit 2
}
# Encoding for both directions: the staged list is read as UTF-8 and the staged
# blob is written as UTF-8, so a non-ASCII path arrives as itself.
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)

$binary = if ($env:VERSE_LINT) { $env:VERSE_LINT } else { 'verse-lint' }
if (-not (Get-Command $binary -ErrorAction SilentlyContinue)) {
    Write-Host "verse-lint was not found at '$binary'; set VERSE_LINT to its path"
    exit 2
}
$denyWarnings = [bool]$env:VERSE_LINT_DENY_WARNINGS
# `core.quotePath=false` is what makes a non-ASCII path usable: with Git's
# default quoting, Content/cafe-and-a-diacritic.verse comes back as the C-quoted
# label Content/caf\303\251.verse, which `git show`, `git cat-file -e` and
# `--stdin-filepath` all reject. That label blocked every commit touching such a
# path, and exited 2 on a clean one.
$files = @(git -c core.quotePath=false diff --cached --name-only --diff-filter=ACMR -- '*.verse')
if ($LASTEXITCODE -ne 0) {
    Write-Host 'verse-lint could not list the staged files'
    exit 2
}
if ($files.Count -eq 0) { exit 0 }

$status = 0
foreach ($file in $files) {
    $arguments = @('-', '--stdin-filepath', $file, '--output-format', 'text', '--color', 'never')
    if ($denyWarnings) { $arguments += '--deny-warnings' }
    git cat-file -e ":$file" 2>$null
    if ($LASTEXITCODE -ne 0) {
        Write-Host "verse-lint could not read the staged content of $file"
        $status = 2
        continue
    }
    git show ":$file" | & $binary @arguments
    $code = $LASTEXITCODE
    if ($code -eq 1) {
        if ($status -eq 0) { $status = 1 }
    } elseif ($code -ne 0) {
        # 2 is an incomplete inspection, and any other non-zero code — a panic's
        # 101, a wrapper's 3 — is one too: nothing says the buffer was inspected.
        # Recording only 2 and 1 would leave $status at 0 and let the commit
        # through with no result at all.
        Write-Host "verse-lint exited $code for $file; the inspection did not complete"
        $status = 2
    }
}
if ($status -ne 0) {
    Write-Host "commit blocked: verse-lint exited $status (0 clean, 1 violations, 2 incomplete)"
}
exit $status
```

`--deny-warnings` is an option, not a default: most policy rules (V2001, V2002,
V2003, V1004, V3001, V3002, V3003) are opt-in warnings, and failing every commit
for a warning is a project decision. With it, a warning exits `1` just like an
error; without it, only errors block the commit — and suppressed findings never
do ([suppressed findings](cli.md#suppressed-findings)).

The PowerShell hook is stricter about its own prerequisites than the `sh` one, and
deliberately so:

- It runs under `pwsh` **7.4 or later**, the first version that pipes raw bytes
  between two native commands: PowerShell 7.0–7.3 converted a native program's
  output to `.NET` strings before piping or redirecting it, and Windows
  PowerShell 5.1 pipes text line by line. Both append a final newline, so a staged
  blob without one would look fixed and V1002 would never be reported. The
  version guard (`$PSVersionTable.PSVersion -lt [version]'7.4'`) exits `2`
  instead of producing a false clean result.
- It reads the staged list with `core.quotePath=false`. The default quoting
  filters the name through Git's C-quoting rules whenever a path holds a byte
  above `0x7f`, so a non-ASCII name arrives as `"Content/caf\303\251.verse"` — a
  label `git show`, `git cat-file -e` and `--stdin-filepath` all reject, which
  blocked every commit touching a non-ASCII `.verse` path and exited `2` on a
  clean one. Both hooks disable that quoting. A name containing a newline still
  splits into two records, so use `git diff --cached -z` and a `read -d ''` loop
  in the `sh` script if such names are possible.
- `git cat-file -e` checks the staged blob before piping it. If the object is
  missing, `git show` writes nothing and the tool would report a clean empty buffer;
  this way the hook exits `2`.
- A missing binary is `2` in both scripts, never a silent pass. That is the
  difference between "this project has no findings" and "the hook is not doing
  its job", and only one of them should let a commit through.
- The recorded status is the **worst** the loop saw, and any non-zero code the
  exit table does not name — a Rust panic's `101`, a wrapper's `3` — is recorded
  as `2`. The `sh` hook's `*)` branch does the same. A code the script did not
  record would leave the status at `0`, and the commit would proceed without any
  inspection result.

Both hooks stop at the first blocked commit and print the tool's own lines, so
the message is the real `path:line:column: severity rule-id message` line; you
can click it in a terminal that understands file links. They never pass `--fix`,
and neither script edits the index.

## Continuous integration: GitHub annotations

`--output-format github` writes one Actions workflow command per finding, so the
pull request shows the finding on the changed line. It needs no uploaded report,
no code-scanning database, no token and no extra permission — only the ordinary
checkout, and the finding is attached to the step's log and the diff.

```yaml
name: Lint Verse

on:
  pull_request:
  workflow_dispatch:

permissions:
  contents: read

jobs:
  verse-lint:
    runs-on: windows-2025
    steps:
      - uses: actions/checkout@v4
      - name: Check out verse-lint
        uses: actions/checkout@v4
        with:
          repository: eiei114/verse-lint
          ref: main          # pin a commit SHA once you rely on this in CI
          path: .verse-lint
      - name: Build verse-lint from source
        shell: pwsh
        working-directory: .verse-lint
        run: cargo build --release --locked --target x86_64-pc-windows-msvc
      - name: Lint Verse
        shell: pwsh
        run: ./.verse-lint/target/x86_64-pc-windows-msvc/release/verse-lint.exe Content --output-format github
```

- The two `actions/checkout` steps matter: the first checks out **your** Verse
  repository, the second checks out the linter into `.verse-lint/`. The crate is
  **not published**: there is no crate on crates.io and no release asset to
  download yet ([installation paths](../README.md#installation)), so the
  workflow has to build it. Building in your own repository would fail — that
  repository has no `Cargo.toml` and no `rust-toolchain.toml`, and
  `cargo build --locked` would stop the job before the lint step ran
  (`error: could not find Cargo.toml`). The linter's own checkout carries the
  pinned `rust-toolchain.toml` and `Cargo.lock`, and the `windows-2025` image
  provides the MSVC build tools. Replace the build step with an install step (and
  drop the second checkout) once a published artifact exists; nothing else
  changes.
- `ref: main` is a moving target and a supply-chain surface: pin a commit SHA,
  as this repository's own workflow does for `actions/checkout`, so a lint run
  cannot change meaning without a commit you reviewed.
- The lint step runs from the repository root, so `./.verse-lint/target/...` is
  the binary the previous step just built. The linter is not on `PATH` and is not
  installed globally; the relative path is the whole wiring.
- The job fails on a non-zero exit: `1` for violations and `2` for an incomplete
  inspection. That is intended — a run that could not inspect every source is
  not a passing run.
- `Content` stands in for your source root. `file=` in the annotation is the
  config-root-relative path, and GitHub resolves it against the repository root,
  so annotations line up only when the configuration root is the repository root.
  The full rule, including what a nested `verse.toml` does, is in
  [GitHub annotations](cli.md#github-annotations).
- GitHub shows at most 10 warning and 10 error annotations per step. Beyond that
  the workflow commands still run but are not attached to the diff, so keep
  `--output-format json` (or `sarif`) for a complete report and use the
  annotations for review. See [GitHub annotations](cli.md#github-annotations).
- No network access, no GitHub API call and no `GITHUB_*` sniffing happen in the
  tool. `--color` is inert for this format: a workflow command never carries
  ANSI.

Do not add a `github/codeql-action/upload-sarif` step. This repository decided
against code-scanning upload — the linter is a style linter, not a security
scanner, and every finding would become a permanent alert — and the reasons are
recorded in [ADR 0001](adr/0001-sarif-upload-policy.md) and [SARIF](sarif.md#upload-policy-none).
Emitting SARIF writes stdout and uploads nothing. If you want code scanning in
your own repository, that upload is your own step under your own permissions,
and it is not something this tool's documentation recommends.

## JSON from a script

`--output-format json` keeps a machine-readable summary that is easy to assert
on, and it is still emitted when the run exits `2`, so a script can tell "clean"
from "not fully inspected" without parsing prose:

```sh
set -o pipefail   # without this, the pipeline reports jq's status, not the linter's
verse-lint Content --output-format json |
  jq -r '.summary | "errors=\(.errors) warnings=\(.warnings) complete=\(.complete)"'
```

```powershell
$report = verse-lint.exe Content --output-format json | ConvertFrom-Json
$exit = $LASTEXITCODE
"files={0} errors={1} warnings={2} complete={3}" -f `
    $report.summary.filesChecked, $report.summary.errors, $report.summary.warnings, $report.summary.complete
if ($exit -eq 2 -or $report.summary.errors -gt 0) { exit 1 }
```

`summary.errors` and `summary.warnings` count active diagnostics only — suppressed
findings never inflate them — and `summary.complete` is `false` whenever an execution
error occurred. The field list is
[the output contract](cli.md#output-contract) plus
[diagnostics.schema.json](diagnostics.schema.json); `schemaVersion` is `1`.

A blocked commit, a failed job or a `2` is worth reporting differently from a
lint finding: the first two say the tool could not do its job, the third says
your source has findings. Do not paper over the difference by treating every
non-zero exit as "warnings".
