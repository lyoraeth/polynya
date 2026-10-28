# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the identity package of the notifications module: modules/notify/package → modules/notify/polynya-notify.msix.
# The built .msix is committed and embedded into the core (src/package.rs), so rebuild it only
# after changing the manifest or the logo.
#
# makeappx comes from the Microsoft.Windows.SDK.BuildTools NuGet package; no Windows SDK needed.

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$version = '10.0.28000.2526'
$tools = Join-Path $root "target\buildtools\$version"
$makeappx = Join-Path $tools "bin\10.0.28000.0\x64\makeappx.exe"

if (-not (Test-Path $makeappx)) {
    New-Item -ItemType Directory -Force $tools | Out-Null
    $zip = Join-Path $tools 'buildtools.zip'
    Invoke-WebRequest "https://api.nuget.org/v3-flatcontainer/microsoft.windows.sdk.buildtools/$version/microsoft.windows.sdk.buildtools.$version.nupkg" -OutFile $zip
    Expand-Archive $zip $tools -Force
}

& $makeappx pack /d (Join-Path $root 'modules\notify\package') /p (Join-Path $root 'modules\notify\polynya-notify.msix') /nv /o
if ($LASTEXITCODE -ne 0) { throw "makeappx: $LASTEXITCODE" }
