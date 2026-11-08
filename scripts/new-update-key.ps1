#Requires -Version 7
# Creates the key pair that signs the updates of Prism (REVIEW U7, ARCHITECTURE §8.1), as for
# Orbit: the private key, protected by a password, in ~\.tauri\prism-updater.key, and a copy of
# key and password in the secrets of the GitHub environment "release", where the Release
# workflow reads them. The public key goes in crates\prism-app\tauri.conf.json.
#
# Run it once, with gh logged in. It asks for the password, or with -GeneratePassword makes a
# random one and saves it in ~\.tauri\prism-updater.key.password.txt (never printed).
# Back up the key file and its password together, outside this PC (a password manager with the
# file attached): if they are lost, the installed copies of Prism can no longer be updated.

param(
    [string]$KeyPath = (Join-Path $HOME '.tauri\prism-updater.key'),
    [string]$Repo = 'caste9612/Prism',
    [switch]$GeneratePassword
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$tauri = Join-Path $root 'ui\node_modules\.bin\tauri.cmd'

if (Test-Path -LiteralPath $KeyPath) {
    throw "$KeyPath exists already: a new key would leave the installed copies without updates (ARCHITECTURE §8.1)"
}
if (-not (Test-Path -LiteralPath $tauri)) { throw 'run npm ci in ui first' }

$passwordFile = "$KeyPath.password.txt"
if ($GeneratePassword) {
    # 32 characters from a cryptographic generator: letters and digits only, so it survives being
    # pasted anywhere.
    $alphabet = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz23456789'.ToCharArray()
    $bytes = [Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
    $password = -join ($bytes | ForEach-Object { $alphabet[$_ % $alphabet.Length] })
} else {
    $first = Read-Host 'Password of the new key' -AsSecureString
    $again = Read-Host 'Same password again' -AsSecureString
    $password = [Net.NetworkCredential]::new('', $first).Password
    if ($password -ne [Net.NetworkCredential]::new('', $again).Password) { throw 'the two passwords differ' }
    if ($password.Length -lt 12) { throw 'use a password of at least 12 characters' }
}

New-Item -ItemType Directory -Force (Split-Path $KeyPath) | Out-Null
& $tauri signer generate --ci -p $password -w $KeyPath | Out-Null
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $KeyPath)) { throw 'key generation failed' }
if ($GeneratePassword) {
    [IO.File]::WriteAllText($passwordFile, $password)
}

Write-Host "==> secrets of the environment release in $Repo"
(Get-Content -LiteralPath $KeyPath -Raw).Trim() | gh secret set TAURI_SIGNING_PRIVATE_KEY --env release --repo $Repo
if ($LASTEXITCODE -ne 0) { throw 'gh secret set TAURI_SIGNING_PRIVATE_KEY failed' }
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release --repo $Repo --body $password
if ($LASTEXITCODE -ne 0) { throw 'gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD failed' }

Write-Host "Key: $KeyPath"
Write-Host "Public key: $KeyPath.pub"
if ($GeneratePassword) { Write-Host "Password: $passwordFile" }
Write-Host 'Now back up the key file and its password together, outside this PC.'
