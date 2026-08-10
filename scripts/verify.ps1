[CmdletBinding()]
param(
    [switch]$SkipWeb,
    [switch]$IncludeDesktop
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repositoryRoot

try {
    Write-Host 'Validating parity fixture JSON...'
    Get-ChildItem -LiteralPath 'tests\parity-fixtures' -Filter '*.json' | ForEach-Object {
        Get-Content -Raw -Encoding UTF8 -LiteralPath $_.FullName | ConvertFrom-Json | Out-Null
    }

    Write-Host 'Checking generated TypeScript contracts for drift...'
    cargo run --locked --package mkvo-contract-gen -- --check
    if ($LASTEXITCODE -ne 0) { throw 'Generated contract drift check failed.' }

    if (-not $SkipWeb) {
        Write-Host 'Building the shared React UI...'
        npm.cmd --prefix web run build
        if ($LASTEXITCODE -ne 0) { throw 'React build failed.' }
    }

    Write-Host 'Checking Rust formatting...'
    cargo fmt --all --check
    if ($LASTEXITCODE -ne 0) { throw 'Rust formatting check failed.' }

    $excluded = @()
    if (-not $IncludeDesktop) {
        $excluded = @('--exclude', 'mkvo-desktop')
    }

    Write-Host 'Linting Rust workspace...'
    cargo clippy --workspace --all-targets @excluded -- -D warnings
    if ($LASTEXITCODE -ne 0) { throw 'Rust lint failed.' }

    Write-Host 'Testing Rust workspace...'
    cargo test --workspace --all-targets @excluded
    if ($LASTEXITCODE -ne 0) { throw 'Rust tests failed.' }

    Write-Host 'Verification passed.' -ForegroundColor Green
}
finally {
    Pop-Location
}
