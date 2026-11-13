#requires -Version 7.2
# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root ('target/release-manifest-tests/' + [guid]::NewGuid().ToString('N'))
$names = foreach ($arch in 'x64', 'arm64') {
    foreach ($kind in 'setup.exe', 'portable.zip', 'modules.zip') { "Polynya-9.9.9-$arch-$kind" }
}

# Replace only the GitHub boundary: the real downloader/generator must use the fixture bytes.
function gh {
    $releaseTestState.Requests.Add($args)
    $global:LASTEXITCODE = 0
    if ($args[0] -ne 'release' -or $args[2] -cne 'v9.9.9') { throw 'Unexpected GitHub request.' }
    if ($args[1] -eq 'view') {
        $global:LASTEXITCODE = [int]$releaseTestState.ViewExit
        return ($releaseTestState.Metadata | ConvertTo-Json -Depth 5)
    }
    if ($args[1] -ne 'download') { throw 'Unexpected GitHub command.' }
    $global:LASTEXITCODE = [int]$releaseTestState.DownloadExit
    if ($releaseTestState.DownloadExit) { return }
    $destination = $args[[array]::IndexOf($args, '--dir') + 1]
    $patterns = for ($i = 0; $i -lt $args.Count; $i++) {
        if ($args[$i] -eq '--pattern') { $args[$i + 1] }
    }
    if (@(Compare-Object $releaseTestState.Names $patterns).Count) { throw 'Download did not request exactly the six release assets.' }
    foreach ($name in $patterns) { [IO.File]::WriteAllText((Join-Path $destination $name), "Downloaded fixture: $name") }
}

$cases = @(
    @{ Name = 'stable'; Tag = 'v9.9.9' },
    @{ Name = 'invalid-tag'; Tag = '9.9.9'; Error = '*'; ErrorId = 'ParameterArgumentValidationError,*' },
    @{ Name = 'prerelease-tag'; Tag = 'v9.9.9-beta.1'; Error = '*'; ErrorId = 'ParameterArgumentValidationError,*' },
    @{ Name = 'leading-zero'; Tag = 'v09.9.9'; Error = '*'; ErrorId = 'ParameterArgumentValidationError,*' },
    @{ Name = 'draft'; Tag = 'v9.9.9'; Draft = $true; Error = '*Only a published stable release*' },
    @{ Name = 'prerelease'; Tag = 'v9.9.9'; Prerelease = $true; Error = '*Only a published stable release*' },
    @{ Name = 'wrong-tag'; Tag = 'v9.9.9'; OtherTag = $true; Error = '*Only a published stable release*' },
    @{ Name = 'missing-asset'; Tag = 'v9.9.9'; Missing = $true; Error = '*Missing or ambiguous release asset*' },
    @{ Name = 'duplicate-asset'; Tag = 'v9.9.9'; Duplicate = $true; Error = '*Missing or ambiguous release asset*' },
    @{ Name = 'view-failure'; Tag = 'v9.9.9'; ViewExit = 1; Error = '*gh release view failed*' },
    @{ Name = 'download-failure'; Tag = 'v9.9.9'; DownloadExit = 1; Error = '*gh release download failed*' },
    @{ Name = 'existing-directory'; Tag = 'v9.9.9'; Existing = $true; Error = '*Download directory already exists*' }
)
foreach ($case in $cases) {
    $releaseTestState = @{
        Requests = [Collections.Generic.List[object]]::new()
        ViewExit = [int]$case.ViewExit
        DownloadExit = [int]$case.DownloadExit
        Names = $names
        Metadata = @{
            tagName = 'v9.9.9'
            isDraft = [bool]$case.Draft
            isPrerelease = [bool]$case.Prerelease
            assets = @($names | ForEach-Object { @{ name = $_ } })
        }
    }
    if ($case.OtherTag) { $releaseTestState.Metadata.tagName = 'v9.9.8' }
    if ($case.Missing) { $releaseTestState.Metadata.assets = @($releaseTestState.Metadata.assets | Select-Object -Skip 1) }
    if ($case.Duplicate) { $releaseTestState.Metadata.assets += $releaseTestState.Metadata.assets[0] }
    $dist = Join-Path $fixture "$($case.Name)/dist"
    $outputDir = Join-Path $fixture "$($case.Name)/output"
    if ($case.Existing) { New-Item -ItemType Directory -Path $dist -Force | Out-Null }
    try {
        $generated = & "$PSScriptRoot/release-manifests.ps1" -Tag $case.Tag -Dist $dist -Output $outputDir
        if ($case.Error) { throw "Unexpectedly accepted: $($case.Name)" }
        $manifest = Get-Content "$generated/scoop/bucket/polynya.json" -Raw | ConvertFrom-Json
        if ($manifest.version -ne '9.9.9' -or $releaseTestState.Requests.Count -ne 2) { throw 'Stable release was not prepared correctly.' }
        foreach ($key in '64bit', 'arm64') {
            for ($i = 0; $i -lt 2; $i++) {
                $name = $manifest.architecture.$key.url[$i].Split('/')[-1]
                if ($manifest.architecture.$key.hash[$i] -ne (Get-FileHash (Join-Path $dist $name)).Hash) {
                    throw 'Manifest hash does not match the downloaded release bytes.'
                }
            }
        }
    } catch {
        if (-not $case.Error -or $_.Exception.Message -notlike $case.Error) { throw }
        if ($case.ErrorId -and $_.FullyQualifiedErrorId -notlike $case.ErrorId) { throw }
        if (Test-Path -LiteralPath $outputDir) { throw "Rejected release generated manifests: $($case.Name)" }
        if ($case.Name -ne 'download-failure' -and $case.Name -ne 'existing-directory' -and (Test-Path -LiteralPath $dist)) {
            throw "Rejected release created a download directory: $($case.Name)"
        }
    }
}
Write-Output "Release distribution guards passed: $($cases.Count) cases, no network requests."
