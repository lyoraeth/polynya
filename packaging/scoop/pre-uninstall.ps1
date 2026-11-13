$ErrorActionPreference = 'Stop'
if ($cmd -notin @('update', 'uninstall')) { throw "Unsupported Scoop lifecycle command: $cmd" }
$appRoot = Split-Path -Parent $dir
$runKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
$registered = (Get-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction SilentlyContinue).Polynya
$ownsAutostart = $registered -and $registered.Trim('"').StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase)
if ($cmd -eq 'update' -and $ownsAutostart) {
    New-Item -ItemType Directory -Path $persist_dir -Force | Out-Null
    New-Item -ItemType File -Path "$persist_dir/autostart.enabled" -Force | Out-Null
}
$command = if ($cmd -eq 'update') { 'quit' } else { 'uninstall' }
$process = Start-Process -FilePath "$dir/polynya.exe" -ArgumentList $command -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode -ne 0) { throw "Polynya cleanup failed: $($process.ExitCode)" }
if (Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase) }) {
    throw 'Polynya processes are still running; close them and retry.'
}
if ($cmd -eq 'update') { return }
# Scoop invokes the versioned exe, while autostart may point through the current junction.
if ($ownsAutostart -and (Get-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction SilentlyContinue).Polynya -eq $registered) {
    Remove-ItemProperty -LiteralPath $runKey -Name Polynya -ErrorAction Stop
}
$marker = Join-Path $persist_dir 'autostart.enabled'
if (Test-Path -LiteralPath $marker) { Remove-Item -LiteralPath $marker -Force }
