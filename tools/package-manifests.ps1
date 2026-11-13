# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

param(
    [Parameter(Mandatory)] [ValidatePattern('^\d+\.\d+\.\d+$')] [string]$Version,
    [string]$Dist = 'target/dist',
    [string]$Output = 'target/package-manifests'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$distDir = (Resolve-Path -LiteralPath $Dist).Path
$outputDir = [IO.Path]::GetFullPath($Output)
$baseUrl = "https://github.com/lyoraeth/polynya/releases/download/v$Version"
$hashes = @{}
foreach ($arch in 'x64', 'arm64') {
    foreach ($kind in 'setup.exe', 'portable.zip', 'modules.zip') {
        $name = "Polynya-$Version-$arch-$kind"
        $path = Join-Path $distDir $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing release artifact: $path" }
        $hashes[$name] = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
    }
}
if (Test-Path -LiteralPath (Join-Path $outputDir $Version)) { throw 'Manifest version already exists; use a new output directory.' }
$versionDir = Join-Path $outputDir $Version
$winget = Join-Path $versionDir "winget/manifests/l/lyoraeth/Polynya/$Version"
$scoop = Join-Path $versionDir 'scoop/bucket'
New-Item -ItemType Directory -Path $winget, $scoop -Force | Out-Null
$encoding = [Text.UTF8Encoding]::new($false)
$installer = @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.installer.1.10.0.schema.json
PackageIdentifier: lyoraeth.Polynya
PackageVersion: $Version
MinimumOSVersion: 10.0.22000.0
InstallerType: inno
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
InstallerSwitches:
  Custom: /COMPONENTS="core,modules\agents,modules\picker" /MERGETASKS="!autostart"
UpgradeBehavior: install
Dependencies:
  PackageDependencies:
  - PackageIdentifier: voidtools.Everything
ProductCode: '{793DD86B-4DA6-450E-B1F6-D7D1CF02CBB4}_is1'
AppsAndFeaturesEntries:
- DisplayName: Polynya
  Publisher: lyoraeth
  ProductCode: '{793DD86B-4DA6-450E-B1F6-D7D1CF02CBB4}_is1'
Installers:
"@
foreach ($arch in 'x64', 'arm64') {
    $name = "Polynya-$Version-$arch-setup.exe"
    $installer += "`n- Architecture: $arch`n  InstallerUrl: $baseUrl/$name`n  InstallerSha256: $($hashes[$name])"
}
$installer += "`nManifestType: installer`nManifestVersion: 1.10.0`n"
[IO.File]::WriteAllText((Join-Path $winget 'lyoraeth.Polynya.installer.yaml'), $installer, $encoding)
[IO.File]::WriteAllText((Join-Path $winget 'lyoraeth.Polynya.locale.en-US.yaml'), @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.defaultLocale.1.10.0.schema.json
PackageIdentifier: lyoraeth.Polynya
PackageVersion: $Version
PackageLocale: en-US
Publisher: lyoraeth
PublisherUrl: https://github.com/lyoraeth
PublisherSupportUrl: https://github.com/lyoraeth/polynya/issues
PackageName: Polynya
PackageUrl: https://github.com/lyoraeth/polynya
License: GPL-3.0-or-later
LicenseUrl: https://github.com/lyoraeth/polynya/blob/main/LICENSE
ShortDescription: A taskbar island and launcher for Windows 11.
Moniker: polynya
ReleaseNotesUrl: https://github.com/lyoraeth/polynya/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: 1.10.0
"@, $encoding)
[IO.File]::WriteAllText((Join-Path $winget 'lyoraeth.Polynya.yaml'), @"
# yaml-language-server: `$schema=https://aka.ms/winget-manifest.version.1.10.0.schema.json
PackageIdentifier: lyoraeth.Polynya
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.10.0
"@, $encoding)
$manifest = [ordered]@{
    version = $Version
    description = 'A taskbar island and launcher for Windows 11.'
    homepage = 'https://github.com/lyoraeth/polynya'
    license = 'GPL-3.0-or-later'
    architecture = [ordered]@{}
    extract_to = @('', 'bundled')
    pre_install = [IO.File]::ReadAllLines((Join-Path $root 'packaging/scoop/pre-install.ps1'))
    post_install = [IO.File]::ReadAllLines((Join-Path $root 'packaging/scoop/post-install.ps1'))
    pre_uninstall = [IO.File]::ReadAllLines((Join-Path $root 'packaging/scoop/pre-uninstall.ps1'))
    bin = 'polynya.exe'
    shortcuts = ,@('polynya.exe', 'Polynya')
    persist = @('data', 'logs', 'modules')
    depends = 'extras/everything'
    checkver = @{ github = 'https://github.com/lyoraeth/polynya' }
    autoupdate = @{ architecture = [ordered]@{} }
    notes = @('Everything is installed as a dependency; start and configure it for file search.', 'Launch Polynya after updating to restore enabled agent integrations.', 'Use one Polynya installation at a time; notify setup is optional and interactive.')
}
foreach ($entry in @(@('64bit', 'x64'), @('arm64', 'arm64'))) {
    $key, $arch = $entry
    $names = @("Polynya-$Version-$arch-portable.zip", "Polynya-$Version-$arch-modules.zip")
    $manifest.architecture[$key] = [ordered]@{ url = @($names | ForEach-Object { "$baseUrl/$_" }); hash = @($names | ForEach-Object { $hashes[$_] }) }
    $manifest.autoupdate.architecture[$key] = @{ url = @("https://github.com/lyoraeth/polynya/releases/download/v`$version/Polynya-`$version-$arch-portable.zip", "https://github.com/lyoraeth/polynya/releases/download/v`$version/Polynya-`$version-$arch-modules.zip") }
}
[IO.File]::WriteAllText((Join-Path $scoop 'polynya.json'), ($manifest | ConvertTo-Json -Depth 12) + "`n", $encoding)
Write-Output $versionDir
