<#
.SYNOPSIS
    Removes the Prism development admin test runner (docs/TESTING.md section 4).

.DESCRIPTION
    Run from an elevated PowerShell:

        .\scripts\admin\uninstall-admin-runner.ps1              # remove task, detach test disk
        .\scripts\admin\uninstall-admin-runner.ps1 -DeleteDisk  # ...and delete the VHDX file
#>
[CmdletBinding()]
param(
    [string]$DevRoot = (Join-Path $env:LOCALAPPDATA 'PrismDev'),
    [switch]$DeleteDisk
)
$ErrorActionPreference = 'Stop'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    throw 'Run this script from an elevated PowerShell (Run as administrator).'
}

$TaskName = 'PrismDevAdminTests'
$vhdx = Join-Path $DevRoot 'prism-test.vhdx'

if (Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue) {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false
    Write-Host "Scheduled task '$TaskName' removed." -ForegroundColor Green
} else {
    Write-Host "Scheduled task '$TaskName' not found."
}

if (Test-Path $vhdx) {
    if (Get-Volume -FileSystemLabel 'PRISMTEST' -ErrorAction SilentlyContinue) {
        $script = Join-Path $env:TEMP "prism-diskpart-$PID.txt"
        Set-Content -Path $script -Value @("select vdisk file=`"$vhdx`"", 'detach vdisk') -Encoding ASCII
        & diskpart.exe /s $script | Out-Null
        Remove-Item $script -ErrorAction SilentlyContinue
        Write-Host 'Test disk detached.' -ForegroundColor Green
    }
    if ($DeleteDisk) {
        Remove-Item $vhdx
        Write-Host "Test disk $vhdx deleted." -ForegroundColor Green
    }
}
