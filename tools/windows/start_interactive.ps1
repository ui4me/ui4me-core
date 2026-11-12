[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$UserName,
    [Parameter(Mandatory=$true)][string]$ScriptPath,
    [int]$TimeoutSeconds = 180,
    [switch]$KeepTask,
    # Run without a console window, so observation jobs do not take the
    # foreground (and keyboard focus) from the application being observed.
    [switch]$Hidden
)
# Invoke from administrative SSH after the development user has logged in by RDP.
# The task uses that user's interactive token; an SSH logon is not a GUI session.
$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $ScriptPath).Path
if ($resolved.Contains('"')) { throw 'Invalid script path' }
$identity = if ($UserName.Contains('\') -or $UserName.Contains('@')) { $UserName } else { "$env:COMPUTERNAME\$UserName" }
$taskName = 'UI4ME-Interactive-' + [guid]::NewGuid().ToString('N')
$arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -STA -File "' + $resolved + '"'
$executable = 'powershell.exe'
if ($Hidden) {
    # A headless console host creates no window that could take the foreground.
    $arguments = '--headless powershell.exe ' + $arguments
    $executable = 'conhost.exe'
}
$action = New-ScheduledTaskAction -Execute $executable -Argument $arguments -WorkingDirectory (Split-Path -Parent $resolved)
$principal = New-ScheduledTaskPrincipal -UserId $identity -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Seconds $TimeoutSeconds) -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries
try {
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Settings $settings -Description 'UI4ME native desktop verification; dedicated interactive development session' | Out-Null
    $started = Get-Date
    Start-ScheduledTask -TaskName $taskName
    $deadline = $started.AddSeconds($TimeoutSeconds + 10)
    do {
        Start-Sleep -Seconds 1
        $task = Get-ScheduledTask -TaskName $taskName
        $info = Get-ScheduledTaskInfo -TaskName $taskName
        # The task is new and uniquely named, so its result leaves the "has not
        # run" (0x41303) and "running" (0x41301) codes only once it finished.
        # LastRunTime is not used: it was observed stale on Windows Server 2022.
        $finished = $info.LastTaskResult -notin @(267011, 267009)
        if ($finished -and $task.State -ne 'Running') {
            if ($info.LastTaskResult -ne 0) { throw "Interactive task failed: $($info.LastTaskResult). Inspect its own output files." }
            Write-Output 'Interactive task completed successfully.'
            exit 0
        }
    } while ((Get-Date) -lt $deadline)
    Stop-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue
    throw 'Interactive task timed out. Verify the dedicated user has an active RDP desktop and inspect its output.'
} finally {
    if (-not $KeepTask) { Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue }
}
