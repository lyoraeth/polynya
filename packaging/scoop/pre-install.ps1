$ErrorActionPreference = 'Stop'
if ($global) { throw 'Polynya supports per-user Scoop installations only.' }
if ([Environment]::OSVersion.Version -lt [version]'10.0.22000') {
    throw 'Polynya requires Windows 11 (build 22000 or newer). Windows 10 is not supported.'
}
if (-not (Test-Path -LiteralPath "$dir/portable")) { throw 'Portable marker is missing.' }
if (get_config NO_JUNCTION) { throw 'Polynya requires Scoop current junctions; disable no_junction and retry.' }
foreach ($folder in 'data', 'logs', 'modules', 'bundled/data', 'bundled/logs') {
    New-Item -ItemType Directory -Path (Join-Path $dir $folder) -Force | Out-Null
}
