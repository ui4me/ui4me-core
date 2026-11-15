Set-Location 'F:\agent-workspaces\ui4me-core\audit-ac27d3dd'
Remove-Item pyw5-*.txt -ErrorAction SilentlyContinue
$pyw = (Resolve-Path .\venv\Scripts\pythonw.exe).Path
$before = @(Get-Process conhost -ErrorAction SilentlyContinue | ForEach-Object Id)
$p = Start-Process -FilePath $pyw -ArgumentList "idle.py" -PassThru
$lines = @()
foreach ($t in 1..10) {
  Start-Sleep 1
  $procs = Get-CimInstance Win32_Process | Where-Object { $_.Name -match 'python|ui4me|conhost' -and $_.SessionId -eq (Get-Process -Id $PID).SessionId } | ForEach-Object { "$($_.Name)#$($_.ProcessId)<-$($_.ParentProcessId)" }
  $lines += "t=$t launcherAlive=$(-not $p.HasExited) exit=$(if ($p.HasExited) { $p.ExitCode }) procs=[$($procs -join ' ')]"
}
$newcon = @(Get-Process conhost -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id } | ForEach-Object { "$($_.Id) handle=$($_.MainWindowHandle)" })
$lines += "new conhost: $($newcon -join '; ')"
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public class W { public delegate bool P(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumWindows(P p, IntPtr l);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
[DllImport("user32.dll")] public static extern int GetWindowThreadProcessId(IntPtr h, out int pid); }
"@
$vis = New-Object System.Collections.ArrayList
[W]::EnumWindows({ param($h,$l) if ([W]::IsWindowVisible($h)) { $sb = New-Object Text.StringBuilder 256; [void][W]::GetClassName($h,$sb,256); $wpid = 0; [void][W]::GetWindowThreadProcessId($h,[ref]$wpid); $n = (Get-Process -Id $wpid -EA SilentlyContinue).Name; [void]$vis.Add("$($sb.ToString())@$n#$wpid") }; $true }, [IntPtr]::Zero) | Out-Null
$lines += "visible windows: $($vis -join ' | ')"
$lines | Out-File pyw5-result.txt
Get-Process ui4me-host, pythonw, python -ErrorAction SilentlyContinue | Where-Object { $_.SessionId -eq (Get-Process -Id $PID).SessionId } | Stop-Process -Force
