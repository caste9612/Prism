#Requires -Version 7
# Builds the Windows installer of Prism (T10, F22): the Explorer menu helper first, then the
# app and its NSIS installer through the Tauri CLI. The installer ends up in
# target\release\bundle\nsis\.
#
# Updates (F22-AC3, ARCHITECTURE §8.1), as for Orbit: the installed copies check
# https://github.com/caste9612/Prism/releases/latest/download/latest.json and install a newer
# version only if its signature matches the public key in crates\prism-app\tauri.conf.json.
# So this script signs the installer with the private key (-KeyPath, outside the repo; in the
# Release workflow it comes from the secrets of the environment "release") and writes
# latest.json next to it, with the notes from docs\releases\<version>.md. The password of the key
# is read from TAURI_SIGNING_PRIVATE_KEY_PASSWORD or asked without showing it, and tried before
# compiling. Installer, .sig and latest.json are the files of the release v<version>.
# -NoUpdateSignature builds an installer for trying it locally, which cannot be published.
#
# Code signing (Q7, REVIEW U1) is not configured: when a certificate is chosen, set
# bundle.windows.signCommand (or certificateThumbprint) in crates\prism-app\tauri.conf.json.

param(
    # The private key of the update signatures, protected by a password.
    [string]$KeyPath = (Join-Path $HOME '.tauri\prism-updater.key'),
    # Build without the update signature and latest.json (local tries only).
    [switch]$NoUpdateSignature
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
$tauri = Join-Path $root 'ui\node_modules\.bin\tauri.cmd'

# --- the version must be the same everywhere --------------------------------------------------
$conf = Get-Content 'crates\prism-app\tauri.conf.json' -Raw | ConvertFrom-Json
$version = $conf.version
$uiLock = Get-Content 'ui\package-lock.json' -Raw | ConvertFrom-Json -AsHashtable
$found = [ordered]@{
    'crates\prism-app\tauri.conf.json'  = $version
    'Cargo.toml (workspace)'            = [regex]::Match((Get-Content 'Cargo.toml' -Raw), '(?m)^version = "([^"]+)"').Groups[1].Value
    'Cargo.lock (prism-app)'            = [regex]::Match((Get-Content 'Cargo.lock' -Raw), '(?m)^name = "prism-app"\r?\nversion = "([^"]+)"').Groups[1].Value
    'ui\package.json'                   = (Get-Content 'ui\package.json' -Raw | ConvertFrom-Json).version
    'ui\package-lock.json'              = $uiLock.version
    'ui\package-lock.json (packages."")' = $uiLock.packages[''].version
}
$wrong = $found.GetEnumerator() | Where-Object { $_.Value -ne $version }
if ($wrong) {
    throw "the version is not the same everywhere:`n$(($found.GetEnumerator() | ForEach-Object { "  $($_.Key): $($_.Value)" }) -join "`n")"
}

$signed = -not $NoUpdateSignature
$notesPath = Join-Path $root "docs\releases\$version.md"
$askedPassword = $false
if ($signed) {
    if (-not (Test-Path -LiteralPath $notesPath)) { throw "the release notes are missing: $notesPath" }
    if (-not (Test-Path -LiteralPath $KeyPath)) {
        throw "the private key of the updates is missing: $KeyPath (scripts\new-update-key.ps1 creates it; -NoUpdateSignature builds without it)"
    }
    if (-not ($conf.plugins.updater.pubkey)) { throw 'plugins.updater.pubkey is empty in tauri.conf.json' }
}

Write-Host '==> interface dependencies'
if (-not (Test-Path 'ui\node_modules')) { npm --prefix ui ci; if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' } }

if ($signed) {
    # Asked once and kept only in this process; tried at once by signing a test file, so a wrong
    # password does not cost a build.
    if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
        if ([Console]::IsInputRedirected) {
            $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
        } else {
            $secure = Read-Host 'Password of the update key' -AsSecureString
            $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Net.NetworkCredential]::new('', $secure).Password
        }
        $askedPassword = $true
    }
    $probe = Join-Path ([IO.Path]::GetTempPath()) "prism-sign-probe-$PID.txt"
    [IO.File]::WriteAllText($probe, 'probe')
    try {
        & $tauri signer sign -f $KeyPath $probe *> $null
        $opened = $LASTEXITCODE -eq 0
    } finally {
        Remove-Item -LiteralPath $probe, "$probe.sig" -ErrorAction SilentlyContinue
    }
    if (-not $opened) {
        if ($askedPassword) { Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue }
        throw "the key $KeyPath does not open with this password (without a terminal, set TAURI_SIGNING_PRIVATE_KEY_PASSWORD)"
    }
}

Write-Host '==> prism-shell (release)'
cargo build --release --locked -p prism-shell
if ($LASTEXITCODE -ne 0) { throw 'prism-shell build failed' }

$installer = Join-Path $root "target\release\bundle\nsis\Prism_${version}_x64-setup.exe"
$latest = Join-Path (Split-Path $installer) 'latest.json'
# A signature or manifest of an earlier build must not be taken for this one.
Remove-Item -LiteralPath "$installer.sig", $latest -ErrorAction SilentlyContinue

Write-Host "==> Prism $version and its installer$(if ($signed) { ' (signed for the updates)' })"
# JSON files without a byte order mark: the updater and the Tauri CLI read them as plain UTF-8.
$utf8 = [Text.UTF8Encoding]::new($false)
$extra = @()
if ($signed) {
    # The update artifacts only here: a plain build has no key.
    $overlay = Join-Path ([IO.Path]::GetTempPath()) "prism-release-$PID.json"
    [IO.File]::WriteAllText($overlay, '{ "bundle": { "createUpdaterArtifacts": true } }', $utf8)
    $extra = @('--config', $overlay)
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content -LiteralPath $KeyPath -Raw).Trim()
}
Push-Location 'crates\prism-app'
try {
    & $tauri build --bundles nsis @extra
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally {
    Pop-Location
    if ($signed) {
        Remove-Item -LiteralPath $overlay -ErrorAction SilentlyContinue
        Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
        if ($askedPassword) { Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue }
    }
}
if (-not (Test-Path $installer)) { throw "installer not found: $installer" }
Write-Host "Installer: $(Resolve-Path $installer)"

if ($signed) {
    $sig = "$installer.sig"
    if (-not (Test-Path $sig)) { throw "update signature not found: $sig" }
    $name = Split-Path $installer -Leaf
    # What the update notice in Settings shows: the first paragraph of the notes.
    $notes = ((Get-Content -LiteralPath $notesPath -Raw) -split '\r?\n\s*\r?\n')[0] -replace '\s+', ' '
    $manifest = [ordered]@{
        version   = $version
        notes     = $notes.Trim()
        pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{
                signature = (Get-Content $sig -Raw).Trim()
                url       = "https://github.com/caste9612/Prism/releases/download/v$version/$name"
            }
        }
    }
    [IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 5), $utf8)
    Write-Host "Signature: $(Resolve-Path $sig)"
    Write-Host "Update manifest: $(Resolve-Path $latest)"
    Write-Host "Publish the installer, its .sig and latest.json in the release v$version."
}
