param(
    [Parameter(Mandatory=$true)][string]$Formatter,
    [Parameter(Mandatory=$true)][string]$Linter,
    [string]$ShellLabel = 'PowerShell'
)
$ErrorActionPreference = 'Stop'
$fmt = (Resolve-Path -LiteralPath $Formatter).Path
$lint = (Resolve-Path -LiteralPath $Linter).Path
$root = Join-Path ([IO.Path]::GetTempPath()) ('verse-shell-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
function Invoke-Checked([string]$Exe, [string[]]$ExeArgs, [int]$Expected, [string]$Label) {
    # Windows PowerShell 5.1 can promote native stderr to a terminating
    # NativeCommandError when the caller uses ErrorActionPreference=Stop.
    # These tools intentionally write diagnostics to stderr for exit 2, so
    # temporarily use Continue (and disable the PS7 native-error preference
    # when that variable exists) while preserving the process exit code.
    $savedErrorActionPreference = $ErrorActionPreference
    $nativePreference = Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue
    $savedNativePreference = $null
    if ($null -ne $nativePreference) {
        $savedNativePreference = $nativePreference.Value
        Set-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -Value $false
    }
    try {
        $ErrorActionPreference = 'Continue'
        & $Exe @ExeArgs > $null 2> $null
        $actual = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $savedErrorActionPreference
        if ($null -ne $nativePreference) {
            Set-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -Value $savedNativePreference
        }
    }
    if ($actual -ne $Expected) { throw "$ShellLabel $Label expected exit $Expected, got $actual" }
    return $actual
}
try {
    $clean = Join-Path $root '日本語 path with spaces.verse'
    [IO.File]::WriteAllText($clean, "A := 1`n", [Text.UTF8Encoding]::new($false))
    $fmtDirty = Join-Path $root 'dirty format.verse'
    [IO.File]::WriteAllText($fmtDirty, "A:=1`n", [Text.UTF8Encoding]::new($false))
    $lintDirty = Join-Path $root 'dirty lint.verse'
    [IO.File]::WriteAllText($lintDirty, "A := 1  `n", [Text.UTF8Encoding]::new($false))
    $bad = Join-Path $root 'unsupported syntax.verse'
    [IO.File]::WriteAllText($bad, "A := map{`"x`" => 1}`n", [Text.UTF8Encoding]::new($false))
    $results = [ordered]@{
        shell = $ShellLabel
        formatter_clean = Invoke-Checked $fmt @('--check', $clean) 0 'fmt clean'
        formatter_diff = Invoke-Checked $fmt @('--check', $fmtDirty) 1 'fmt diff'
        formatter_error = Invoke-Checked $fmt @('--check', $bad) 2 'fmt unsupported'
        linter_clean = Invoke-Checked $lint @('--output-format', 'json', $clean) 0 'lint clean'
        linter_violation = Invoke-Checked $lint @('--output-format', 'json', $lintDirty) 1 'lint violation'
        linter_error = Invoke-Checked $lint @('--output-format', 'json', $bad) 2 'lint unsupported'
    }
    if ($ShellLabel -eq 'cmd') {
        $line = ('"{0}" --check "{1}"' -f $fmt, $clean)
        & $env:ComSpec /d /c $line > $null 2> $null
        $results['cmd_clean'] = $LASTEXITCODE
        if ($LASTEXITCODE -ne 0) { throw "cmd.exe clean path expected 0, got $LASTEXITCODE" }
        $line = ('"{0}" --check "{1}"' -f $fmt, $fmtDirty)
        & $env:ComSpec /d /c $line > $null 2> $null
        $results['cmd_diff'] = $LASTEXITCODE
        if ($LASTEXITCODE -ne 1) { throw "cmd.exe diff path expected 1, got $LASTEXITCODE" }
    }
    [pscustomobject]$results | ConvertTo-Json -Compress
} finally {
    Remove-Item -LiteralPath $root -Recurse -Force
}
