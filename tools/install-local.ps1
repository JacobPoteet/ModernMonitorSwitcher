# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at http://mozilla.org/MPL/2.0/.

<#
.SYNOPSIS
    Build the installer from this checkout and install it.

.DESCRIPTION
    Installs without a SmartScreen warning, because the installer is built here
    rather than downloaded. SmartScreen only inspects files carrying a
    Mark-of-the-Web, the NTFS stream a browser attaches to anything it
    downloads; a file your own compiler just produced does not have one.

    This is the way to get the first install onto a machine cleanly. After
    that, the application updates itself, and the updater writes its download
    directly rather than through a browser, so updates do not pick up a
    Mark-of-the-Web either.
#>

$ErrorActionPreference = "Stop"

$root = Split-Path -Parent $PSScriptRoot

# Build the installer, including staging the CLI sidecar.
& (Join-Path $PSScriptRoot "build-release.ps1")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

# The bundle folder keeps every installer ever built, so take the newest rather
# than a filename that stops matching at the next version bump.
$bundle = Join-Path $root "target/release/bundle/nsis"
$setup = Get-ChildItem (Join-Path $bundle "*-setup.exe") |
    Sort-Object LastWriteTime |
    Select-Object -Last 1

if (-not $setup) {
    Write-Error "The build finished but left no *-setup.exe in $bundle"
    exit 1
}

$app = Join-Path $env:LOCALAPPDATA "ModernMonitorSwitcher\ModernMonitorSwitcher.exe"

# `/P` is NSIS passive mode: a progress bar, no pages, and a running copy is
# closed without prompting. The install and the relaunch happen in a
# PowerShell of their own so that closing the running application cannot take
# down the shell this script was typed into.
Write-Host "Installing $($setup.Name)..." -ForegroundColor Cyan
$then = "Start-Process '$($setup.FullName)' -ArgumentList '/P' -Wait; Start-Process '$app'"
Start-Process powershell -WindowStyle Hidden -ArgumentList "-NoProfile", "-Command", $then

Write-Host "Installed. Modern Monitor Switcher will start in the tray." -ForegroundColor Green
