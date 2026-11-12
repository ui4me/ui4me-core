[CmdletBinding()]
param(
    [int]$TargetProcessId = 0,
    [string]$WindowTitle = '',
    [string]$AutomationId = '',
    [string]$ControlName = '',
    [switch]$FirstEdit,
    [ValidateSet('pattern','document-text','select','selection','selection-text')]
    [string]$Mode = 'selection',
    [ValidateSet('MTA','STA')][string]$Apartment = 'MTA',
    [switch]$Focus,
    [Parameter(Mandatory=$true)][string]$LogPath,
    [int]$TimeoutSeconds = 15
)
$ErrorActionPreference = 'Stop'
if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 300) { throw 'Timeout must be 1..300 seconds' }
if ($TargetProcessId -le 0 -and [string]::IsNullOrWhiteSpace($WindowTitle)) {
    throw 'Provide a target PID or a distinctive window title'
}
$selectorCount = [int](-not [string]::IsNullOrWhiteSpace($AutomationId)) +
    [int](-not [string]::IsNullOrWhiteSpace($ControlName)) + [int]$FirstEdit.IsPresent
if ($selectorCount -gt 1) { throw 'Choose only one of AutomationId, ControlName, or FirstEdit' }
if ($selectorCount -eq 0) { $AutomationId = 'name' }
$LogPath = [IO.Path]::GetFullPath($LogPath)
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($LogPath)) | Out-Null
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, WindowsBase
$refs = @([System.Windows.Automation.AutomationElement].Assembly.Location,
          [System.Windows.Automation.ControlType].Assembly.Location,
          [System.Windows.DependencyObject].Assembly.Location)
Add-Type -Path (Join-Path $PSScriptRoot 'UiaTextProbe.cs') -ReferencedAssemblies $refs
$result = [UiaTextProbe]::Run($TargetProcessId, $WindowTitle, $AutomationId,
    $ControlName, $Mode, $Apartment, $Focus.IsPresent, $LogPath, $TimeoutSeconds)
Get-Content -LiteralPath $LogPath -Encoding UTF8
exit $result
