#Requires -RunAsAdministrator
$ErrorActionPreference = 'Stop'
. "$PSScriptRoot\task-lifecycle.ps1"
Remove-BatteryBridgeTask -Destination $PSScriptRoot
# UAC can run this script under a different administrator account. Clean only
# loaded user hives and only the Run value pointing at this installation.
$gui = Join-Path (Split-Path $PSScriptRoot -Parent) 'batterybridge-gui.exe'
foreach ($hive in Get-ChildItem 'Registry::HKEY_USERS') {
    if ($hive.PSChildName -notmatch '^S-1-(5-21|12-1)-[0-9-]+$') { continue }
    $run = Join-Path $hive.PSPath 'Software\Microsoft\Windows\CurrentVersion\Run'
    $entry = Get-ItemProperty -LiteralPath $run -Name BatteryBridge -ErrorAction SilentlyContinue
    if ($entry -and $entry.BatteryBridge -in @(('"' + $gui + '"'), ('"' + $gui + '" --start-hidden'))) {
        Remove-ItemProperty -LiteralPath $run -Name BatteryBridge
        $approval = Join-Path $hive.PSPath 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
        if (Get-ItemProperty -LiteralPath $approval -Name BatteryBridge -ErrorAction SilentlyContinue) {
            Remove-ItemProperty -LiteralPath $approval -Name BatteryBridge
        }
    }
}
$state = Join-Path $env:ProgramData 'BatteryBridge'
if (Test-Path -LiteralPath $state) {
    Protect-BatteryBridgeDirectory -LiteralPath $state
    Remove-Item -LiteralPath $state -Recurse -Force
}
