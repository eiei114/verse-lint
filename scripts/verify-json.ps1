# PowerShell 7 JSON Schema validation against actual binary success/failure output,
# plus the GitHub annotation format, which has no schema and is checked by shape.
param([string]$Binary = "$PSScriptRoot/../target/x86_64-pc-windows-msvc/release/verse-lint.exe")
$ErrorActionPreference = 'Stop'
$Binary = (Resolve-Path $Binary).Path
$sarifSchema = "$PSScriptRoot/../tests/schemas/sarif-schema-2.1.0.json"
if ((Get-FileHash $sarifSchema -Algorithm SHA256).Hash.ToLowerInvariant() -ne 'ad6db49878699b091f3eeb765b6e29e92a34bad4da88664d000c923b549c3a25') { throw 'Pinned SARIF schema hash changed' }
$temp = Join-Path ([IO.Path]::GetTempPath()) ('verse-lint-schema-' + [Guid]::NewGuid())
[IO.Directory]::CreateDirectory($temp) | Out-Null
# Native invocation would leave $LASTEXITCODE at the last expected failure, and
# GitHub's pwsh wrapper turns that into the step's exit code even after this
# script prints success. Every probe therefore goes through ProcessStartInfo,
# which never touches the automatic variable.
function Invoke-Lint([string]$WorkingDirectory, [string[]]$Arguments, [string]$InputText) {
    $start = [Diagnostics.ProcessStartInfo]::new($Binary)
    $start.WorkingDirectory = $WorkingDirectory
    $start.UseShellExecute = $false
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    $start.RedirectStandardInput = $null -ne $InputText
    $start.StandardOutputEncoding = [Text.Encoding]::UTF8
    if ($start.RedirectStandardInput) { $start.StandardInputEncoding = [Text.UTF8Encoding]::new($false) }
    foreach ($arg in $Arguments) { $start.ArgumentList.Add($arg) }
    $process = [Diagnostics.Process]::Start($start)
    if ($start.RedirectStandardInput) { $process.StandardInput.Write($InputText); $process.StandardInput.Close() }
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) { $process.Kill(); throw "verse-lint timed out: $($Arguments -join ' ')" }
    $out = $stdout.GetAwaiter().GetResult()
    $err = $stderr.GetAwaiter().GetResult()
    $code = $process.ExitCode
    $process.Dispose()
    [pscustomobject]@{ Output = $out; Error = $err; Code = $code }
}
function New-Fixture([string]$Name, [string]$Body) {
    [IO.File]::WriteAllText((Join-Path $temp $Name), $Body)
}
# Mixed and CRLF fixtures are byte inputs, not text: a text write is free to
# normalise the terminators this rule exists to detect.
function New-ByteFixture([string]$Name, [string]$Body) {
    [IO.File]::WriteAllBytes((Join-Path $temp $Name), [Text.Encoding]::UTF8.GetBytes($Body))
}
try {
    New-Fixture 'verse.toml' "schema-version = 1`n"
    New-Fixture 'clean.verse' "A := 1`n"
    New-Fixture 'bad.verse' "A := 1  `n"
    New-Fixture 'suppressed.verse' "# verse-lint: disable-next-line V1001 -- test`nA := 1  `n"
    New-Fixture '日 space #%.verse' "A := 1  `n# TODO later`n"
    New-ByteFixture 'crlf.verse' "A := 1`r`nB := 2`r`n"
    New-ByteFixture 'mixed.verse' "A := 1`r`nB := 2`r`nC := 3`n"

    foreach ($format in @('json', 'sarif')) {
    $schema = if ($format -eq 'sarif') { $sarifSchema } else { "$PSScriptRoot/../docs/diagnostics.schema.json" }
    foreach ($case in @(
        @{ Arguments = @('clean.verse'); Exit = 0 },
        @{ Arguments = @('bad.verse'); Exit = 1 },
        @{ Arguments = @('bad.verse', '--fix'); Exit = 0 },
        @{ Arguments = @('suppressed.verse'); Exit = 0 },
        @{ Arguments = @('suppressed.verse', '--show-suppressed'); Exit = 0 },
        @{ Arguments = @('missing.verse'); Exit = 2 },
        @{ Arguments = @('--invalid-flag'); Exit = 2 },
        @{ Arguments = @('日 space #%.verse', '--select=V1001,V2002'); Exit = 1 },
        @{ Arguments = @('mixed.verse', '--select=V2003'); Exit = 0 },
        @{ Arguments = @('mixed.verse', '--select=V2003', '--deny-warnings'); Exit = 1 },
        @{ Arguments = @('crlf.verse', '--select=V2003'); Exit = 0 },
        @{ Arguments = @('-', '--stdin-filepath=C:/virtual/日.verse'); Input = "A := 1"; Exit = 1 },
        @{ Arguments = @('-'); Input = '<# incomplete'; Exit = 2 }
    )) {
        # Rebuild each time: the `--fix` case rewrites its input.
        New-Fixture 'bad.verse' "A := 1  `n"
        $run = Invoke-Lint $temp (@($case.Arguments) + @("--output-format=$format")) $case.Input
        if ($run.Code -ne $case.Exit) { throw "Unexpected exit $($run.Code): $($run.Error)" }
        if (-not (Test-Json -Json $run.Output -SchemaFile $schema)) { throw 'Output failed JSON schema validation' }
    }
    $invalid = $run.Output | ConvertFrom-Json -AsHashtable
    if ($format -eq 'sarif') { $invalid.version = '0.0.0' } else { $invalid.schemaVersion = 999 }
    if (Test-Json -Json ($invalid | ConvertTo-Json -Depth 50) -SchemaFile $schema -ErrorAction SilentlyContinue) { throw 'Schema accepted an intentionally invalid version' }
    Write-Output "$format schema: clean/violation/fix/suppression/Unicode/stdin/I/O/CLI outputs passed; invalid version rejected."
    }

    # GitHub annotations: stdout is workflow commands only, never ANSI, with the
    # same exit-code contract. Paths keep the escaping GitHub decodes.
    New-Fixture 'a, b.verse' "A := 1  `n"
    New-Fixture 'bad.verse' "A := 1  `n"
    New-Fixture 'long.verse' "# $('a' * 130)`n"
    foreach ($case in @(
        @{ Arguments = @('clean.verse'); Exit = 0; Lines = 0 },
        @{ Arguments = @('bad.verse'); Exit = 1; Lines = 1 },
        @{ Arguments = @('long.verse', '--select=V2001'); Exit = 0; Lines = 1 },
        @{ Arguments = @('mixed.verse', '--select=V2003'); Exit = 0; Lines = 1 },
        @{ Arguments = @('crlf.verse', '--select=V2003'); Exit = 0; Lines = 0 },
        @{ Arguments = @('suppressed.verse'); Exit = 0; Lines = 0 },
        @{ Arguments = @('suppressed.verse', '--show-suppressed'); Exit = 0; Lines = 1 },
        @{ Arguments = @('missing.verse'); Exit = 2; Lines = 0 },
        @{ Arguments = @('a, b.verse', '--color=always'); Exit = 1; Lines = 1 },
        @{ Arguments = @('日 space #%.verse', '--select=V1001,V2002'); Exit = 1; Lines = 2 }
    )) {
        $run = Invoke-Lint $temp (@($case.Arguments) + @('--output-format=github')) $null
        if ($run.Code -ne $case.Exit) { throw "GitHub format: unexpected exit $($run.Code) for $($case.Arguments -join ' ')" }
        $text = $run.Output.TrimEnd("`r", "`n")
        $lines = if ($text -eq '') { @() } else { $text -split "`r?`n" }
        if ($lines.Count -ne $case.Lines) { throw "GitHub format: expected $($case.Lines) annotation(s) for $($case.Arguments -join ' '), got $($lines.Count): $text" }
        foreach ($line in $lines) {
            if ($line -notmatch '^::(error|warning|notice) file=[^,]*,line=\d+,col=\d+,endLine=\d+,endColumn=\d+,title=[^,]*(,|::)') { throw "GitHub format: not an annotation: $line" }
            if ($line.Contains([char]27)) { throw "GitHub format: emitted ANSI: $line" }
        }
    }
    $comma = (Invoke-Lint $temp @('a, b.verse', '--output-format=github') $null).Output
    if ($comma -notmatch 'file=a%2C b\.verse,') { throw "GitHub format: a comma in a path must be escaped: $comma" }
    $unicode = (Invoke-Lint $temp @('日 space #%.verse', '--select=V1001,V2002', '--output-format=github') $null).Output
    if ($unicode -notmatch 'file=日 space #%25\.verse,') { throw "GitHub format: a percent in a path must be escaped: $unicode" }
    $partial = Invoke-Lint $temp @('.', 'missing.verse', '--output-format=github') $null
    if ($partial.Code -ne 2) { throw "GitHub format: a partial failure must exit 2, got $($partial.Code)" }
    if ($partial.Output -match 'missing') { throw "GitHub format: an execution error reached stdout: $($partial.Output)" }
    if ($partial.Error -notmatch 'missing\.verse') { throw "GitHub format: an execution error must name the path on stderr: $($partial.Error)" }
    Write-Output 'github format: clean/error/warning/suppression/partial-failure/escaping/no-color passed.'
} finally {
    [IO.Directory]::Delete($temp, $true)
}
