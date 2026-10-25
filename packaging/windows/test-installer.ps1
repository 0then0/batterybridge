# Run only in the disposable GitHub Actions Windows VM after building NSIS.
$ErrorActionPreference = 'Stop'
function Invoke-Installer {
    param([string]$FilePath)
    $process = Start-Process -FilePath $FilePath -ArgumentList '/S' -PassThru
    if (-not $process.WaitForExit(180000)) {
        & taskkill.exe /PID $process.Id /T /F | Out-Host
        throw "Installer timed out after 180 seconds: $FilePath"
    }
    if ($process.ExitCode -ne 0) { throw "Installer failed ($($process.ExitCode)): $FilePath" }
}
$setup = Get-ChildItem target/release/bundle/nsis/*-setup.exe | Select-Object -First 1
if (-not $setup) { throw 'NSIS installer missing' }
try { Invoke-Installer -FilePath $setup.FullName } catch {
    # Diagnose on this disposable VM without turning a failed installer into a pass.
    $failure = $_
    foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32, [Microsoft.Win32.RegistryView]::Registry64)) {
        $key = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::LocalMachine, $view).OpenSubKey('Software\BatteryBridge\Installer')
        if ($key) { Write-Output ($key.GetValue('Error')); $key.Dispose() }
    }
    $installed = Join-Path $env:ProgramFiles 'BatteryBridge'
    if (Test-Path "$installed/service/install-packaged.ps1") {
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$installed/service/install-packaged.ps1"
    } else {
        & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ./packaging/windows/prepare-install.ps1 -Destination $installed
    }
    throw $failure
}
$installed = Join-Path $env:ProgramFiles 'BatteryBridge'
$client = Join-Path $installed 'service/batterybridge.exe'
& $client config --json
if ($LASTEXITCODE -ne 0) { throw 'Installed service did not answer IPC' }
$task = Get-ScheduledTask -TaskName 'BatteryBridge'
if ($task.Principal.UserId -notin @('SYSTEM','S-1-5-18')) { throw 'Task must run as SYSTEM' }
& "$PSScriptRoot/test-gui.ps1" -Executable (Join-Path $installed 'batterybridge-gui.exe')
Get-Process batterybridge-gui -ErrorAction SilentlyContinue | Stop-Process -Force
$run = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
New-Item -Path $run -Force | Out-Null
New-ItemProperty -Path $run -Name BatteryBridge -Value ('"' + (Join-Path $installed 'batterybridge-gui.exe') + '" --start-hidden') -PropertyType String -Force | Out-Null
$approval = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
New-Item -Path $approval -Force | Out-Null
New-ItemProperty -Path $approval -Name BatteryBridge -Value ([byte[]](3,0,0,0,0,0,0,0,0,0,0,0)) -PropertyType Binary -Force | Out-Null
Invoke-Installer -FilePath (Join-Path $installed 'uninstall.exe')
# NSIS copies its uninstaller to a temporary child process before removing itself.
# Wait for that child's observable result as well as the original process exit.
$deadline = [DateTime]::UtcNow.AddSeconds(30)
while ((Test-Path (Join-Path $installed 'service/batterybridge-service.exe')) -and [DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Milliseconds 200
}
if (Get-ScheduledTask -TaskName 'BatteryBridge' -ErrorAction SilentlyContinue) { throw 'Background task survived removal' }
if (Test-Path (Join-Path $installed 'service/batterybridge-service.exe')) { throw 'Daemon executable survived removal' }
if (Test-Path (Join-Path $env:ProgramData 'BatteryBridge')) { throw 'Saved settings survived removal' }
if (Get-ItemProperty -Path $run -Name BatteryBridge -ErrorAction SilentlyContinue) { throw 'GUI autostart survived removal' }
if (Get-ItemProperty -Path $approval -Name BatteryBridge -ErrorAction SilentlyContinue) { throw 'GUI startup approval record survived removal' }
Write-Output 'Windows NSIS install / IPC / uninstall integration: passed'
