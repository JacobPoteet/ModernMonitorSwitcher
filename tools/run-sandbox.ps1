# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
    Build this checkout and run it without touching your real settings.

.DESCRIPTION
    Settings, profiles and the log all live under %APPDATA%, so this runs the
    application with %APPDATA% pointed at a scratch folder instead, and gives
    the WebView its own data folder there too. MSW_SANDBOX tells the application to
    leave alone what does not live there: it skips the single-instance lock so
    it runs beside an installed copy, refuses to change Start with Windows, and
    never installs an update.

    The sandbox starts empty each time, so the first-run guide shows on every
    launch. Pass -Keep to carry profiles and settings over from the last run.

    Switching profiles still changes your real displays; that is the thing
    being tested. Restore Windows layout, or `msw reset`, undoes it.

.PARAMETER Keep
    Reuse the previous sandbox rather than starting from nothing.

.EXAMPLE
    powershell -ExecutionPolicy Bypass -File tools/run-sandbox.ps1
#>

param(
    [switch]$Keep
)

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot
$sandbox = Join-Path $env:TEMP "msw-sandbox"

Push-Location $root
try {
    # tauri-build refuses to build unless the CLI sidecar is staged, which a
    # fresh checkout will not have. Any build of it will do for running.
    $sidecar = Join-Path $root "msw-app/binaries/msw-x86_64-pc-windows-msvc.exe"
    if (-not (Test-Path $sidecar)) {
        Write-Host "Staging msw.exe..." -ForegroundColor Cyan
        cargo build -p msw-cli
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed for msw-cli" }
        New-Item -ItemType Directory -Force -Path (Split-Path $sidecar) | Out-Null
        Copy-Item -Force (Join-Path $root "target/debug/msw.exe") $sidecar
    }

    # Built with the real environment, so cargo and rustup find their own
    # files; only the application itself runs with the redirected one.
    Write-Host "Building..." -ForegroundColor Cyan
    cargo build -p msw-app
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed for msw-app" }
}
finally {
    Pop-Location
}

if (-not $Keep -and (Test-Path $sandbox)) {
    Remove-Item -Recurse -Force $sandbox
}
$roaming = Join-Path $sandbox "Roaming"
$webview = Join-Path $sandbox "WebView2"
New-Item -ItemType Directory -Force -Path $roaming, $webview | Out-Null

Write-Host "Running from $sandbox" -ForegroundColor Green
Write-Host "Log: $(Join-Path $roaming 'ModernMonitorSwitcher\msw.log')"
Write-Host "Quit from the tray icon to end the run."

# Run as `.\tools\run-sandbox.ps1` this is the caller's own shell, so put the
# environment back afterwards rather than leave it pointing at the sandbox.
$saved = @{
    APPDATA                   = $env:APPDATA
    WEBVIEW2_USER_DATA_FOLDER = $env:WEBVIEW2_USER_DATA_FOLDER
    MSW_SANDBOX               = $env:MSW_SANDBOX
}
try {
    $env:APPDATA = $roaming
    # Not %LOCALAPPDATA%: WebView2 will not start at all with that moved.
    $env:WEBVIEW2_USER_DATA_FOLDER = $webview
    $env:MSW_SANDBOX = "1"

    & (Join-Path $root "target/debug/ModernMonitorSwitcher.exe")
}
finally {
    $env:APPDATA = $saved.APPDATA
    $env:WEBVIEW2_USER_DATA_FOLDER = $saved.WEBVIEW2_USER_DATA_FOLDER
    $env:MSW_SANDBOX = $saved.MSW_SANDBOX
}
