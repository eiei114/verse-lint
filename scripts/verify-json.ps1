# PowerShell 7 JSON Schema validation against actual binary success/failure output,
# plus the GitHub annotation format, which has no schema and is checked by shape.
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
        @{ Arguments = @('suppressed.verse', '--show-suppressed'); Exit = 0 },
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

    # GitHub annotations: stdout is workflow commands only, never ANSI, with the
    # same exit-code contract. Paths keep the escaping GitHub decodes.
    [IO.File]::WriteAllText((Join-Path $temp 'a, b.verse'), "A := 1  `n")
    # The schema loop above runs `bad.verse --fix`, so restore it before reuse.
    [IO.File]::WriteAllText((Join-Path $temp 'bad.verse'), "A := 1  `n")
    [IO.File]::WriteAllText((Join-Path $temp 'long.verse'), "# $('a' * 130)`n")
    Push-Location $temp
    try {
        foreach ($case in @(
            @{ Arguments = @('clean.verse'); Exit = 0; Lines = 0 },
            @{ Arguments = @('bad.verse'); Exit = 1; Lines = 1 },
            @{ Arguments = @('long.verse', '--select=V2001'); Exit = 0; Lines = 1 },
            @{ Arguments = @('suppressed.verse'); Exit = 0; Lines = 0 },
            @{ Arguments = @('suppressed.verse', '--show-suppressed'); Exit = 0; Lines = 1 },
            @{ Arguments = @('missing.verse'); Exit = 2; Lines = 0 },
            @{ Arguments = @('a, b.verse', '--color=always'); Exit = 1; Lines = 1 },
            @{ Arguments = @('日 space #%.verse', '--select=V1001,V2002'); Exit = 1; Lines = 2 }
        )) {
            $run = & $Binary @($case.Arguments + @('--output-format=github')) 2>$null
            if ($LASTEXITCODE -ne $case.Exit) { throw "GitHub format: unexpected exit $LASTEXITCODE for $($case.Arguments -join ' ')" }
            $text = (@($run) -join "`n").TrimEnd("`r", "`n")
            $lines = if ($text -eq '') { @() } else { $text -split "`r?`n" }
            if ($lines.Count -ne $case.Lines) { throw "GitHub format: expected $($case.Lines) annotation(s) for $($case.Arguments -join ' '), got $($lines.Count): $text" }
            foreach ($line in $lines) {
                if ($line -notmatch '^::(error|warning|notice) file=[^,]*,line=\d+,col=\d+,endLine=\d+,endColumn=\d+,title=[^,]*(,|::)') { throw "GitHub format: not an annotation: $line" }
                if ($line.Contains([char]27)) { throw "GitHub format: emitted ANSI: $line" }
            }
        }
        $comma = & $Binary 'a, b.verse' '--output-format=github' 2>$null
        if ($comma -notmatch 'file=a%2C b\.verse,') { throw "GitHub format: a comma in a path must be escaped: $comma" }
        $unicode = & $Binary '日 space #%.verse' '--select=V1001,V2002' '--output-format=github' 2>$null
        if ($unicode -notmatch 'file=日 space #%25\.verse,') { throw "GitHub format: a percent in a path must be escaped: $unicode" }
        $partial = & $Binary '.' 'missing.verse' '--output-format=github' 2>$null
        if ($partial -match 'missing') { throw "GitHub format: an execution error reached stdout: $partial" }
    } finally { Pop-Location }
    Write-Output 'github format: clean/error/warning/suppression/partial-failure/escaping/no-color passed.'
} finally {
    [IO.Directory]::Delete($temp, $true)
}
