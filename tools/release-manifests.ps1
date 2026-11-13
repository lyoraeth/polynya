#requires -Version 7.2
# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

param(
    [Parameter(Mandatory)] [ValidatePattern('^v(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$')] [string]$Tag,
    [string]$Dist = 'target/release-download',
    [string]$Output = 'target/package-manifests'
)

$ErrorActionPreference = 'Stop'
$repo = 'lyoraeth/polynya'
$metadata = & gh release view $Tag --repo $repo --json tagName,isDraft,isPrerelease,assets
if ($LASTEXITCODE -ne 0) { throw "gh release view failed: $LASTEXITCODE" }
$release = $metadata | ConvertFrom-Json
if ($release.tagName -cne $Tag -or $release.isDraft -ne $false -or $release.isPrerelease -ne $false) {
    throw 'Only a published stable release with the requested tag can be distributed.'
}
$commit = & gh api "repos/$repo/commits/$Tag" --jq .sha
if ($LASTEXITCODE -ne 0 -or $commit -cnotmatch '^[0-9a-f]{40}$') { throw 'Could not resolve the release source commit.' }
$head = & git rev-parse HEAD
if ($LASTEXITCODE -ne 0 -or $head -cne $commit) { throw 'Check out the release source commit before generating its manifests.' }
$version = $Tag.Substring(1)
$names = foreach ($arch in 'x64', 'arm64') {
    foreach ($kind in 'setup.exe', 'portable.zip', 'modules.zip') { "Polynya-$version-$arch-$kind" }
}
foreach ($name in $names) {
    if (@($release.assets | Where-Object name -CEQ $name).Count -ne 1) { throw "Missing or ambiguous release asset: $name" }
}
if (Test-Path -LiteralPath $Dist) { throw 'Download directory already exists; use a fresh directory.' }
New-Item -ItemType Directory -Path $Dist -Force | Out-Null
$downloadArgs = @('release', 'download', $Tag, '--repo', $repo, '--dir', $Dist)
foreach ($name in $names) { $downloadArgs += @('--pattern', $name) }
& gh @downloadArgs | Out-Host
if ($LASTEXITCODE -ne 0) { throw "gh release download failed: $LASTEXITCODE" }
$generated = & "$PSScriptRoot/package-manifests.ps1" -Version $version -Dist $Dist -Output $Output
Write-Output $generated
