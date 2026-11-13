$bundled = Join-Path $dir 'bundled/modules'
if (-not (Test-Path -LiteralPath $bundled)) { throw 'Bundled modules are missing.' }
# Replace bundled files in the persisted tree without deleting third-party modules or their data.
foreach ($file in Get-ChildItem -LiteralPath $bundled -File -Recurse) {
    $relative = $file.FullName.Substring($bundled.Length).TrimStart([char[]]'\/')
    $destination = Join-Path "$dir/modules" $relative
    New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
    Move-Item -LiteralPath $file.FullName -Destination $destination -Force
}
$marker = Join-Path $persist_dir 'autostart.enabled'
if (Test-Path -LiteralPath $marker) {
    $runKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
    $registered = (Get-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction SilentlyContinue).Polynya
    $appRoot = Split-Path -Parent $dir
    if (-not $registered -or $registered.Trim('"').StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase)) {
        New-Item -Path $runKey -Force | Out-Null
        Set-ItemProperty -LiteralPath $runKey -Name Polynya -Value "`"$dir\polynya.exe`""
    }
    Remove-Item -LiteralPath $marker -Force
}
