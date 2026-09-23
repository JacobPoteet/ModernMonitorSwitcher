# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
    Build the NSIS installer.

.DESCRIPTION
    Builds msw.exe, stages it as a Tauri sidecar so it lands next to the
    application, then builds the bundle.

    Tauri expects sidecars to carry the target triple in their filename, and
    strips it on install, so `msw-x86_64-pc-windows-msvc.exe` is installed as
    `msw.exe`.

    Signing the update artifacts needs TAURI_SIGNING_PRIVATE_KEY set. If it
    isn't already in the environment, this falls back to reading it from
    %USERPROFILE%\.tauri\msw-updater.key. Without either, the installer still
    builds, but the result cannot be published as an update because the
    updater will refuse an unsigned package.

    The key has no password, but `tauri build` still stops to prompt for one
    interactively unless told not to, which would hang a non-interactive
    build; `--ci` skips that prompt.
#>

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
Push-Location $root

try {
    $triple = "x86_64-pc-windows-msvc"

    Write-Host "Building msw.exe..." -ForegroundColor Cyan
    cargo build --release -p msw-cli
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for msw-cli" }

    $binaries = Join-Path $root "msw-app/binaries"
    New-Item -ItemType Directory -Force -Path $binaries | Out-Null
    Copy-Item -Force `
        -Path (Join-Path $root "target/release/msw.exe") `
        -Destination (Join-Path $binaries "msw-$triple.exe")

    if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
        $keyFile = Join-Path $env:USERPROFILE ".tauri\msw-updater.key"
        if (Test-Path $keyFile) {
            $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $keyFile -Raw
        } else {
            Write-Warning "TAURI_SIGNING_PRIVATE_KEY is not set; update artifacts will not be signed."
        }
    }

    Write-Host "Building the installer..." -ForegroundColor Cyan
    npx --yes @tauri-apps/cli@^2 build --config msw-app/tauri.conf.json --ci
    if ($LASTEXITCODE -ne 0) { throw "tauri build failed" }

    $bundle = Join-Path $root "target/release/bundle/nsis"
    Write-Host ""
    Write-Host "Done. Artifacts in $bundle" -ForegroundColor Green
    Get-ChildItem $bundle -ErrorAction SilentlyContinue | ForEach-Object {
        "  {0}  ({1:N1} MB)" -f $_.Name, ($_.Length / 1MB)
    }
}
finally {
    Pop-Location
}
