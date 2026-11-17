param([ValidateSet('msi','nsis')][string]$Format = 'msi')
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'dev-env.ps1')
Push-Location (Split-Path -Parent $PSScriptRoot)
try {
    & npm.cmd ci
    if ($LASTEXITCODE -ne 0) { throw 'Dependency installation failed' }
    & npm.cmd run tauri -- build --bundles $Format
    if ($LASTEXITCODE -ne 0) { throw 'Packaging failed' }
    Write-Output 'Development monitor/simulator package only. No BIOS writer/service is installed.'
} finally { Pop-Location }
