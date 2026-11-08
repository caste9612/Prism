# Runs every check required before a commit (docs/TESTING.md §3).
# Usage: .\scripts\check.ps1
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

function Invoke-Step([string]$Name, [scriptblock]$Command) {
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Command
    if ($LASTEXITCODE -ne 0) {
        Write-Host "FAILED: $Name (exit code $LASTEXITCODE)" -ForegroundColor Red
        exit $LASTEXITCODE
    }
}

Invoke-Step 'cargo fmt' { cargo fmt --all -- --check }
Invoke-Step 'cargo clippy' { cargo clippy --workspace --all-targets --locked -- -D warnings }
# The elevated tests are compiled and linted here too (they run only through the admin runner).
Invoke-Step 'cargo clippy (admin tests)' { cargo clippy -p prism-win --features admin-tests --all-targets --locked -- -D warnings }
Invoke-Step 'cargo test' { cargo test --workspace --locked }

if (Test-Path 'ui/package.json') {
    Push-Location 'ui'
    try {
        Invoke-Step 'ui: type check' { npm run check }
        Invoke-Step 'ui: tests' { npm test }
        # Needs Playwright's Chromium once: npx playwright install chromium --only-shell
        Invoke-Step 'ui: end-to-end' { npm run e2e }
    } finally {
        Pop-Location
    }
}

Write-Host 'All checks passed.' -ForegroundColor Green
