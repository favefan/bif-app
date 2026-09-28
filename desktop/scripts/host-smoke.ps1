# SPDX-License-Identifier: Apache-2.0
# Runs the extracted release artifact in an otherwise empty CI user profile.
param([string]$Archive = "$PSScriptRoot/../dist/bif-app-windows-x64-portable.zip")
$ErrorActionPreference = 'Stop'
if ($env:CI -ne 'true') { throw 'This check requires a disposable CI Windows profile; it must not alter an existing desktop installation.' }
$statePath = Join-Path $env:LOCALAPPDATA 'bif-app/desktop/state.json'
if (Test-Path (Join-Path $env:LOCALAPPDATA 'bif-app')) { throw 'Host smoke test requires a clean bif-app profile' }
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ('bif-app-artifact-' + [guid]::NewGuid())
Expand-Archive -LiteralPath $Archive -DestinationPath $testRoot
$exe = Join-Path $testRoot 'bif-app.exe'
$bytes = [IO.File]::ReadAllBytes($exe)
$pe = [BitConverter]::ToInt32($bytes, 0x3c)
if ([BitConverter]::ToUInt16($bytes, $pe + 24 + 68) -ne 2) { throw 'Desktop EXE must use the Windows GUI subsystem (no console)' }
function Ready($process) {
    $deadline = (Get-Date).AddSeconds(120)
    do {
        if ($process.HasExited) { throw "Desktop exited early: $($process.ExitCode)" }
        if (Test-Path $statePath) {
            $port = (Get-Content $statePath -Raw | ConvertFrom-Json).port
            try {
                $health = Invoke-WebRequest "http://127.0.0.1:$port/health" -TimeoutSec 2
                if ($health.StatusCode -eq 200) {
                    $children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId = $($process.Id) AND Name = 'bifrost-http.exe'")
                    if ($children.Count -eq 1) {
                        $listeners = @(Get-NetTCPConnection -State Listen -OwningProcess $children[0].ProcessId -ErrorAction SilentlyContinue)
                        if ($listeners.Count -eq 1 -and $listeners[0].LocalAddress -eq '127.0.0.1' -and $listeners[0].LocalPort -eq $port) { return @{ Port=$port; Child=[int]$children[0].ProcessId } }
                    }
                }
            } catch { }
        }
        Start-Sleep -Milliseconds 250
    } while ((Get-Date) -lt $deadline)
    throw 'Extracted host did not start its own healthy loopback gateway'
}
function CrashAndReap($process, $child) {
    if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force }
    $deadline = (Get-Date).AddSeconds(10)
    while ((Get-Process -Id $child -ErrorAction SilentlyContinue) -and (Get-Date) -lt $deadline) { Start-Sleep -Milliseconds 100 }
    if (Get-Process -Id $child -ErrorAction SilentlyContinue) { throw 'Gateway orphaned after owner termination' }
}
$hostProcess = $null
try {
    $hostProcess = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
    $first = Ready $hostProcess
    $root = Invoke-WebRequest "http://127.0.0.1:$($first.Port)/" -TimeoutSec 5
    if ($root.StatusCode -ne 200 -or $root.Content -notmatch '/assets/') { throw 'Missing original UI' }
    $second = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
    if (-not $second.WaitForExit(10000) -or $second.ExitCode -ne 0) { throw 'Second instance did not exit successfully' }
    $check = Ready $hostProcess
    if ($check.Child -ne $first.Child) { throw 'Second instance replaced the gateway' }
    CrashAndReap $hostProcess $first.Child
    $hostProcess = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
    $restarted = Ready $hostProcess
    if ($restarted.Port -ne $first.Port) { throw 'Persisted port was not reused' }
    CrashAndReap $hostProcess $restarted.Child
    Write-Output 'PASS: extracted release, GUI subsystem, healthy original UI, loopback-only socket, single instance, crash reaping, restart with existing data/key/port'
} finally {
    if ($hostProcess -and -not $hostProcess.HasExited) { Stop-Process -Id $hostProcess.Id -Force }
}
