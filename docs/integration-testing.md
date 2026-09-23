# Paired and shell integration checks

The linter repository stays independent of `verse-fmt`: it has no formatter
crate, binary, or runtime dependency. This developer-only script accepts a
formatter release binary and the formatter's self-authored golden corpus to
verify the tools' on-disk convergence contract.

## Paired fix/format convergence

From the linter worktree, after building both release binaries:

```powershell
python scripts/verify-pair.py `
  --formatter ..\verse-fmt-implementation\target\x86_64-pc-windows-msvc\release\verse-fmt.exe `
  --corpus ..\verse-fmt-implementation\tests\fixtures\corpus.json `
  --output target\verification\paired\convergence.json
```

The script pins the corpus SHA-256 and requires its 56 golden-success inputs.
For each case it runs `verse-lint --fix`, then `verse-fmt --write`, checks the
formatter's independent golden bytes, and repeats both tools to prove the
second cycle changes nothing. It uses isolated temporary files and removes
them on exit. The 18 explicit formatter refusals are not sent through a
successful paired-fix path. Reports are local artifacts and are not committed.

On 2026-09-23, all 52/52 cases passed with formatter revision
`ae191de08058a217d331640f0b098d6fb76435f3` and linter revision
`8ecb25acfebdf22d08accfcf56ecf145a72bf9cd`; corpus SHA-256 was
`5b981328c3020541697d95a21db76031386347a6d4902aef2fb2428b35ecf5f7`.
This is a tool-to-tool invariant test, not UEFN compiler acceptance.

After mixed LF/CRLF preservation became supported, the corpus grew by one golden
case. Current Iteration 10 paired report validates 53/53 with corpus SHA-256
`bb9275b7950b798ca001757f8c7f34aa87d424d7391d899b873986b3355adc83`.

Iteration 11 adds self-authored parser/formatter coverage for `class()`,
if-binding conditions and nested indented object/array construction. After the
changes were committed, the paired harness validated all 56 golden cases using
corpus SHA-256
`f5e44fbb19e017bf7caf242ab2275fab991169e7fb22c325cef462ec48452c94`.

## Shell matrix

The PowerShell 5.1-compatible script checks exit codes 0/1/2 for both tools
using a Unicode-and-space file path, plus `cmd.exe` invocation for formatter
codes 0 and 1. Run the base matrix under PowerShell 7 and Windows PowerShell
5.1, then run `cmd.exe` checks from Windows PowerShell 5.1:

```powershell
$fmt = '..\verse-fmt-implementation\target\x86_64-pc-windows-msvc\release\verse-fmt.exe'
$lint = 'target\x86_64-pc-windows-msvc\release\verse-lint.exe'
pwsh -NoProfile -File scripts/verify-shells.ps1 -Formatter $fmt -Linter $lint -ShellLabel pwsh
powershell.exe -NoProfile -File scripts/verify-shells.ps1 -Formatter $fmt -Linter $lint -ShellLabel powershell51
powershell.exe -NoProfile -File scripts/verify-shells.ps1 -Formatter $fmt -Linter $lint -ShellLabel cmd
```

All three invocations passed on Windows: PowerShell 7 and PowerShell 5.1 each
observed formatter/linter exit codes 0, 1, 2; `cmd.exe` observed formatter 0
and 1. Error output is intentionally suppressed by the harness while the
native process exit code is checked. This tests local file arguments, not
PowerShell pipeline encoding; file input remains recommended for PS 5.1.
