# Build installer at current version, launch it so you can update.
# Versioning is done by the GitHub release workflow.
# Usage: .\scripts\release.ps1 [-NoInstall]
param(
  [switch]$NoInstall
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$new = (Get-Content 'src-tauri/tauri.conf.json' -Raw | ConvertFrom-Json).version

# Stop running app so build can overwrite the exe
Get-Process dauclem -ErrorAction SilentlyContinue | Stop-Process -Force

npm run build
if ($LASTEXITCODE -ne 0) { throw 'build failed' }

$bundle = Join-Path $root 'src-tauri/target/release/bundle'
$installer = Get-ChildItem "$bundle/nsis/*_${new}_*-setup.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $installer) { $installer = Get-ChildItem "$bundle/msi/*_${new}_*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1 }
if (-not $installer) { throw "No installer for $new found under $bundle" }

Write-Host "Built: $($installer.FullName)"
if (-not $NoInstall) {
  Write-Host 'Launching installer...'
  Start-Process $installer.FullName
}
