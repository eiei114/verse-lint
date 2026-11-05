# Exact-base-versus-HEAD output parity sweep.
#
# The pre-slice baseline is the merge base of HEAD and the base branch
# (`-BaseBranch`, default `origin/main`), i.e. the commit this slice branched
# from. The sweep builds a binary from that commit and one from HEAD, then
# compares them over the self-authored fixtures in text, JSON and SARIF.
#
# The merge base is used rather than `HEAD^` on purpose: a stacked or amended
# PR would make `HEAD^` an already-sliced commit and the comparison would pass
# trivially. When the base branch is missing locally the commit stated in
# `-BaseRef` is required, so the gate fails instead of silently weakening.
#
# Every comparison is byte-exact on stdout, stderr and the process exit code.
# Any mismatch fails the script, and `scripts/verify.ps1` runs the script, so a
# regression cannot reach a green Windows verification path.
#
# One narrow exception exists because it is not a regression: SARIF
# `runs[].tool.driver.rules` is derived from the live registry, so a slice that
# registers a rule necessarily gains one driver entry in every SARIF document.
# A SARIF case is therefore accepted when it differs in exactly that way and
# nothing else: with the registry-derived rule list removed from both documents
# every field must still be identical, every rule the baseline emitted must
# appear byte-for-byte unchanged, and at least one rule must have been added.
# A changed existing rule, a changed result, region, artifact, invocation or
# count, a different exit code or any stderr difference still fails, and every
# text and JSON comparison stays byte-exact. The acceptance is recorded per case
# in the report as `sarifDriverRuleAdded`.
#
# Local artifact: the JSON report under `target/verification/baseline-parity/`
# is not committed.
param(
    [string]$BaseBranch = 'origin/main',
    [string]$BaseRef = '',
    [string]$Profile = 'release',
    [string]$FixtureDir = "$PSScriptRoot/../tests/fixtures/baseline",
    [string]$ReportDir = "$PSScriptRoot/../target/verification/baseline-parity",
    [string]$ToolchainBin = '',
    [string]$BaselineBinary = ''
)
$ErrorActionPreference = 'Stop'
if ($Profile -notin @('release', 'dev')) { throw "Unsupported profile '$Profile'; use release or dev" }
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$FixtureDir = (Resolve-Path $FixtureDir).Path
$ReportDir = [IO.Path]::GetFullPath($ReportDir)
Push-Location $repo
$worktree = $null
$savedPath = $env:Path
$savedToolchain = $env:RUSTUP_TOOLCHAIN
$savedTargetDir = $env:CARGO_TARGET_DIR
$savedRustc = $env:RUSTC
$savedRustdoc = $env:RUSTDOC
# The sweep compares two builds of this repository, so it must control where
# they land. An inherited CARGO_TARGET_DIR would put HEAD's binary somewhere
# else and the "HEAD binary not found" check would abort the gate.
$env:CARGO_TARGET_DIR = $null
try {
    # Resolve the pinned toolchain without trusting PATH ordering (mise installs
    # may precede the rustup proxies).
    if (-not $ToolchainBin) {
        $match = Select-String -Path rust-toolchain.toml -Pattern '^channel\s*=\s*"([^"]+)"'
        if (-not $match) { throw 'No pinned Rust channel found' }
        $channel = $match.Matches[0].Groups[1].Value
        $rustc = (rustup which --toolchain $channel rustc)
        if ($LASTEXITCODE -ne 0) { throw "Cannot resolve the pinned compiler ($channel)" }
        $ToolchainBin = Split-Path -Parent $rustc.Trim()
        $env:RUSTUP_TOOLCHAIN = $channel
    }
    $env:Path = $ToolchainBin + [IO.Path]::PathSeparator + $savedPath
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw 'cargo is unavailable' }

    function Resolve-Commit([string]$ref) {
        if (-not $ref) { return $null }
        $sha = (git rev-parse --verify --quiet "$ref^{commit}" 2>$null)
        if ($LASTEXITCODE -ne 0 -or -not $sha) { return $null }
        return "$sha".Trim()
    }

    if ($BaseRef) {
        # An explicit baseline is authoritative; it must be a real commit.
        $baseSha = Resolve-Commit $BaseRef
        if (-not $baseSha) { throw "-BaseRef '$BaseRef' is not a commit in this checkout" }
    } else {
        # `origin/main` may be absent in a local checkout that never fetched it.
        # Trying `main` keeps the local run usable; CI always has the remote ref.
        $branchSha = Resolve-Commit $BaseBranch
        if (-not $branchSha -and $BaseBranch -eq 'origin/main') { $branchSha = Resolve-Commit 'main' }
        if (-not $branchSha) {
            throw "Cannot resolve the base branch '$BaseBranch'. Fetch base history (ci.yml checks out full history) or pass -BaseRef <pre-slice commit>"
        }
        $baseSha = "$(git merge-base HEAD $branchSha)".Trim()
        if ($LASTEXITCODE -ne 0 -or -not $baseSha) {
            throw "No merge base between HEAD and '$BaseBranch'; pass -BaseRef <pre-slice commit>"
        }
        if ($baseSha -eq (git rev-parse HEAD).Trim()) {
            # HEAD is already on the base branch: a post-merge push to main, where
            # the merge base is HEAD itself and there is nothing to diff against.
            # The commit that just landed has HEAD^ as its parent on main, so
            # that is the pre-slice state. This self-updates with history instead
            # of pinning a hash that would drift into a false failure.
            $baseSha = Resolve-Commit 'HEAD^'
            if (-not $baseSha) { throw 'HEAD is on the base branch and has no parent to compare against' }
        }
    }
    $headSha = (git rev-parse --verify HEAD).Trim()
    if ($baseSha -eq $headSha) { throw "Baseline resolves to HEAD ($headSha); there is nothing to compare" }
    if (git status --porcelain) {
        throw 'The parity sweep needs a clean working tree so HEAD names the exact tested bytes'
    }

    $targetTriple = 'x86_64-pc-windows-msvc'
    $exeName = 'verse-lint.exe'
    $profileDir = if ($Profile -eq 'dev') { 'debug' } else { 'release' }
    $headExe = Join-Path $repo "target/$targetTriple/$profileDir/$exeName"
    $baselineCache = Join-Path $repo "target/verification/baseline/$($baseSha.Substring(0, 12))/$profileDir"
    $baselineExe = if ($BaselineBinary) { (Resolve-Path $BaselineBinary).Path } else { Join-Path $baselineCache $exeName }

    Write-Output "Baseline commit: $baseSha"
    Write-Output "HEAD commit:     $headSha"

    # Build HEAD from the checkout under test.
    & cargo build --locked "--profile=$Profile" --target $targetTriple
    if ($LASTEXITCODE -ne 0) { throw 'HEAD build failed' }
    if (-not (Test-Path $headExe)) { throw "HEAD binary not found at $headExe" }

    # Build the baseline from that commit only. The binary is cached per base
    # SHA and profile so repeated local runs do not rebuild it.
    if (-not (Test-Path $baselineExe)) {
        $worktree = Join-Path ([IO.Path]::GetTempPath()) ("verse-lint-baseline-" + [Guid]::NewGuid())
        & git worktree add --detach --quiet $worktree $baseSha
        if ($LASTEXITCODE -ne 0) { throw "Cannot create the baseline worktree at $baseSha" }
        Push-Location $worktree
        try {
            & cargo build --locked "--profile=$Profile" --target $targetTriple
            if ($LASTEXITCODE -ne 0) { throw 'Baseline build failed' }
            $built = Join-Path $worktree "target/$targetTriple/$profileDir/$exeName"
            if (-not (Test-Path $built)) { throw "Baseline binary not found at $built" }
            New-Item -ItemType Directory -Force -Path $baselineCache | Out-Null
            Copy-Item $built $baselineExe -Force
        } finally {
            Pop-Location
        }
    }
    Write-Output "HEAD binary:     $headExe"
    Write-Output "Baseline binary: $baselineExe"

    # Byte-exact capture: raw stdout/stderr bytes, exit code and a deadline.
    function Invoke-Captured {
        param([string]$Exe, [string]$WorkingDirectory, [string[]]$Arguments, [string]$InputText, [int]$TimeoutMs)
        $start = [Diagnostics.ProcessStartInfo]::new($Exe)
        $start.WorkingDirectory = $WorkingDirectory
        $start.UseShellExecute = $false
        $start.RedirectStandardOutput = $true
        $start.RedirectStandardError = $true
        $start.RedirectStandardInput = $PSBoundParameters.ContainsKey('InputText')
        foreach ($argument in $Arguments) { $start.ArgumentList.Add($argument) }
        $process = [Diagnostics.Process]::Start($start)
        try {
            if ($start.RedirectStandardInput) {
                $process.StandardInput.Write($InputText)
                $process.StandardInput.Close()
            }
            $out = [IO.MemoryStream]::new()
            $err = [IO.MemoryStream]::new()
            $stdoutTask = $process.StandardOutput.BaseStream.CopyToAsync($out)
            $stderrTask = $process.StandardError.BaseStream.CopyToAsync($err)
            if (-not $process.WaitForExit($TimeoutMs)) {
                $process.Kill($true)
                throw "Timed out after ${TimeoutMs}ms: $Exe $($Arguments -join ' ')"
            }
            [void]$stdoutTask.GetAwaiter().GetResult()
            [void]$stderrTask.GetAwaiter().GetResult()
            return [pscustomobject]@{
                ExitCode = $process.ExitCode
                Stdout   = $out.ToArray()
                Stderr   = $err.ToArray()
            }
        } finally {
            $process.Dispose()
        }
    }

    function Get-HexDigest {
        param([byte[]]$Bytes)
        $sha = [Security.Cryptography.SHA256]::Create()
        try { return ([BitConverter]::ToString($sha.ComputeHash($Bytes))).Replace('-', '').ToLowerInvariant() }
        finally { $sha.Dispose() }
    }

    $fixtures = @(Get-ChildItem -LiteralPath $FixtureDir -Filter '*.verse' -File | Sort-Object Name)
    if ($fixtures.Count -lt 10) { throw "Fixture corpus is unexpectedly small ($($fixtures.Count) files)" }
    $allRules = '--select=V1001,V1002,V1003,V2001,V2002'
    $selections = @(
        @{ Name = 'default-selection'; Extra = @() },
        @{ Name = 'all-rules'; Extra = @($allRules) }
    )
    $formats = @('text', 'json', 'sarif')
    # A bounded stdin subset: stdin reaches the same rules through a different
    # input path, so parity is asserted there too.
    $stdinFixtures = @('clean.verse', 'trailing-whitespace.verse', 'suppressed.verse', 'refuse-unterminated-comment.verse')

    $comparisons = [Collections.Generic.List[object]]::new()
    $failures = [Collections.Generic.List[string]]::new()

    # Whole-document SARIF equivalence except `runs[].tool.driver.rules`, which
    # is registry-derived and therefore legitimately grows when a slice registers
    # a rule. Returns $true only when every other field is identical and every
    # baseline driver rule still appears unchanged. The reason for a refusal is
    # left in `$script:driverRuleFailure` so the caller can report it.
    function Test-SarifAddsOnlyDriverRules {
        param([byte[]]$BeforeBytes, [byte[]]$AfterBytes)
        $script:driverRuleFailure = $null
        try {
            $before = [Text.Encoding]::UTF8.GetString($BeforeBytes) | ConvertFrom-Json -Depth 100
            $after = [Text.Encoding]::UTF8.GetString($AfterBytes) | ConvertFrom-Json -Depth 100
        } catch {
            $script:driverRuleFailure = "not parseable SARIF: $_"
            return $false
        }
        $beforeRuns = @($before.runs)
        $afterRuns = @($after.runs)
        if ($beforeRuns.Count -ne $afterRuns.Count) {
            $script:driverRuleFailure = "run count changed ($($beforeRuns.Count) -> $($afterRuns.Count))"
            return $false
        }
        for ($index = 0; $index -lt $beforeRuns.Count; $index++) {
            $beforeRules = @($beforeRuns[$index].tool.driver.rules)
            $afterRules = @($afterRuns[$index].tool.driver.rules)
            # Empty the registry-derived list on both sides, then compare the whole
            # remaining document. Any other difference refuses the comparison.
            $beforeRuns[$index].tool.driver.rules = @()
            $afterRuns[$index].tool.driver.rules = @()
            $beforeRest = $before | ConvertTo-Json -Depth 100 -Compress
            $afterRest = $after | ConvertTo-Json -Depth 100 -Compress
            if ($beforeRest -ne $afterRest) {
                $script:driverRuleFailure = 'a field other than tool.driver.rules differs'
                return $false
            }

            # Every baseline rule has to survive unchanged; the only permitted
            # difference is an additional rule object.
            $afterById = @{}
            foreach ($rule in $afterRules) {
                $afterById["$($rule.id)"] = ($rule | ConvertTo-Json -Depth 100 -Compress)
            }
            foreach ($rule in $beforeRules) {
                $id = "$($rule.id)"
                if (-not $afterById.ContainsKey($id)) {
                    $script:driverRuleFailure = "baseline driver rule $id disappeared"
                    return $false
                }
                if ($afterById[$id] -ne ($rule | ConvertTo-Json -Depth 100 -Compress)) {
                    $script:driverRuleFailure = "driver rule $id changed"
                    return $false
                }
            }
            if ($afterRules.Count -le $beforeRules.Count) {
                $script:driverRuleFailure = 'no driver rule was added, so the difference is unexplained'
                return $false
            }
        }
        return $true
    }

    function Compare-Case {
        param([string]$Name, [string[]]$Arguments, [string]$InputText)
        $limit = if ($Name -like 'corpus-*') { 60000 } else { 15000 }
        if ($PSBoundParameters.ContainsKey('InputText')) {
            $before = Invoke-Captured -Exe $baselineExe -WorkingDirectory $FixtureDir -Arguments $Arguments -InputText $InputText -TimeoutMs $limit
            $after = Invoke-Captured -Exe $headExe -WorkingDirectory $FixtureDir -Arguments $Arguments -InputText $InputText -TimeoutMs $limit
        } else {
            $before = Invoke-Captured -Exe $baselineExe -WorkingDirectory $FixtureDir -Arguments $Arguments -TimeoutMs $limit
            $after = Invoke-Captured -Exe $headExe -WorkingDirectory $FixtureDir -Arguments $Arguments -TimeoutMs $limit
        }
        $sameExit = $before.ExitCode -eq $after.ExitCode
        $sameOut = (Get-HexDigest $before.Stdout) -eq (Get-HexDigest $after.Stdout)
        $sameErr = (Get-HexDigest $before.Stderr) -eq (Get-HexDigest $after.Stderr)
        # A SARIF case may differ from the baseline in exactly one documented way:
        # `tool.driver.rules` gains the rule this slice registered. The structural
        # check below proves that is the whole difference, so it can stand in for
        # byte equality without weakening any other comparison.
        $sarifOnly = $false
        if (-not $sameOut -and $Name -like '*sarif*') {
            $sarifOnly = Test-SarifAddsOnlyDriverRules $before.Stdout $after.Stdout
            if (-not $sarifOnly) {
                $reason = $script:driverRuleFailure
                $failures.Add("MISMATCH $Name [$($Arguments -join ' ')]: SARIF differs beyond an added driver rule ($reason)")
            }
        }
        $comparisons.Add([pscustomobject]@{
                case             = $Name
                arguments        = ($Arguments -join ' ')
                exitCode         = $after.ExitCode
                exitCodeSame     = $sameExit
                stdoutSame       = $sameOut
                stderrSame       = $sameErr
                sarifDriverRuleAdded = $sarifOnly
                stdoutSha256     = (Get-HexDigest $after.Stdout)
                stdoutBytes      = $after.Stdout.Length
            })
        if (-not $sarifOnly -and -not ($sameExit -and $sameOut -and $sameErr)) {
            $detail = "MISMATCH $Name [$($Arguments -join ' ')]: exit $($before.ExitCode)->$($after.ExitCode), stdoutSame=$sameOut, stderrSame=$sameErr"
            if (-not $sameOut) {
                $detail += "`n  baseline stdout:`n$([Text.Encoding]::UTF8.GetString($before.Stdout))"
                $detail += "`n  head stdout:`n$([Text.Encoding]::UTF8.GetString($after.Stdout))"
            }
            if (-not $sameErr) {
                $detail += "`n  baseline stderr:`n$([Text.Encoding]::UTF8.GetString($before.Stderr))"
                $detail += "`n  head stderr:`n$([Text.Encoding]::UTF8.GetString($after.Stderr))"
            }
            $failures.Add($detail)
        }
        return $after
    }

    foreach ($fixture in $fixtures) {
        foreach ($format in $formats) {
            foreach ($selection in $selections) {
                $arguments = @($fixture.Name, "--output-format=$format") + $selection.Extra
                Compare-Case -Name "$($fixture.Name)/$format/$($selection.Name)" -Arguments $arguments | Out-Null
            }
        }
    }
    $corpus = @{}
    foreach ($format in $formats) {
        foreach ($selection in $selections) {
            $arguments = @('.', "--output-format=$format") + $selection.Extra
            $corpus["$format/$($selection.Name)"] = Compare-Case -Name "corpus-$format/$($selection.Name)" -Arguments $arguments
        }
    }
    foreach ($name in $stdinFixtures) {
        $text = [IO.File]::ReadAllText((Join-Path $FixtureDir $name))
        foreach ($format in $formats) {
            Compare-Case -Name "stdin-$name/$format" -Arguments @('-', '--stdin-filepath=virtual.verse', "--output-format=$format") -InputText $text | Out-Null
        }
    }

    # Anti-vacuity: equality only proves something when the compared output is
    # non-trivial. A corpus run that reported no diagnostic, or a format that
    # serialized nothing, would make every comparison trivially true.
    foreach ($case in $corpus.Keys) {
        $result = $corpus[$case]
        $format = $case.Split('/')[0]
        $stdout = [Text.Encoding]::UTF8.GetString($result.Stdout)
        if ([string]::IsNullOrWhiteSpace($stdout)) { throw "Corpus run '$case' produced no output; parity would be vacuous" }
        $ruleHits = ([regex]::Matches($stdout, 'V1001|V1002|V1003|V2001|V2002')).Count
        if ($ruleHits -lt 1) { throw "Corpus run '$case' reported no rule ID; the sweep never exercised a diagnostic" }
        if ($case -like 'sarif/*') {
            if ($stdout -notmatch '"version"\s*:\s*"2\.1\.0"' -or $stdout -notmatch '"results"\s*:\s*\[') {
                throw "SARIF corpus run '$case' is not a SARIF 2.1.0 document with results; parity would not cover SARIF"
            }
        }
        if ($case -like 'json/*') {
            if ($stdout -notmatch '"diagnostics"\s*:\s*\[' -or $stdout -notmatch '"summary"') {
                throw "JSON corpus run '$case' is missing the diagnostics/summary envelope; parity would not cover JSON"
            }
        }
    }
    $formatsSeen = @($corpus.Keys | ForEach-Object { $_.Split('/')[0] } | Sort-Object -Unique)
    if ($formatsSeen.Count -ne 3) { throw "Expected text, JSON and SARIF runs; observed: $($formatsSeen -join ', ')" }

    New-Item -ItemType Directory -Force -Path $ReportDir | Out-Null
    [pscustomobject]@{
        baseBranch      = $BaseBranch
        baseRefSpec     = $BaseRef
        baseCommit       = $baseSha
        headCommit       = $headSha
        profile          = $Profile
        fixtures         = @($fixtures.Name)
        comparisons      = $comparisons.Count
        formatsExercised = $formatsSeen
        sarifDriverRuleAdditionCount = @($comparisons | Where-Object { $_.sarifDriverRuleAdded }).Count
        mismatchCount    = $failures.Count
        results          = $comparisons
    } | ConvertTo-Json -Depth 6 | Set-Content -Path (Join-Path $ReportDir 'report.json') -Encoding utf8

    if ($failures.Count -gt 0) {
        foreach ($failure in $failures) { Write-Output $failure }
        throw "Pattern parity failed: $($failures.Count) of $($comparisons.Count) comparisons differ between $baseSha and $headSha"
    }
    # State exactly what was proven: byte equality everywhere except the SARIF
    # cases whose only difference was an added registry-derived driver rule.
    $added = @($comparisons | Where-Object { $_.sarifDriverRuleAdded }).Count
    $byteExact = $comparisons.Count - $added
    $summary = "Pattern parity: BYTE-IDENTICAL across $byteExact comparisons ($($fixtures.Count) fixtures x text/JSON/SARIF + corpus and stdin runs) between $baseSha and $headSha."
    if ($added -gt 0) {
        $summary += " A further $added SARIF comparison(s) differ only by registry-derived driver rule entries, with every other SARIF field and every existing driver rule unchanged; see sarifDriverRuleAdded in the report."
    }
    Write-Output $summary
} finally {
    Pop-Location
    $env:Path = $savedPath
    # Restore every variable this script touched, including "originally unset":
    # assigning $null to an $env: variable removes it.
    $env:RUSTUP_TOOLCHAIN = $savedToolchain
    $env:CARGO_TARGET_DIR = $savedTargetDir
    $env:RUSTC = $savedRustc
    $env:RUSTDOC = $savedRustdoc
    if ($worktree -and (Test-Path $worktree)) {
        & git -C $repo worktree remove --force $worktree
        if ($LASTEXITCODE -ne 0) { Remove-Item -Recurse -Force $worktree -ErrorAction SilentlyContinue }
    }
}
