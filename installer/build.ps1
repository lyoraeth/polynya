# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# Builds the installer: target/installer/Polynya-<version>-<arch>-setup.exe.
#   ./installer/build.ps1 -Arch x64 -Bin target/release
# Needs Inno Setup 6.5+ (preinstalled on GitHub's windows-latest; locally `winget install JRSoftware.InnoSetup`).

param(
    [ValidateSet('x64', 'arm64')] [string]$Arch = 'x64',
    [string]$Bin = 'target/release'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent

# Everything is bundled as its own signed installer. Stable 1.4 has no ARM64 setup, so the ARM64
# installer carries the x64 one too: it runs under emulation and talks to the island the same way.
$everything = @{
    Url = 'https://www.voidtools.com/Everything-1.4.1.1032.x64-Setup.exe'
    Sha256 = 'C42EFAD041D4C0BB4D4AC97AE7CBE89F153EC1FE078772392E749C7F5D5282D3'
}
$cache = Join-Path $root 'target\installer\cache'
New-Item -ItemType Directory -Force $cache | Out-Null
$setup = Join-Path $cache (Split-Path $everything.Url -Leaf)
if (-not (Test-Path $setup) -or (Get-FileHash $setup -Algorithm SHA256).Hash -ne $everything.Sha256) {
    Invoke-WebRequest $everything.Url -OutFile $setup
    $hash = (Get-FileHash $setup -Algorithm SHA256).Hash
    if ($hash -ne $everything.Sha256) { Remove-Item $setup; throw "Everything setup: SHA-256 $hash, expected $($everything.Sha256)" }
}

$iscc = @(
    (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
if (-not $iscc) { throw 'Inno Setup 6 (ISCC.exe) not found' }

$binDir = (Resolve-Path $(if ([IO.Path]::IsPathRooted($Bin)) { $Bin } else { Join-Path $root $Bin })).Path
& $iscc /Q "/DArch=$Arch" "/DBin=$binDir" "/DEverything=$setup" (Join-Path $PSScriptRoot 'polynya.iss')
if ($LASTEXITCODE -ne 0) { throw "ISCC: $LASTEXITCODE" }
Get-ChildItem (Join-Path $root 'target\installer') -Filter "*-$Arch-setup.exe" | Sort-Object LastWriteTime | Select-Object -Last 1 | ForEach-Object FullName
