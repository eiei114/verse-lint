# Works in a fresh PowerShell shell. Leaves build artifacts for further inspection.
param(
    # Forwarded to verify-baseline-parity.ps1 so a PR that intentionally changes
    # rule output can name its pre-slice baseline instead of deleting the gate.
    [string]$BaseRef = '',
    [string]$BaselineBinary = ''
)
$ErrorActionPreference = 'Stop'
$saved = @{ Path = $env:Path; RUSTUP_TOOLCHAIN = $env:RUSTUP_TOOLCHAIN; RUSTC = $env:RUSTC; RUSTDOC = $env:RUSTDOC }
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & "$PSScriptRoot/verify-grammar.ps1"
    # The call operator does not propagate a child script's exit code on its own.
    & "$PSScriptRoot/verify-parity.ps1"
    if ($LASTEXITCODE -ne 0) { throw 'Declared parity check failed' }
    $match = Select-String -Path rust-toolchain.toml -Pattern '^channel\s*=\s*"([^"]+)"'
    if (-not $match) { throw 'No pinned Rust channel found' }
    $channel = $match.Matches[0].Groups[1].Value
    $installed = @(rustup toolchain list)
    if ($LASTEXITCODE -ne 0) { throw 'rustup unavailable' }
    if (-not ($installed | Where-Object { $_.StartsWith("$channel-") })) {
        rustup toolchain install $channel --profile minimal
        if ($LASTEXITCODE -ne 0) { throw 'Cannot install pinned toolchain' }
    }
    rustup component add --toolchain $channel rustfmt clippy
    if ($LASTEXITCODE -ne 0) { throw 'Pinned Rust components unavailable' }
    $rustc = (rustup which --toolchain $channel rustc)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot resolve pinned compiler' }
    $bin = Split-Path -Parent $rustc.Trim()
    # mise/direct Rust installations may precede rustup proxies on PATH.
    $env:Path = $bin + [IO.Path]::PathSeparator + $saved.Path
    $env:RUSTUP_TOOLCHAIN = $channel
    $env:RUSTC = Join-Path $bin 'rustc.exe'
    $env:RUSTDOC = Join-Path $bin 'rustdoc.exe'
    rustc --version --verbose
    if ($LASTEXITCODE -ne 0) { throw 'Rust toolchain unavailable' }
    cargo fmt --all -- --check
    if ($LASTEXITCODE -ne 0) { throw 'cargo fmt failed' }
    cargo clippy --all-targets --locked -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'cargo clippy failed' }
    cargo test --locked
    if ($LASTEXITCODE -ne 0) { throw 'cargo test failed' }
    cargo build --release --locked --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) { throw 'Windows release build failed' }
    # The sweep's accept/reject decision is checked on its own truth table first:
    # the comparison it gates is a PowerShell condition, and a wrong one there
    # would silently pass the cases it exists to fail. The check builds nothing.
    & "$PSScriptRoot/verify-baseline-parity.ps1" -SelfTest
    if ($LASTEXITCODE -ne 0) { throw 'Baseline parity comparison self-check failed' }
    # Primary regression gate for the retained-CST slice: the pre-slice binary
    # and this HEAD must produce byte-identical text and JSON for every
    # self-authored fixture, and SARIF identical except for registry-derived
    # driver-rule additions (see verify-baseline-parity.ps1). A mismatch fails
    # verification.
    $parity = @{ Profile = 'release'; ToolchainBin = $bin }
    if ($BaseRef) { $parity.BaseRef = $BaseRef }
    if ($BaselineBinary) { $parity.BaselineBinary = $BaselineBinary }
    & "$PSScriptRoot/verify-baseline-parity.ps1" @parity
    if ($LASTEXITCODE -ne 0) { throw 'Baseline output parity failed' }
    & "$PSScriptRoot/verify-json.ps1"
    # The Python harness is a developer tool, not a runtime dependency; skip it
    # explicitly rather than silently instead of failing on a Python-less host.
    $python = Get-Command python -ErrorAction SilentlyContinue
    if ($python) {
        & $python.Source -X utf8 "$PSScriptRoot/../tests/test_parity_pin.py"
        if ($LASTEXITCODE -ne 0) { throw 'Parity pin self-check failed' }
    } else {
        Write-Output 'skipped: parity pin self-check (python not on PATH)'
    }
    Write-Output 'Verification passed (UEFN acceptance is a separate gate).'
} finally {
    foreach ($key in $saved.Keys) { [Environment]::SetEnvironmentVariable($key, $saved[$key], 'Process') }
    Pop-Location
}
