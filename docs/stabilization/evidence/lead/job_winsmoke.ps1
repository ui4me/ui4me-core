$ErrorActionPreference = 'Continue'
$ws = 'F:\agent-workspaces\ui4me-core\stab-374b06f6'
$out = "$ws\results\winsmoke2"; New-Item -ItemType Directory -Force $out | Out-Null
Set-Location "$ws\probes"
$env:UI4ME_HOST_BINARY = "$ws\target\release\ui4me-host.exe"; $env:PYTHONPATH = "$ws\src\python"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$log = "$out\uia.txt"
$p = Start-Process -FilePath "$ws\py\cpython-3.13-windows-x86_64-none\python.exe" -ArgumentList 'winsmoke.py' -RedirectStandardOutput "$out\app.out.txt" -RedirectStandardError "$out\app.err.txt" -PassThru
$null = $p.Handle
$root = [System.Windows.Automation.AutomationElement]::RootElement
$nameCond = { param($n) New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $n) }
$t0 = Get-Date; $dlg = $null
while (-not $dlg -and ((Get-Date)-$t0).TotalSeconds -lt 40) { Start-Sleep -Milliseconds 500; $dlg = $root.FindFirst([System.Windows.Automation.TreeScope]::Descendants, (& $nameCond 'UI4ME smoke open')) }
if ($dlg) {
  "DIALOG_FOUND class=$($dlg.Current.ClassName) pid=$($dlg.Current.ProcessId)" | Out-File -Append $log
  $btn = New-Object System.Windows.Automation.AndCondition((& $nameCond 'Cancel'), (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button)))
  $cancel = $dlg.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $btn)
  try {
    if ($cancel) { $cancel.GetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern).Invoke(); 'CANCEL_BUTTON_INVOKED' | Out-File -Append $log }
    else { $dlg.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close(); 'DIALOG_WINDOWPATTERN_CLOSED' | Out-File -Append $log }
  } catch { "CANCEL_ERROR $_" | Out-File -Append $log }
} else { 'DIALOG_NOT_FOUND' | Out-File -Append $log }
Start-Sleep 1.5
$win = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, (& $nameCond 'winsmoke'))
if ($win) {
  "WINDOW_FOUND class=$($win.Current.ClassName)" | Out-File -Append $log
  $all = $win.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($e in $all) { "  {0} | {1} | focusable={2}" -f $e.Current.ControlType.ProgrammaticName, $e.Current.Name, $e.Current.IsKeyboardFocusable | Out-File -Append $log }
} else { 'WINDOW_NOT_FOUND' | Out-File -Append $log }
if (-not $p.WaitForExit(30000)) { 'TIMEOUT' | Out-File -Append $log; Stop-Process -Id $p.Id -Force }
"exit=$($p.ExitCode)" | Out-File -Append $log
