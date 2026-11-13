#requires -Version 5.1
# Copyright (c) Danil Klimov (lyoraeth)
# SPDX-License-Identifier: GPL-3.0-or-later

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$fixture = Join-Path $root ('target/scoop-lifecycle-tests/' + [guid]::NewGuid().ToString('N'))
$appRoot = Join-Path $fixture 'path with spaces/apps/polynya'
$persist_dir = Join-Path $fixture 'path with spaces/persist/polynya'
$current = Join-Path $appRoot 'current'
$preInstall = Join-Path $root 'packaging/scoop/pre-install.ps1'
$postInstall = Join-Path $root 'packaging/scoop/post-install.ps1'
$preUninstall = Join-Path $root 'packaging/scoop/pre-uninstall.ps1'
$runKey = 'HKCU:/Software/Microsoft/Windows/CurrentVersion/Run'
$global = $false
$lifecycleState = @{
    NoJunction = $false; Registered = $null; FailRegistry = $false; FailLink = $false
    ExitCode = 0; Remaining = @(); Commands = [Collections.Generic.List[string]]::new()
}

# Mock only registry/process boundaries; filesystem junctions and hook execution are real.
function get_config([string]$Name) {
    if ($Name -ne 'NO_JUNCTION') { throw "Unexpected Scoop setting: $Name" }
    return $lifecycleState.NoJunction
}
function Get-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Name)
    if ($LiteralPath -ne $runKey -or $Name -ne 'Polynya') { throw 'Unexpected registry read' }
    [pscustomobject]@{ Polynya = $lifecycleState.Registered }
}
function Set-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Name, [string]$Value)
    if ($LiteralPath -ne $runKey -or $Name -ne 'Polynya') { throw 'Unexpected registry write' }
    if ($lifecycleState.FailRegistry) { Write-Error 'Fixture registry access denied' }
    $lifecycleState.Registered = $Value
}
function Remove-ItemProperty {
    [CmdletBinding()] param([string]$LiteralPath, [string]$Name)
    if ($LiteralPath -ne $runKey -or $Name -ne 'Polynya') { throw 'Unexpected registry removal' }
    $lifecycleState.Registered = $null
}
function New-Item {
    [CmdletBinding()] param([string[]]$Path, [string]$ItemType, [string]$Target, [switch]$Force)
    if ($Path.Count -eq 1 -and $Path[0] -eq $runKey) { return }
    if ($lifecycleState.FailLink -and $ItemType -eq 'Junction') { Write-Error 'Fixture junction access denied' }
    Microsoft.PowerShell.Management\New-Item @PSBoundParameters
}
function Start-Process {
    [CmdletBinding()] param([string]$FilePath, [string]$ArgumentList, [string]$WindowStyle, [switch]$PassThru, [switch]$Wait)
    if (-not $FilePath.StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase) -or $WindowStyle -ne 'Hidden') {
        throw 'Unexpected process launch'
    }
    $lifecycleState.Commands.Add($ArgumentList)
    [pscustomobject]@{ ExitCode = $lifecycleState.ExitCode }
}
function Get-Process { $lifecycleState.Remaining }
function Assert-Failure([scriptblock]$Action, [string]$Pattern) {
    $ErrorActionPreference = 'Continue'
    $failed = $false
    try { & $Action } catch {
        if ($_.Exception.Message -notlike $Pattern) { throw }
        $failed = $true
    }
    if (-not $failed) { throw "Expected a terminating error: $Pattern" }
}
function Set-Current([string]$Version) {
    $target = [IO.Path]::GetFullPath((Join-Path $appRoot $Version))
    if (-not $target.StartsWith("$appRoot\", [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture target is outside its app root' }
    if (Test-Path -LiteralPath $current) { [IO.Directory]::Delete($current) }
    Microsoft.PowerShell.Management\New-Item -ItemType Junction -Path $current -Target $target | Out-Null
}

foreach ($folder in 'data', 'logs', 'modules/custom') {
    New-Item -ItemType Directory -Path (Join-Path $persist_dir $folder) -Force | Out-Null
}
[IO.File]::WriteAllText("$persist_dir/data/config.ini", 'kept settings')
[IO.File]::WriteAllText("$persist_dir/modules/custom/main.py", 'kept custom module')
foreach ($version in '1.0.0', '2.0.0') {
    $dir = Join-Path $appRoot $version
    New-Item -ItemType Directory -Path "$dir/bundled/modules/agents" -Force | Out-Null
    [IO.File]::WriteAllText("$dir/portable", '')
    [IO.File]::WriteAllText("$dir/polynya.exe", "core $version")
    [IO.File]::WriteAllText("$dir/bundled/modules/agents/module.toml", "module $version")
    [IO.File]::WriteAllText("$dir/bundled/modules/agents/polynya-agents.exe", "binary $version")
    & $preInstall
    foreach ($entry in @(@('data', 'data'), @('logs', 'logs'), @('modules', 'modules'), @('bundled/data', 'data'), @('bundled/logs', 'logs'))) {
        $folder, $targetFolder = $entry
        # pre_install created empty directories; Scoop replaces these with persist junctions.
        [IO.Directory]::Delete((Join-Path $dir $folder))
        New-Item -ItemType Junction -Path (Join-Path $dir $folder) -Target (Join-Path $persist_dir $targetFolder) | Out-Null
    }
}
Set-Current 1.0.0
$dir = $current
& $postInstall
if ([IO.File]::ReadAllText("$current/modules/agents/polynya-agents.exe") -ne 'binary 1.0.0') { throw 'Wrong initial module version' }
if (-not (Test-Path "$appRoot/1.0.0/bundled/portable") -or -not (Test-Path "$persist_dir/portable")) { throw 'Physical module roots are not portable' }
if ([IO.File]::ReadAllText("$appRoot/1.0.0/bundled/data/config.ini") -ne 'kept settings') { throw 'Physical module root has different data' }
$lifecycleState.NoJunction = $true
Assert-Failure { & $preInstall } '*requires Scoop current junctions*'
$lifecycleState.NoJunction = $false
$global = $true
Assert-Failure { & $preInstall } '*per-user Scoop installations*'
$global = $false

$lifecycleState.Registered = '"' + (Join-Path $appRoot '1.0.0/polynya.exe') + '"'
$cmd = 'update'
$dir = Join-Path $appRoot '1.0.0'
& $preUninstall
if ($lifecycleState.Commands[-1] -ne 'quit' -or -not $lifecycleState.Registered) { throw 'Update ran destructive cleanup' }
if (-not (Test-Path "$persist_dir/autostart.enabled")) { throw 'Update did not remember autostart' }
$oldBinary = Join-Path $appRoot '1.0.0/bundled/modules/agents/polynya-agents.exe'
$lock = [IO.File]::Open($oldBinary, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::None)
try {
    Set-Current 2.0.0
    $dir = $current
    & $postInstall
} finally { $lock.Dispose() }
if ([IO.File]::ReadAllText("$current/modules/agents/polynya-agents.exe") -ne 'binary 2.0.0') { throw 'New module version was not selected' }
if ([IO.File]::ReadAllText($oldBinary) -ne 'binary 1.0.0') { throw 'Upgrade overwrote the old module binary' }
if ($lifecycleState.Registered -ne ('"' + (Join-Path $current 'polynya.exe') + '"')) { throw 'Autostart did not use the stable current path' }
if (Test-Path "$persist_dir/autostart.enabled") { throw 'Successful autostart restore kept its marker' }
# scoop reset only switches current and persist links; it does not run post_install.
Set-Current 1.0.0
if ([IO.File]::ReadAllText("$current/polynya.exe") -ne 'core 1.0.0' -or
    [IO.File]::ReadAllText("$current/modules/agents/polynya-agents.exe") -ne 'binary 1.0.0') { throw 'Rollback mixed core and module versions' }
Set-Current 2.0.0
& $postInstall
if ([IO.File]::ReadAllText("$persist_dir/data/config.ini") -ne 'kept settings' -or
    [IO.File]::ReadAllText("$current/modules/custom/main.py") -ne 'kept custom module') { throw 'User data changed during upgrade or rollback' }

New-Item -ItemType Directory -Path "$current/bundled/modules/picker" -Force | Out-Null
[IO.File]::WriteAllText("$current/bundled/modules/picker/module.toml", 'new module')
$lifecycleState.FailLink = $true
Assert-Failure { & $postInstall } '*Fixture junction access denied*'
$lifecycleState.FailLink = $false
& $postInstall
New-Item -ItemType File -Path "$persist_dir/autostart.enabled" -Force | Out-Null
$lifecycleState.FailRegistry = $true
Assert-Failure { & $postInstall } '*Fixture registry access denied*'
if (-not (Test-Path "$persist_dir/autostart.enabled")) { throw 'Failed registry write lost the retry marker' }
$lifecycleState.FailRegistry = $false
& $postInstall
$lifecycleState.ExitCode = 1
Assert-Failure { & $preUninstall } '*cleanup failed*'
$lifecycleState.ExitCode = 0
$lifecycleState.Remaining = @([pscustomobject]@{ Path = Join-Path $current 'polynya.exe' })
Assert-Failure { & $preUninstall } '*processes are still running*'
$lifecycleState.Remaining = @()
$cmd = 'unexpected'
Assert-Failure { & $preUninstall } '*Unsupported Scoop lifecycle command*'
$cmd = 'uninstall'
& $preUninstall
if ($lifecycleState.Commands[-1] -ne 'uninstall' -or $lifecycleState.Registered -or (Test-Path "$persist_dir/autostart.enabled")) {
    throw 'Uninstall did not clean its autostart and retry marker'
}
if ([IO.File]::ReadAllText("$persist_dir/data/config.ini") -ne 'kept settings') { throw 'Uninstall purged user data' }
$lifecycleState.Registered = '"C:\Other\polynya.exe"'
New-Item -ItemType File -Path "$persist_dir/autostart.enabled" -Force | Out-Null
& $postInstall
if ($lifecycleState.Registered -ne '"C:\Other\polynya.exe"') { throw 'Another installation lost its autostart' }
New-Item -ItemType Directory -Path "$current/bundled/modules/conflict", "$current/modules/conflict" -Force | Out-Null
[IO.File]::WriteAllText("$current/bundled/modules/conflict/module.toml", 'bundled')
[IO.File]::WriteAllText("$current/modules/conflict/user-data.txt", 'keep me')
Assert-Failure { & $postInstall } '*conflicts with the Scoop module link*'
if ([IO.File]::ReadAllText("$current/modules/conflict/user-data.txt") -ne 'keep me') { throw 'Module conflict destroyed user files' }
Write-Output "Scoop lifecycle checks passed on PowerShell $($PSVersionTable.PSVersion): upgrade, rollback, locked old binary, failure propagation and autostart. No real registry or process changes."
