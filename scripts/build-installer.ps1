# Builds the Windows installer of Prism (T10, F22): the Explorer menu helper first, then the
# app and its NSIS installer through the Tauri CLI. The installer ends up in
# target\release\bundle\nsis\.
#
# Code signing (Q7) is not configured yet: when the certificate is chosen, set
# bundle.windows.signCommand (or certificateThumbprint) in crates\prism-app\tauri.conf.json.
#
# Automatic updates (F22-AC3, REVIEW U7) are built in only when these variables are set:
#   PRISM_UPDATE_PUBKEY            public key of the update signatures (`tauri signer generate`)
#   PRISM_UPDATE_URL               https address of latest.json, e.g.
#                                  https://github.com/caste9612/Prism/releases/latest/download/latest.json
#   TAURI_SIGNING_PRIVATE_KEY      the private key (or its path) that signs the installer
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD   its password, if it has one
# Then the installer is signed (.sig next to it) and latest.json is written next to it: both
# are published with the installer in the release named v<version>.

param(
    # Where the installer of this version is downloaded from; {version} is replaced.
    [string]$DownloadBase = 'https://github.com/caste9612/Prism/releases/download/v{version}',
    # What the update notice shows about this version.
    [string]$Notes = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$updates = [bool]$env:TAURI_SIGNING_PRIVATE_KEY
# JSON files without a byte order mark: the updater and the Tauri CLI read them as plain UTF-8.
$utf8 = [Text.UTF8Encoding]::new($false)
if ($updates) {
    if (-not $env:PRISM_UPDATE_PUBKEY -or -not ($env:PRISM_UPDATE_URL -like 'https://*')) {
        throw 'TAURI_SIGNING_PRIVATE_KEY is set: PRISM_UPDATE_PUBKEY and an https PRISM_UPDATE_URL are needed too'
    }
} elseif ($env:PRISM_UPDATE_PUBKEY -or $env:PRISM_UPDATE_URL) {
    throw 'PRISM_UPDATE_PUBKEY/PRISM_UPDATE_URL are set but TAURI_SIGNING_PRIVATE_KEY is not: the update could not be signed'
}

Write-Host '==> prism-shell (release)'
cargo build --release --locked -p prism-shell
if ($LASTEXITCODE -ne 0) { throw 'prism-shell build failed' }

Write-Host '==> interface dependencies'
if (-not (Test-Path 'ui\node_modules')) { npm --prefix ui ci; if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' } }

$conf = Get-Content 'crates\prism-app\tauri.conf.json' -Raw | ConvertFrom-Json
$version = $conf.version
$installer = Join-Path $root "target\release\bundle\nsis\Prism_${version}_x64-setup.exe"

Write-Host "==> Prism $version and its installer$(if ($updates) { ' (with signed updates)' })"
$extra = @()
if ($updates) {
    $overlay = Join-Path $root 'target\updater.conf.json'
    New-Item -ItemType Directory -Force (Split-Path $overlay) | Out-Null
    # The Tauri CLI checks the new signature against the configured public key, which is empty
    # in tauri.conf.json: the overlay gives it for this build only.
    $overlayConf = [ordered]@{
        bundle  = [ordered]@{ createUpdaterArtifacts = $true }
        plugins = [ordered]@{ updater = [ordered]@{ pubkey = $env:PRISM_UPDATE_PUBKEY; endpoints = @($env:PRISM_UPDATE_URL) } }
    }
    [IO.File]::WriteAllText($overlay, ($overlayConf | ConvertTo-Json -Depth 5), $utf8)
    $extra = @('--config', $overlay)
}
Push-Location 'crates\prism-app'
try {
    & "$root\ui\node_modules\.bin\tauri.cmd" build --bundles nsis @extra
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
} finally {
    Pop-Location
}
if (-not (Test-Path $installer)) { throw "installer not found: $installer" }
Write-Host "Installer: $(Resolve-Path $installer)"

if ($updates) {
    $sig = "$installer.sig"
    if (-not (Test-Path $sig)) { throw "update signature not found: $sig" }
    $name = Split-Path $installer -Leaf
    $manifest = [ordered]@{
        version   = $version
        notes     = $Notes
        pub_date  = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        platforms = [ordered]@{
            'windows-x86_64' = [ordered]@{
                signature = (Get-Content $sig -Raw).Trim()
                url       = "$($DownloadBase.Replace('{version}', $version))/$name"
            }
        }
    }
    $latest = Join-Path (Split-Path $installer) 'latest.json'
    [IO.File]::WriteAllText($latest, ($manifest | ConvertTo-Json -Depth 5), $utf8)
    Write-Host "Signature: $(Resolve-Path $sig)"
    Write-Host "Update manifest: $(Resolve-Path $latest)"
    Write-Host "Publish the installer, its .sig and latest.json in the release v$version."
}
