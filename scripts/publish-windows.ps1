#Requires -Version 5.1
<#
.SYNOPSIS
    Builds the Windows desktop installers.

.DESCRIPTION
    The Tauri CLI drives the whole chain: it builds the React frontend the
    desktop host embeds, compiles the host in release, and bundles the result.
    Artifacts land under target/release/bundle.
#>
$ErrorActionPreference = "Stop"
$RootDir = Split-Path -Parent $PSScriptRoot

Push-Location (Join-Path $RootDir 'web')
try {
    if (-not (Test-Path 'node_modules')) {
        npm ci
        if ($LASTEXITCODE -ne 0) { throw 'npm ci failed.' }
    }

    npm run tauri -- build
    if ($LASTEXITCODE -ne 0) { throw 'Tauri build failed.' }
}
finally {
    Pop-Location
}

Write-Host "Installers written to $(Join-Path $RootDir 'target\release\bundle')" -ForegroundColor Green
