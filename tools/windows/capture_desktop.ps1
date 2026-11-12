[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$OutputDirectory,
    [Parameter(Mandatory=$true)][int]$TargetProcessId,
    [string]$InvokeName
)
# Run with powershell.exe -STA inside the dedicated RDP session.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
$target = Get-Process -Id $TargetProcessId
$self = Get-Process -Id $PID
if ($target.SessionId -ne $self.SessionId -or $self.SessionId -eq 0) {
    throw 'Capture and target must run in the same interactive session.'
}
New-Item -ItemType Directory -Force -Path $OutputDirectory | Out-Null
$output = (Resolve-Path -LiteralPath $OutputDirectory).Path
$condition = [System.Windows.Automation.PropertyCondition]::new([System.Windows.Automation.AutomationElement]::ProcessIdProperty, $TargetProcessId)
$roots = [System.Windows.Automation.AutomationElement]::RootElement.FindAll([System.Windows.Automation.TreeScope]::Children, $condition)
if ($roots.Count -eq 0) { throw 'No UI Automation window was found for the target process in this desktop.' }
$records = [System.Collections.Generic.List[object]]::new()
$invoked = $false
foreach ($root in $roots) {
    $elements = $root.FindAll([System.Windows.Automation.TreeScope]::Subtree, [System.Windows.Automation.Condition]::TrueCondition)
    foreach ($element in $elements) {
        if ($records.Count -ge 500) { break }
        $current = $element.Current
        $records.Add([ordered]@{
            name = $current.Name; automation_id = $current.AutomationId
            control_type = $current.ControlType.ProgrammaticName
            enabled = $current.IsEnabled; focused = $current.HasKeyboardFocus
            offscreen = $current.IsOffscreen; rectangle = $current.BoundingRectangle.ToString()
        })
        if ($InvokeName -and -not $invoked -and $current.Name -eq $InvokeName) {
            $pattern = $null
            if (-not $element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
                throw "Element '$InvokeName' has no Invoke pattern"
            }
            ([System.Windows.Automation.InvokePattern]$pattern).Invoke()
            $invoked = $true
        }
    }
}
if ($InvokeName -and -not $invoked) { throw "No accessible element named '$InvokeName' was found" }
$screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = [System.Drawing.Bitmap]::new($screen.Width, $screen.Height)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
try {
    $graphics.CopyFromScreen($screen.Left, $screen.Top, 0, 0, $screen.Size)
    $bitmap.Save((Join-Path $output 'desktop.png'), [System.Drawing.Imaging.ImageFormat]::Png)
} finally { $graphics.Dispose(); $bitmap.Dispose() }
$report = [ordered]@{
    time_utc = (Get-Date).ToUniversalTime().ToString('o')
    target_pid = $TargetProcessId; session_id = $self.SessionId; session_name = $env:SESSIONNAME
    windows = $roots.Count; accessibility_nodes = $records; invoked_name = $InvokeName
    width = $screen.Width; height = $screen.Height
    qualification = 'OBSERVATION_ONLY'
    note = 'A screenshot and tree are evidence, not automatic proof of IME, focus or screen-reader correctness.'
}
$report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'desktop.json') -Encoding UTF8
Write-Output "Saved desktop observation to $output"
