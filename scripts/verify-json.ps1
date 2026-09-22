# PowerShell 7 JSON Schema validation against actual binary success/failure output.
param([string]$Binary = "$PSScriptRoot/../target/x86_64-pc-windows-msvc/release/verse-lint.exe")
$ErrorActionPreference = 'Stop'
$Binary = (Resolve-Path $Binary).Path
$schema = "$PSScriptRoot/../docs/diagnostics.schema.json"
$temp = Join-Path ([IO.Path]::GetTempPath()) ('verse-lint-schema-' + [Guid]::NewGuid())
[IO.Directory]::CreateDirectory($temp) | Out-Null
try {
    [IO.File]::WriteAllText((Join-Path $temp 'verse.toml'), "schema-version = 1`n")
    [IO.File]::WriteAllText((Join-Path $temp 'clean.verse'), "A := 1`n")
    [IO.File]::WriteAllText((Join-Path $temp 'bad.verse'), "A := 1  `n")
    foreach ($case in @(
        @{ Arguments = @('clean.verse'); Exit = 0 },
        @{ Arguments = @('bad.verse'); Exit = 1 },
        @{ Arguments = @('missing.verse'); Exit = 2 },
        @{ Arguments = @('--invalid-flag'); Exit = 2 }
    )) {
        $start = [Diagnostics.ProcessStartInfo]::new($Binary)
        $start.WorkingDirectory = $temp
        $start.UseShellExecute = $false
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.StandardOutputEncoding = [Text.Encoding]::UTF8
        foreach ($arg in $case.Arguments) { $start.ArgumentList.Add($arg) }
        $start.ArgumentList.Add('--output-format=json')
        $process = [Diagnostics.Process]::Start($start)
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'JSON smoke test timed out' }
        $json = $stdout.GetAwaiter().GetResult()
        $errorText = $stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne $case.Exit) { throw "Unexpected exit $($process.ExitCode): $errorText" }
        $process.Dispose()
        if (-not (Test-Json -Json $json -SchemaFile $schema)) { throw 'Output failed JSON schema validation' }
    }
    Write-Output 'JSON Schema: success, violation, I/O failure and CLI failure passed.'
} finally {
    [IO.Directory]::Delete($temp, $true)
}
