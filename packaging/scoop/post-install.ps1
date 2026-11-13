$ErrorActionPreference = 'Stop'
$bundled = Join-Path $dir 'bundled/modules'
if (-not (Test-Path -LiteralPath $bundled)) { throw 'Bundled modules are missing.' }
$appRoot = Split-Path -Parent $dir
$current = Join-Path $appRoot 'current'
if (-not (Test-Path -LiteralPath "$current/portable")) { throw 'Scoop current junction is missing.' }
foreach ($folder in 'data', 'logs') {
    if (-not (Test-Path -LiteralPath "$persist_dir/$folder" -PathType Container)) { throw "Persisted directory is missing: $folder" }
}
# Module images may be resolved through junctions by Windows; both physical roots need portable data.
New-Item -ItemType File -Path "$persist_dir/portable" -Force | Out-Null
New-Item -ItemType File -Path "$dir/bundled/portable" -Force | Out-Null
$dataLinks = foreach ($folder in 'data', 'logs') {
    @{ Path = Join-Path "$dir/bundled" $folder; Target = [IO.Path]::GetFullPath("$persist_dir/$folder") }
}
# Only the links are persisted: reset switches core and bundled binaries together, without hooks.
$modules = @(Get-ChildItem -LiteralPath $bundled -Directory)
if (-not $modules.Count) { throw 'Bundled modules are missing.' }
$moduleLinks = foreach ($module in $modules) {
    $destination = Join-Path "$dir/modules" $module.Name
    $target = [IO.Path]::GetFullPath((Join-Path "$current/bundled/modules" $module.Name))
    if (-not (Test-Path -LiteralPath "$($module.FullName)/module.toml")) { throw "Bundled module manifest is missing: $($module.Name)" }
    @{ Path = $destination; Target = $target }
}
$missingLinks = foreach ($link in @($dataLinks) + @($moduleLinks)) {
    $existing = Get-Item -LiteralPath $link.Path -Force -ErrorAction SilentlyContinue
    $targets = @($existing.Target)
    if ($existing -and ($existing.LinkType -ne 'Junction' -or $targets.Count -ne 1 -or $targets[0] -ine $link.Target)) {
        throw "Path conflicts with the Scoop module link: $($link.Path)"
    }
    if (-not $existing) { $link }
}
foreach ($link in $missingLinks) {
    New-Item -ItemType Junction -Path $link.Path -Target $link.Target | Out-Null
}
$marker = Join-Path $persist_dir 'autostart.enabled'
if (Test-Path -LiteralPath $marker) {
    $runKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
    $registered = (Get-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction SilentlyContinue).Polynya
    if (-not $registered -or $registered.Trim('"').StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase)) {
        New-Item -Path $runKey -Force | Out-Null
        Set-ItemProperty -LiteralPath $runKey -Name Polynya -Value "`"$dir\polynya.exe`""
    }
    Remove-Item -LiteralPath $marker -Force
}
