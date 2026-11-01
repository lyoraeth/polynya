# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the legal files shipped with the binaries. They are generated into target/legal and are
# not committed: THIRD-PARTY.md is the hand-written part, cargo-about supplies dependency notices.

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$dir = Join-Path $root 'target\legal'
New-Item -ItemType Directory -Force $dir | Out-Null

$source = "Source code: https://github.com/lyoraeth/polynya`n`n"
$license = Get-Content (Join-Path $root 'LICENSE') -Raw
[System.IO.File]::WriteAllText((Join-Path $dir 'LICENSE.txt'), $source + $license, [System.Text.UTF8Encoding]::new($false))

$generated = Join-Path $dir 'NOTICE.dependencies.txt'
& cargo about generate --workspace --locked (Join-Path $root 'about.hbs') -o $generated
if ($LASTEXITCODE -ne 0) { throw "cargo-about: $LASTEXITCODE" }
$thirdParty = Get-Content (Join-Path $root 'THIRD-PARTY.md') -Raw
$dependencies = Get-Content $generated -Raw
[System.IO.File]::WriteAllText((Join-Path $dir 'NOTICE.txt'), $thirdParty + "`n`n" + $dependencies, [System.Text.UTF8Encoding]::new($false))
Remove-Item $generated -Force
