# Opens a Windows Sandbox with the installer of Prism on its desktop, to try the installation
# on a clean Windows (REVIEW U3, checklist T10) without touching this PC. Everything done in the
# sandbox is thrown away when its window is closed.
#
# Windows Sandbox must be enabled once, from an elevated PowerShell, followed by a restart:
#   Enable-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM -All
#
# The installer comes from scripts\build-installer.ps1. Its folder is shared read-only.

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$sandbox = Join-Path $env:WINDIR 'System32\WindowsSandbox.exe'
if (-not (Test-Path $sandbox)) {
    throw 'Windows Sandbox is not enabled: see the comment at the top of this script'
}
$bundle = Join-Path $root 'target\release\bundle\nsis'
if (-not (Get-ChildItem $bundle -Filter 'Prism_*_x64-setup.exe' -ErrorAction SilentlyContinue)) {
    throw "no installer in $bundle`: run scripts\build-installer.ps1 first"
}

$inside = 'C:\Users\WDAGUtilityAccount\Desktop\Prism'
$config = @"
<Configuration>
  <MappedFolders>
    <MappedFolder>
      <HostFolder>$bundle</HostFolder>
      <SandboxFolder>$inside</SandboxFolder>
      <ReadOnly>true</ReadOnly>
    </MappedFolder>
  </MappedFolders>
  <LogonCommand>
    <Command>explorer.exe $inside</Command>
  </LogonCommand>
</Configuration>
"@
$wsb = Join-Path $root 'target\prism-sandbox.wsb'
[IO.File]::WriteAllText($wsb, $config, [Text.UTF8Encoding]::new($false))
Start-Process $wsb
Write-Host "Sandbox started with $wsb"
