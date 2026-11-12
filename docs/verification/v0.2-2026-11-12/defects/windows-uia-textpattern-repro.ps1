param([int]$Step)
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
$A = [System.Windows.Automation.AutomationElement]; $self = (Get-Process -Id $PID).SessionId; $t = $null
foreach ($p in @(Get-Process -Name ui4me-host | Where-Object SessionId -eq $self | ForEach-Object Id)) { foreach ($w in $A::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, [System.Windows.Automation.PropertyCondition]::new($A::ProcessIdProperty, $p))) { if ($w.Current.Name -like '*Desktop workspace*') { $t = $w } } }
$name = $t.FindFirst([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.PropertyCondition]::new($A::AutomationIdProperty, 'name'))
"TP start step $Step"
$tp = $name.GetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern); "TP got pattern"
$doc = $tp.DocumentRange; "TP doc text: " + $doc.GetText(100)
if ($Step -ge 2) { $r = $doc.Clone(); "TP cloned"; $n = $r.MoveEndpointByUnit([System.Windows.Automation.Text.TextPatternRangeEndpoint]::End, [System.Windows.Automation.Text.TextUnit]::Character, -5); "TP moved ${n}: " + $r.GetText(100) }
if ($Step -ge 3) { $r.Select(); Start-Sleep -Milliseconds 300; "TP selected" }
if ($Step -eq 5) { "TP selection only: " + (($tp.GetSelection() | ForEach-Object { $_.GetText(100) }) -join "|") }
if ($Step -eq 4) { "TP selection: " + (($tp.GetSelection() | ForEach-Object { $_.GetText(100) }) -join '|') }
"TP done"
