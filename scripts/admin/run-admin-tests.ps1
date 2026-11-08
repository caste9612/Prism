<#
.SYNOPSIS
    Runs the administrator-only tests of prism-win (docs/TESTING.md section 4).

.DESCRIPTION
    Normally started by the scheduled task "PrismDevAdminTests" (schtasks /run /tn PrismDevAdminTests).
    Can also be run by hand from an elevated PowerShell.

    Output:
      %LOCALAPPDATA%\PrismDev\admin-tests\last.log     full cargo output
      %LOCALAPPDATA%\PrismDev\admin-tests\last.status  "running|passed|failed <code>|error|attached|detached" + timestamp

    The tests only READ real volumes; anything that writes happens on the PRISMTEST test volume,
    whose root is passed to the tests in PRISM_TEST_VOLUME. The test disk is attached for the
    run and detached at the end, when the run attached it: it must not stay in the user's
    Explorer and catalog.

    A request file %LOCALAPPDATA%\PrismDev\admin-tests\request.txt, read and removed at the
    start, changes what a run does (the end-to-end check of the real app needs the disk while
    it runs, docs/TESTING.md section 4bis):
      attach   attach the test disk and leave it attached, no tests
      detach   detach the test disk, no tests
#>
$ErrorActionPreference = 'Stop'

$Label = 'PRISMTEST'
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$devRoot = Join-Path $env:LOCALAPPDATA 'PrismDev'
$outDir = Join-Path $devRoot 'admin-tests'
$log = Join-Path $outDir 'last.log'
$status = Join-Path $outDir 'last.status'
$vhdx = Join-Path $devRoot 'prism-test.vhdx'
New-Item -ItemType Directory -Force -Path $outDir | Out-Null

function Set-Status([string]$Text) {
    Set-Content -Path $status -Value "$Text $(Get-Date -Format o)"
}

# Attaches or detaches the test disk (`attach vdisk` / `detach vdisk`).
function Invoke-VDisk([string]$Verb) {
    $script = Join-Path $env:TEMP "prism-diskpart-$PID.txt"
    Set-Content -Path $script -Value @("select vdisk file=`"$vhdx`"", "$Verb vdisk") -Encoding ASCII
    & diskpart.exe /s $script | Out-Null
    Remove-Item $script -ErrorAction SilentlyContinue
}

$request = Join-Path $outDir 'request.txt'
$mode = 'test'
if (Test-Path $request) {
    $mode = (Get-Content $request -Raw).Trim().ToLowerInvariant()
    Remove-Item $request
}

Set-Status 'running'
Set-Content -Path $log -Value "Prism admin runner ($mode) - $(Get-Date -Format o) - repo $repoRoot"

$attachedHere = $false
try {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $isAdmin = (New-Object Security.Principal.WindowsPrincipal($identity)).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
    if (-not $isAdmin) { throw 'Runner is not elevated.' }
    if ($mode -notin 'test', 'attach', 'detach') { throw "Unknown request '$mode'." }

    $volume = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue
    if ($mode -eq 'detach') {
        if ($volume -and (Test-Path $vhdx)) { Invoke-VDisk 'detach' }
        Set-Status 'detached'
        exit 0
    }

    # Make sure the test volume is attached (VHDX attachments do not survive a reboot).
    if (-not $volume) {
        if (-not (Test-Path $vhdx)) { throw "Test disk $vhdx not found: run install-admin-runner.ps1." }
        Invoke-VDisk 'attach'
        $attachedHere = $true
        Start-Sleep -Seconds 2
        $volume = Get-Volume -FileSystemLabel $Label -ErrorAction SilentlyContinue
    }
    if (-not $volume -or -not $volume.DriveLetter) { throw "Test volume $Label not available." }
    if ($mode -eq 'attach') {
        # Left attached on purpose: the caller detaches it with a "detach" request.
        $attachedHere = $false
        Set-Status 'attached'
        exit 0
    }

    $env:PRISM_TEST_VOLUME = "$($volume.DriveLetter):\"
    $env:PRISM_ADMIN_RUNNER = '1'
    $env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"
    # Separate target dir: elevated builds never touch the normal (non-elevated) build output.
    $env:CARGO_TARGET_DIR = Join-Path $repoRoot 'target\admin'
    Add-Content -Path $log -Value "PRISM_TEST_VOLUME=$env:PRISM_TEST_VOLUME"

    Push-Location $repoRoot
    try {
        # cmd handles the redirection so that cargo's stderr progress is not treated as an error.
        & cmd.exe /c "cargo test -p prism-win --features admin-tests --locked -- --test-threads=1 >> `"$log`" 2>&1"
        $code = $LASTEXITCODE
    } finally {
        Pop-Location
    }

    if ($code -eq 0) { Set-Status 'passed' } else { Set-Status "failed $code" }
    exit $code
} catch {
    Add-Content -Path $log -Value "RUNNER ERROR: $_"
    Set-Status 'error'
    exit 1
} finally {
    # Not left in the user's Explorer and catalog when this run attached it.
    if ($attachedHere) {
        Start-Sleep -Seconds 1
        Invoke-VDisk 'detach'
    }
}
