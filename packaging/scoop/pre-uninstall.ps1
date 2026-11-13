$appRoot = Split-Path -Parent $dir
$runKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
$registered = (Get-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction SilentlyContinue).Polynya
$ownsAutostart = $registered -and $registered.Trim('"').StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase)
if ($ownsAutostart) {
    New-Item -ItemType Directory -Path $persist_dir -Force | Out-Null
    New-Item -ItemType File -Path "$persist_dir/autostart.enabled" -Force | Out-Null
}
# The same cleanup runs on update: settings stay in persist, integrations return on next launch.
$process = Start-Process -FilePath "$dir/polynya.exe" -ArgumentList uninstall -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode -ne 0) { throw "Polynya cleanup failed: $($process.ExitCode)" }
# Scoop invokes the versioned exe, while autostart may point through the current junction.
if ($ownsAutostart -and (Get-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction SilentlyContinue).Polynya -eq $registered) {
    Remove-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction Stop
}
