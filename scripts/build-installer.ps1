# Builds the Windows installer of Prism (T10, F22): the Explorer menu helper first, then the
# app and its NSIS installer through the Tauri CLI. The installer ends up in
# target\release\bundle\nsis\.
#
# Code signing (Q7) is not configured yet: when the certificate is chosen, set
# bundle.windows.signCommand (or certificateThumbprint) in crates\prism-app\tauri.conf.json.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Host '==> prism-shell (release)'
cargo build --release --locked -p prism-shell
if ($LASTEXITCODE -ne 0) { throw 'prism-shell build failed' }

Write-Host '==> interface dependencies'
if (-not (Test-Path 'ui\node_modules')) { npm --prefix ui ci; if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' } }

Write-Host '==> Prism and its installer'
Push-Location 'crates\prism-app'
try {
    & "$root\ui\node_modules\.bin\tauri.cmd" build --bundles nsis
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally {
    Pop-Location
}

Get-ChildItem 'target\release\bundle\nsis\*.exe' | ForEach-Object { Write-Host "Installer: $($_.FullName)" }
