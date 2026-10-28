# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

# Installs the Rust version from rust-toolchain.toml for the runner's own architecture, MSVC
# flavor. The windows-11-arm image does not promise rustup, so it is fetched when missing.

$ErrorActionPreference = 'Stop'
$triple = switch ($env:RUNNER_ARCH) {
    'X64' { 'x86_64-pc-windows-msvc' }
    'ARM64' { 'aarch64-pc-windows-msvc' }
    default { throw "unsupported runner architecture: $env:RUNNER_ARCH" }
}

if (-not (Get-Command rustup -ErrorAction SilentlyContinue)) {
    $init = Join-Path $env:RUNNER_TEMP 'rustup-init.exe'
    Invoke-WebRequest "https://static.rust-lang.org/rustup/dist/$triple/rustup-init.exe" -OutFile $init
    & $init -y --no-modify-path --profile minimal --default-toolchain none
    if ($LASTEXITCODE -ne 0) { throw "rustup-init: $LASTEXITCODE" }
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    $env:PATH = "$cargoBin;$env:PATH"
    Add-Content $env:GITHUB_PATH $cargoBin
}

# rust-toolchain.toml names the channel without a target: it resolves against the default host.
& rustup set default-host $triple
if ($LASTEXITCODE -ne 0) { throw "rustup set default-host: $LASTEXITCODE" }
$toml = Get-Content (Join-Path $PSScriptRoot '..\rust-toolchain.toml') -Raw
if ($toml -notmatch 'channel\s*=\s*"([^"]+)"') { throw 'rust-toolchain.toml has no channel' }
$install = @('toolchain', 'install', $Matches[1], '--profile', 'minimal')
if ($toml -match 'components\s*=\s*\[([^\]]*)\]') {
    foreach ($c in [regex]::Matches($Matches[1], '"([^"]+)"')) { $install += @('--component', $c.Groups[1].Value) }
}
& rustup @install
if ($LASTEXITCODE -ne 0) { throw "rustup toolchain install: $LASTEXITCODE" }
& rustc -vV
