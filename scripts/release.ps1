# Bump version, build installer, launch it so you can update.
# Usage: .\scripts\release.ps1 [-Bump patch|minor|major|none] [-Version 1.2.3] [-NoInstall]
param(
  [ValidateSet('patch', 'minor', 'major', 'none')][string]$Bump = 'patch',
  [string]$Version,
  [switch]$NoInstall
)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$confPath = 'src-tauri/tauri.conf.json'
$conf = Get-Content $confPath -Raw | ConvertFrom-Json
$cur = [version]$conf.version

if ($Version) { $new = $Version }
else {
  switch ($Bump) {
    'major' { $new = "$($cur.Major + 1).0.0" }
    'minor' { $new = "$($cur.Major).$($cur.Minor + 1).0" }
    'patch' { $new = "$($cur.Major).$($cur.Minor).$($cur.Build + 1)" }
    'none'  { $new = $conf.version }
  }
}
Write-Host "Version: $($conf.version) -> $new"

function Set-FileText($path, $pattern, $replacement) {
  $t = Get-Content $path -Raw
  $t = [regex]::Replace($t, $pattern, $replacement, 1)
  [System.IO.File]::WriteAllText((Join-Path $root $path), $t, (New-Object System.Text.UTF8Encoding $false))
}
Set-FileText $confPath '"version":\s*"[^"]*"' "`"version`": `"$new`""
Set-FileText 'package.json' '"version":\s*"[^"]*"' "`"version`": `"$new`""
Set-FileText 'src-tauri/Cargo.toml' '(?m)^version\s*=\s*"[^"]*"' "version = `"$new`""

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
