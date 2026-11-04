# Declared-side parity check: no formatter binary, no formatter checkout, no network.
# It deliberately does not fail when the formatter has moved on. Only structural
# problems and a declared linter revision that is not reachable from HEAD fail.
param([string]$PinPath)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $PinPath) { $PinPath = Join-Path $root 'parity.json' }

function Fail([string]$message) {
    [Console]::Error.WriteLine("parity declaration: $message")
    exit 1
}

$required = @(
    'corpusSha256',
    'goldenCaseCount',
    'formatterGrammarBaseRevision',
    'lastVerifiedFormatterRevision',
    'lastVerifiedLinterRevision'
)

if (Test-Path -LiteralPath $PinPath -PathType Leaf) {
    $text = [IO.File]::ReadAllText((Resolve-Path -LiteralPath $PinPath).Path, [Text.Encoding]::UTF8)
} else {
    $text = ''
}
if (-not $text.Trim()) { Fail "missing or empty $PinPath" }
try { $declared = $text | ConvertFrom-Json } catch { Fail "$PinPath is not valid JSON ($($_.Exception.Message))" }
if ($declared -isnot [pscustomobject]) { Fail "$PinPath must contain a JSON object" }

$present = @($declared.PSObject.Properties.Name)
$missing = @($required | Where-Object { $present -notcontains $_ })
if ($missing.Count) { Fail "$PinPath is missing $($missing -join ', ')" }
$unexpected = @($present | Where-Object { $required -notcontains $_ })
if ($unexpected.Count) { Fail "$PinPath has unexpected key(s) $($unexpected -join ', ')" }

if ($declared.corpusSha256 -isnot [string] -or $declared.corpusSha256 -notmatch '^[0-9a-f]{64}$') {
    Fail 'corpusSha256 must be 64 lowercase hex digits'
}
if ($declared.goldenCaseCount -isnot [long] -and $declared.goldenCaseCount -isnot [int]) {
    Fail 'goldenCaseCount must be a positive integer'
}
if ($declared.goldenCaseCount -lt 1) { Fail 'goldenCaseCount must be a positive integer' }
foreach ($key in @('formatterGrammarBaseRevision', 'lastVerifiedFormatterRevision', 'lastVerifiedLinterRevision')) {
    if ($declared.$key -isnot [string] -or $declared.$key -notmatch '^[0-9a-f]{40}$') {
        Fail "$key must be a 40-character lowercase revision"
    }
}

# Reachability is a local git question: the declared revision must be HEAD or an ancestor of it.
$declaredRevision = $declared.lastVerifiedLinterRevision
& git -C $root rev-parse --git-dir 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) { Fail "cannot read git repository at $root" }
& git -C $root cat-file -e "$declaredRevision^{commit}" 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    Fail "declared linter revision $declaredRevision is not present in this repository (shallow clone or wrong revision)"
}
& git -C $root merge-base --is-ancestor $declaredRevision HEAD 2>&1 | Out-Null
if ($LASTEXITCODE -ne 0) {
    $head = (& git -C $root rev-parse HEAD).Trim()
    Fail "declared linter revision $declaredRevision is not an ancestor of HEAD ($head)"
}

Write-Output "Declared parity pin verified: corpus $($declared.corpusSha256), $($declared.goldenCaseCount) golden cases, linter revision $declaredRevision reachable from HEAD. Convergence is not claimed; run scripts/verify-pair.py for that."
