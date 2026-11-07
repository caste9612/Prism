<#
.SYNOPSIS
    One-time setup of the Prism development admin test runner (docs/TESTING.md section 4).

.DESCRIPTION
    Run ONCE from an elevated PowerShell ("Run as administrator"):

        .\scripts\admin\install-admin-runner.ps1

    It does two things:
      1. Creates and mounts a dedicated NTFS test disk: a VHDX file (default 4 GB, dynamically
         expanding) in %LOCALAPPDATA%\PrismDev, labelled PRISMTEST. Only test data lives there.
         diskpart commands always target this VHDX file by path, never a physical disk.
      2. Registers the on-demand scheduled task "PrismDevAdminTests" (no trigger, highest
         privileges) that runs scripts\admin\run-admin-tests.ps1.

    SECURITY: while the task exists, any program running as you can start it, and it runs code
    from this user-writable repository as administrator without a UAC prompt. Remove it with
    scripts\admin\uninstall-admin-runner.ps1 when development is over (or whenever you want).
#>
[CmdletBinding()]
param(
    [string]$DevRoot = (Join-Path $env:LOCALAPPDATA 'PrismDev'),
    [ValidateRange(1, 64)][int]$SizeGB = 4
)
$ErrorActionPreference = 'Stop'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$isAdmin = (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) {
    throw 'Run this script from an elevated PowerShell (Run as administrator).'
}

$TaskName = 'PrismDevAdminTests'
$Label = 'PRISMTEST'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$runner = Join-Path $PSScriptRoot 'run-admin-tests.ps1'
$vhdx = Join-Path $DevRoot 'prism-test.vhdx'
New-Item -ItemType Directory -Force -Path $DevRoot | Out-Null

function Invoke-Diskpart([string[]]$Lines) {
    $script = Join-Path $env:TEMP "prism-diskpart-$PID.txt"
    Set-Content -Path $script -Value $Lines -Encoding ASCII
    try {
        $output = & diskpart.exe /s $script
        if ($LASTEXITCODE -ne 0) {
            throw "diskpart failed (exit $LASTEXITCODE):`n$($output -join "`n")"
        }
    } finally {
        Remove-Item $script -ErrorAction SilentlyContinue
    }
}

# 1. Test disk ---------------------------------------------------------------------------------
$volume = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue
if ($volume) {
    Write-Host "Test volume $Label already mounted at $($volume.DriveLetter):\"
} elseif (Test-Path $vhdx) {
    Write-Host "Attaching existing test disk $vhdx"
    Invoke-Diskpart @("select vdisk file=`"$vhdx`"", 'attach vdisk')
} else {
    Write-Host "Creating test disk $vhdx ($SizeGB GB, expandable, NTFS, label $Label)"
    Invoke-Diskpart @(
        "create vdisk file=`"$vhdx`" maximum=$($SizeGB * 1024) type=expandable",
        "select vdisk file=`"$vhdx`"",
        'attach vdisk',
        'create partition primary',
        "format fs=ntfs label=$Label quick",
        'assign'
    )
}
Start-Sleep -Seconds 2
$volume = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue
if (-not $volume -or -not $volume.DriveLetter) {
    throw "Test volume $Label is not mounted with a drive letter."
}
Write-Host "Test volume ready at $($volume.DriveLetter):\" -ForegroundColor Green

# 2. Scheduled task -----------------------------------------------------------------------------
$action = New-ScheduledTaskAction -Execute 'powershell.exe' `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File `"$runner`"" `
    -WorkingDirectory $repoRoot
$taskPrincipal = New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Highest
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit (New-TimeSpan -Hours 2) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $taskPrincipal -Settings $settings `
    -Description 'Prism development: runs administrator-only tests on demand (docs/TESTING.md section 4).' `
    -Force | Out-Null
Write-Host "Scheduled task '$TaskName' registered." -ForegroundColor Green

Write-Host ''
Write-Host 'Setup complete. To verify it, run from a NORMAL (non-elevated) terminal:'
Write-Host "    schtasks /run /tn $TaskName"
Write-Host "    Get-Content `"$DevRoot\admin-tests\last.status`""
Write-Host ''
Write-Host 'Reminder: while the task exists, code from this repository can run as administrator' -ForegroundColor Yellow
Write-Host 'without a UAC prompt. Remove it with scripts\admin\uninstall-admin-runner.ps1.' -ForegroundColor Yellow
