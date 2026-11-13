#requires -Version 7.2
# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

param([string]$ScoopSchema, [switch]$Winget, [string]$ScoopScript, [switch]$ResolveWingetDependency)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root ('target/package-manifest-tests/' + [guid]::NewGuid().ToString('N'))
$dist = Join-Path $fixture 'dist'
New-Item -ItemType Directory -Path $dist -Force | Out-Null
foreach ($arch in 'x64', 'arm64') {
    foreach ($kind in 'setup.exe', 'portable.zip', 'modules.zip') {
        [IO.File]::WriteAllText((Join-Path $dist "Polynya-9.9.9-$arch-$kind"), "Fixture only: $arch $kind")
    }
}
$outputDir = Join-Path $fixture 'output'
$generated = & "$PSScriptRoot/package-manifests.ps1" -Version 9.9.9 -Dist $dist -Output $outputDir
$manifestPath = Join-Path $generated 'scoop/bucket/polynya.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
if (@($manifest.depends).Count -ne 1 -or $manifest.depends -ne 'extras/everything') { throw 'Everything must be a required Scoop dependency' }
if ($manifest.PSObject.Properties.Name -contains 'suggest') { throw 'Required dependency is still optional' }
foreach ($folder in 'data', 'logs') {
    if (-not ($manifest.persist | Where-Object { $_ -is [array] -and $_[0] -eq "bundled/$folder" -and $_[1] -eq $folder })) {
        throw "Nested module data link must be managed by Scoop persist: $folder"
    }
}
$installerPath = Join-Path $generated 'winget/manifests/l/lyoraeth/Polynya/9.9.9/lyoraeth.Polynya.installer.yaml'
$installerText = [IO.File]::ReadAllText($installerPath)
if ($installerText -notmatch '(?m)^Dependencies:\r?\n  PackageDependencies:\r?\n  - PackageIdentifier: voidtools\.Everything\r?$') {
    throw 'Everything must be a required WinGet package dependency'
}
if ($manifest.shortcuts.Count -ne 1 -or $manifest.shortcuts[0] -is [string] -or $manifest.shortcuts[0][0] -ne 'polynya.exe') {
    throw 'Invalid shortcut structure'
}
foreach ($arch in @(@('64bit', 'x64'), @('arm64', 'arm64'))) {
    $key, $suffix = $arch
    $entry = $manifest.architecture.$key
    if ($entry.url.Count -ne 2 -or $entry.hash.Count -ne 2 -or $manifest.extract_to[1] -ne 'bundled') { throw 'Invalid archive layout' }
    for ($i = 0; $i -lt 2; $i++) {
        $filename = $entry.url[$i].Split('/')[-1]
        if ($filename -notlike "Polynya-9.9.9-$suffix-*") { throw 'Wrong architecture URL' }
        if ($entry.hash[$i] -ne (Get-FileHash -LiteralPath (Join-Path $dist $filename)).Hash) { throw 'Hash mismatch' }
    }
}
foreach ($scriptPath in Get-ChildItem -LiteralPath (Join-Path $root 'packaging/scoop') -Filter '*.ps1') {
    $tokens = $null
    $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile($scriptPath.FullName, [ref]$tokens, [ref]$errors)
    if ($errors) { throw ($errors | Out-String) }
}
try {
    $null = & "$PSScriptRoot/package-manifests.ps1" -Version 9.9.9 -Dist $dist -Output $outputDir
    throw 'Existing output was overwritten'
} catch {
    if ($_.Exception.Message -notlike '*Manifest version already exists*') { throw }
}
$missing = Join-Path $fixture 'missing'
New-Item -ItemType Directory -Path $missing | Out-Null
try {
    $null = & "$PSScriptRoot/package-manifests.ps1" -Version 9.9.9 -Dist $missing -Output (Join-Path $fixture 'should-not-exist')
    throw 'Missing artifacts were accepted'
} catch {
    if ($_.Exception.Message -notlike 'Missing release artifact:*') { throw }
}

# Substitute the OS query in the generated hook to exercise version boundaries on this host.
$preInstall = $manifest.pre_install -join "`n"
$versionQuery = '[Environment]::OSVersion.Version'
if (($preInstall.Split($versionQuery, [StringSplitOptions]::None).Count - 1) -ne 1) { throw 'Missing or ambiguous Windows version guard' }
$global = $false
function get_config([string]$Name) {
    if ($Name -ne 'NO_JUNCTION') { throw "Unexpected Scoop setting: $Name" }
    return $false
}
foreach ($os in @('6.3.9600', '10.0.19045', '10.0.21999', '10.0.22000', '10.0.26100')) {
    $dir = Join-Path $fixture "os-$os"
    New-Item -ItemType Directory -Path $dir | Out-Null
    [IO.File]::WriteAllText("$dir/portable", '')
    $allowed = [version]$os -ge [version]'10.0.22000'
    $rejected = $false
    try {
        & ([scriptblock]::Create($preInstall.Replace($versionQuery, "([version]'$os')")))
    } catch {
        if (-not $allowed -and $_.Exception.Message -like 'Polynya requires Windows 11*') { $rejected = $true }
        else { throw }
    }
    if ($rejected -eq $allowed) { throw "Incorrect Windows version decision: $os" }
    foreach ($folder in 'data', 'logs', 'modules') {
        if ((Test-Path -LiteralPath "$dir/$folder") -ne $allowed) { throw "Unexpected pre-install side effect: $os/$folder" }
    }
}
Write-Output 'Scoop Windows version guard passed: 5 boundary cases.'

if ($ScoopSchema) {
    if (-not (Test-Json -Json (Get-Content -LiteralPath $manifestPath -Raw) -SchemaFile $ScoopSchema)) { throw 'Scoop schema validation failed' }
}
if ($Winget) {
    & winget validate (Join-Path $generated 'winget/manifests/l/lyoraeth/Polynya/9.9.9')
    if ($LASTEXITCODE -ne 0) { throw 'WinGet validation failed' }
}
if ($ScoopScript) {
    $query = Join-Path $fixture 'resolve-scoop.ps1'
    [IO.File]::WriteAllText($query, @'
param([string]$ScoopScript, [string]$ManifestPath, [string]$Architecture)
$ErrorActionPreference = 'Stop'
& $ScoopScript depends $ManifestPath --arch $Architecture | Select-Object Source, Name | ConvertTo-Json -Compress
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
'@)
    foreach ($arch in '64bit', 'arm64') {
        $resolved = & (Join-Path $PSHOME 'pwsh.exe') -NoProfile -ExecutionPolicy Bypass -File $query -ScoopScript $ScoopScript -ManifestPath $manifestPath -Architecture $arch
        if ($LASTEXITCODE -ne 0) { throw "Scoop dependency resolution failed: $arch`n$resolved" }
        $packages = ($resolved | Out-String) | ConvertFrom-Json
        if (-not ($packages | Where-Object { $_.Name -eq 'everything' -and $_.Source -eq 'extras' })) { throw "Missing Scoop dependency: $arch" }
        Write-Output "Scoop dependency resolution passed: $arch"
    }
}
if ($ResolveWingetDependency) {
    & winget show --id voidtools.Everything --exact --source winget --disable-interactivity
    if ($LASTEXITCODE -ne 0) { throw 'WinGet could not resolve the Everything dependency' }
}
Write-Output "Manifest generation checks passed: $fixture"
