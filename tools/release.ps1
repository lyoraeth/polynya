# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# Release files for one architecture, into target/dist:
#   Polynya-<version>-<arch>-setup.exe     the installer (installer/build.ps1)
#   Polynya-<version>-<arch>-portable.zip  the core with the `portable` marker, runs where unpacked
#   Polynya-<version>-<arch>-modules.zip   modules for the portable copy: unpack next to polynya.exe
#
#   ./tools/release.ps1 -Arch x64 -Bin target/release

param(
    [ValidateSet('x64', 'arm64')] [string]$Arch = 'x64',
    [string]$Bin = 'target/release'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$binDir = (Resolve-Path (Join-Path $root $Bin)).Path
$version = (Get-Item (Join-Path $binDir 'polynya.exe')).VersionInfo.ProductVersion
$dist = Join-Path $root 'target\dist'
New-Item -ItemType Directory -Force $dist | Out-Null

# The shipped modules; `start` and `devtools` are not released yet (src/modules.rs, BUNDLED).
$modules = 'agents', 'picker', 'notify'

$installer = & (Join-Path $root 'installer\build.ps1') -Arch $Arch -Bin $Bin | Select-Object -Last 1
Copy-Item $installer $dist -Force

$stage = Join-Path $root "target\stage-$Arch"
foreach ($kind in 'portable', 'modules') {
    $dir = Join-Path $stage $kind
    if (Test-Path $dir) { Remove-Item -Recurse -Force $dir }
    New-Item -ItemType Directory -Force $dir | Out-Null
    Copy-Item (Join-Path $root 'LICENSE') (Join-Path $dir 'LICENSE.txt')
}
Copy-Item (Join-Path $binDir 'polynya.exe') (Join-Path $stage 'portable')
New-Item -ItemType File (Join-Path $stage 'portable\portable') | Out-Null
foreach ($id in $modules) {
    $dir = Join-Path $stage "modules\modules\$id"
    New-Item -ItemType Directory -Force $dir | Out-Null
    Copy-Item (Join-Path $binDir "polynya-$id.exe") $dir
    Copy-Item (Join-Path $root "modules\$id\module.toml") $dir
}

foreach ($kind in 'portable', 'modules') {
    $zip = Join-Path $dist "Polynya-$version-$Arch-$kind.zip"
    if (Test-Path $zip) { Remove-Item $zip }
    Compress-Archive -Path (Join-Path $stage "$kind\*") -DestinationPath $zip
    $zip
}
$installer
