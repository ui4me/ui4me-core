param()
$ErrorActionPreference = 'Continue'
$ws = 'F:\agent-workspaces\ui4me-core\stab-374b06f6'
$cfg = Get-Content "$ws\probes\nat-config.txt"   # lines: tag|python|hostOrEmpty|pythonpathOrEmpty
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class NI {
  [StructLayout(LayoutKind.Sequential)] public struct KI { public ushort vk; public ushort scan; public uint flags; public uint time; public IntPtr extra; }
  [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public KI ki; public long pad; }
  [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] i, int size);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, int x, int y, uint d, IntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr LoadKeyboardLayout(string id, uint flags);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern IntPtr GetKeyboardLayout(uint t);
  public static void Key(ushort scan, bool up) {
    var i = new INPUT[1]; i[0].type = 1; i[0].ki.scan = scan; i[0].ki.flags = 0x0008u | (up ? 0x0002u : 0u);
    SendInput(1, i, Marshal.SizeOf(typeof(INPUT)));
  }
}
"@
[NI]::SetProcessDPIAware() | Out-Null
function Tap([int]$s) { [NI]::Key($s, $false); Start-Sleep -Milliseconds 40; [NI]::Key($s, $true); Start-Sleep -Milliseconds 150 }
function Chord([int]$s) { [NI]::Key(0x1D, $false); Start-Sleep -Milliseconds 40; Tap $s; [NI]::Key(0x1D, $true); Start-Sleep -Milliseconds 200 }
function Click([int]$x, [int]$y) { [NI]::SetCursorPos($x, $y) | Out-Null; Start-Sleep -Milliseconds 80; [NI]::mouse_event(2,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 60; [NI]::mouse_event(4,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 250 }
$root = [System.Windows.Automation.AutomationElement]::RootElement
function ByName($scope, $n) { $scope.FindFirst([System.Windows.Automation.TreeScope]::Descendants, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $n))) }
function Rect($e) { if ($e) { $r = $e.Current.BoundingRectangle; "{0:N0},{1:N0} {2:N0}x{3:N0}" -f $r.X, $r.Y, $r.Width, $r.Height } else { 'none' } }
function Center($e) { $r = $e.Current.BoundingRectangle; @([int]($r.X + $r.Width/2), [int]($r.Y + $r.Height/2)) }
function Clip { try { (Get-Clipboard -Raw) } catch { "CLIPERR $_" } }
foreach ($line in $cfg) {
  $tag, $py, $hostexe, $pp = $line.Split('|')
  $out = "$ws\results\native\$tag"; New-Item -ItemType Directory -Force $out | Out-Null
  $L = "$out\driver.txt"; Remove-Item "$out\events.jsonl" -ErrorAction SilentlyContinue
  function Log($m) { $m | Out-File -Append -Encoding utf8 $L }
  $env:NAT_LOG = "$out\events.jsonl"; $env:NAT_TAG = $tag
  if ($hostexe) { $env:UI4ME_HOST_BINARY = $hostexe } else { Remove-Item Env:UI4ME_HOST_BINARY -ErrorAction SilentlyContinue }
  if ($pp) { $env:PYTHONPATH = $pp } else { Remove-Item Env:PYTHONPATH -ErrorAction SilentlyContinue }
  $p = Start-Process -FilePath $py -ArgumentList "$ws\probes\nat_app.py" -RedirectStandardOutput "$out\app.out.txt" -RedirectStandardError "$out\app.err.txt" -PassThru
  $null = $p.Handle
  $win = $null; $t0 = Get-Date
  while (-not $win -and ((Get-Date)-$t0).TotalSeconds -lt 40) { Start-Sleep -Milliseconds 500; $win = $root.FindFirst([System.Windows.Automation.TreeScope]::Children, (New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, 'natprobe'))) }
  if (-not $win) { R 'WINDOW_NOT_FOUND'; Stop-Process -Id $p.Id -Force; continue }
  Start-Sleep 1.5
  $hwnd = [IntPtr]$win.Current.NativeWindowHandle
  [NI]::SetForegroundWindow($hwnd) | Out-Null; Start-Sleep -Milliseconds 400
  Log "WINDOW $(Rect $win)"
  # 1. Ctrl+A / Ctrl+C in the input, US layout
  $inp = ByName $win 'Name field'; Log "INPUT $(Rect $inp)"
  $c = Center $inp; Click $c[0] $c[1]; Set-Clipboard -Value 'none'
  Chord 0x1E; Chord 0x2E; Start-Sleep -Milliseconds 300; Log "CLIP_US [$(Clip)]"
  # 2. Tab to the link, Enter
  Tap 0x0F; Start-Sleep -Milliseconds 300
  $fe = [System.Windows.Automation.AutomationElement]::FocusedElement; Log "FOCUS_AFTER_TAB1 [$($fe.Current.Name)] $($fe.Current.ControlType.ProgrammaticName)"
  Log "MARK before_enter"; Tap 0x1C; Start-Sleep -Milliseconds 500; Log "MARK after_enter"
  # 3. Tab to the far button: must scroll into view
  $far = ByName $win 'Far button'; Log "FAR_BEFORE $(Rect $far) offscreen=$($far.Current.IsOffscreen)"
  Tap 0x0F; Start-Sleep -Milliseconds 700
  $far = ByName $win 'Far button'; $fe = [System.Windows.Automation.AutomationElement]::FocusedElement
  Log "FOCUS_AFTER_TAB2 [$($fe.Current.Name)]"; Log "FAR_AFTER $(Rect $far) offscreen=$($far.Current.IsOffscreen)"
  $wr = $win.Current.BoundingRectangle; $fr = $far.Current.BoundingRectangle
  Log ("FAR_VISIBLE " + ($fr.Y -ge $wr.Y -and ($fr.Y + $fr.Height) -le ($wr.Y + $wr.Height)))
  # 5. Russian layout in the app window, Ctrl+A / Ctrl+C again
  $hkl = [NI]::LoadKeyboardLayout('00000419', 1)
  [NI]::PostMessage($hwnd, 0x50, [IntPtr]::Zero, $hkl) | Out-Null; Start-Sleep -Milliseconds 600
  $tid = [NI]::GetWindowThreadProcessId($hwnd, [IntPtr]::Zero); $lay = [NI]::GetKeyboardLayout($tid).ToInt64() -band 0xFFFF; Log ("LAYOUT 0x{0:X}" -f $lay)
  if ($lay -ne 0x419) {
    $ll = Get-WinUserLanguageList; if (-not ($ll | Where-Object LanguageTag -eq 'ru')) { $ll.Add('ru-RU'); Set-WinUserLanguageList $ll -Force; Log 'ADDED ru-RU to the test user language list' }
    Start-Sleep 1; $hkl = [NI]::LoadKeyboardLayout('00000419', 1); [NI]::SetForegroundWindow($hwnd) | Out-Null; [NI]::PostMessage($hwnd, 0x50, [IntPtr]::Zero, $hkl) | Out-Null; Start-Sleep -Milliseconds 800
    Log ("LAYOUT_RETRY 0x{0:X}" -f ([NI]::GetKeyboardLayout($tid).ToInt64() -band 0xFFFF))
  }
  # scroll back to the input by clicking it via UIA focus, then select+copy
  $inp = ByName $win 'Name field'; try { $inp.SetFocus() } catch { Log "SETFOCUS_ERR $_" }; Start-Sleep -Milliseconds 500
  $inp = ByName $win 'Name field'; Log "INPUT2 $(Rect $inp)"; $c = Center $inp; Click $c[0] $c[1]
  Set-Clipboard -Value 'none'; Log "MARK before_ru"; Chord 0x1E; Chord 0x2E; Start-Sleep -Milliseconds 300; Log "CLIP_RU [$(Clip)]"; Log "MARK after_ru"; Tap 0x1E; Start-Sleep -Milliseconds 300; Log "MARK after_plain_ru"
  # 4. press on one button, release on another; then same-node click
  [NI]::Key(0x2A, $false); Tap 0x0F; Tap 0x0F; [NI]::Key(0x2A, $true); Start-Sleep -Milliseconds 500
  $b1 = ByName $win 'Press here'; $b2 = ByName $win 'Release here'; $c1 = Center $b1; $c2 = Center $b2
  Log "PR1 $(Rect $b1) PR2 $(Rect $b2) WIN $(Rect $win)"
  Log "MARK before_split"; Add-Content -Encoding utf8 $env:NAT_LOG '{"t": "mark_split"}'
  [NI]::SetCursorPos($c1[0], $c1[1]) | Out-Null; Start-Sleep -Milliseconds 100; [NI]::mouse_event(2,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 150
  [NI]::SetCursorPos($c2[0], $c2[1]) | Out-Null; Start-Sleep -Milliseconds 150; [NI]::mouse_event(4,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 400
  Log "MARK after_split"; Add-Content -Encoding utf8 $env:NAT_LOG '{"t": "mark_same"}'; Click $c2[0] $c2[1]; Log "MARK after_same"
  $ib = ByName $win 'Icon button'; $r = $ib.Current.BoundingRectangle; Log "IB $(Rect $ib)"
  Add-Content -Encoding utf8 $env:NAT_LOG '{"t": "mark_icon"}'
  [NI]::SetCursorPos([int]($r.X + 4 + 12), [int]($r.Y + $r.Height/2)) | Out-Null; Start-Sleep -Milliseconds 100; [NI]::mouse_event(2,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 150
  [NI]::SetCursorPos([int]($r.X + $r.Width - 8), [int]($r.Y + $r.Height/2)) | Out-Null; Start-Sleep -Milliseconds 150; [NI]::mouse_event(4,0,0,0,[IntPtr]::Zero); Start-Sleep -Milliseconds 400
  Add-Content -Encoding utf8 $env:NAT_LOG '{"t": "mark_end"}'
  $us = [NI]::LoadKeyboardLayout('00000409', 1); [NI]::PostMessage($hwnd, 0x50, [IntPtr]::Zero, $us) | Out-Null
  Start-Sleep 0.5
  try { $win.GetCurrentPattern([System.Windows.Automation.WindowPattern]::Pattern).Close() } catch { Log "CLOSE_ERR $_" }
  if (-not $p.WaitForExit(20000)) { R 'TIMEOUT'; Stop-Process -Id $p.Id -Force }
  Log "exit=$($p.ExitCode)"
}
'DONE' | Out-File -Append "$ws\results\native\summary.txt"
