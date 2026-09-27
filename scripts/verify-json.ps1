# PowerShell 7 JSON Schema validation against actual binary success/failure output.
param([string]$Binary = "$PSScriptRoot/../target/x86_64-pc-windows-msvc/release/verse-lint.exe")
$ErrorActionPreference = 'Stop'
$Binary = (Resolve-Path $Binary).Path
$sarifSchema = "$PSScriptRoot/../tests/schemas/sarif-schema-2.1.0.json"
if ((Get-FileHash $sarifSchema -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'ad6db49878699b091f3eeb765b6e29e92a34bad4da88664d000c923b549c3a25') { throw 'Pinned SARIF schema hash changed' }
$temp = Join-Path ([IO.Path]::GetTempPath()) ('verse-lint-schema-' + [Guid]::NewGuid())
[IO.Directory]::CreateDirectory($temp) | Out-Null
try {
    foreach ($format in @('json', 'sarif')) {
    $schema = if ($format -eq 'sarif') { $sarifSchema } else { "$PSScriptRoot/../docs/diagnostics.schema.json" }
    [IO.File]::WriteAllText((Join-Path $temp 'verse.toml'), "schema-version = 1`n")
    [IO.File]::WriteAllText((Join-Path $temp 'clean.verse'), "A := 1`n")
    [IO.File]::WriteAllText((Join-Path $temp 'bad.verse'), "A := 1  `n")
    [IO.File]::WriteAllText((Join-Path $temp 'suppressed.verse'), "# verse-lint: disable-next-line V1001 -- test`nA := 1  `n")
    [IO.File]::WriteAllText((Join-Path $temp '日 space #%.verse'), "A := 1  `n# TODO later`n")
    foreach ($case in @(
        @{ Arguments = @('clean.verse'); Exit = 0 },
        @{ Arguments = @('bad.verse'); Exit = 1 },
        @{ Arguments = @('bad.verse', '--fix'); Exit = 0 },
        @{ Arguments = @('suppressed.verse'); Exit = 0 },
        @{ Arguments = @('missing.verse'); Exit = 2 },
        @{ Arguments = @('--invalid-flag'); Exit = 2 },
        @{ Arguments = @('日 space #%.verse', '--select=V1001,V2002'); Exit = 1 },
        @{ Arguments = @('-', '--stdin-filepath=C:/virtual/日.verse'); Input = "A := 1"; Exit = 1 },
        @{ Arguments = @('-'); Input = '<# incomplete'; Exit = 2 }
    )) {
        $start = [Diagnostics.ProcessStartInfo]::new($Binary)
        $start.WorkingDirectory = $temp
        $start.UseShellExecute = $false
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.RedirectStandardInput = $case.ContainsKey('Input')
        $start.StandardOutputEncoding = [Text.Encoding]::UTF8
        if ($start.RedirectStandardInput) { $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false) }
        foreach ($arg in $case.Arguments) { $start.ArgumentList.Add($arg) }
        $start.ArgumentList.Add("--output-format=$format")
        $process = [Diagnostics.Process]::Start($start)
        if ($start.RedirectStandardInput) { $process.StandardInput.Write($case.Input); $process.StandardInput.Close() }
        $stdout = $process.StandardOutput.ReadToEndAsync()
        $stderr = $process.StandardError.ReadToEndAsync()
        if (-not $process.WaitForExit(30000)) { $process.Kill(); throw 'JSON smoke test timed out' }
        $json = $stdout.GetAwaiter().GetResult()
        $errorText = $stderr.GetAwaiter().GetResult()
        if ($process.ExitCode -ne $case.Exit) { throw "Unexpected exit $($process.ExitCode): $errorText" }
        $process.Dispose()
        if (-not (Test-Json -Json $json -SchemaFile $schema)) { throw 'Output failed JSON schema validation' }
    }
    $invalid = $json | ConvertFrom-Json -AsHashtable
    if ($format -eq 'sarif') { $invalid.version = '0.0.0' } else { $invalid.schemaVersion = 999 }
    if (Test-Json -Json ($invalid | ConvertTo-Json -Depth 50) -SchemaFile $schema -ErrorAction SilentlyContinue) { throw 'Schema accepted an intentionally invalid version' }
    Write-Output "$format schema: clean/violation/fix/suppression/Unicode/stdin/I/O/CLI outputs passed; invalid version rejected."
    }
} finally {
    [IO.Directory]::Delete($temp, $true)
}
