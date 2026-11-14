# Frameless window: HTML title bar, native drag/resize regions, window controls.
#
# Run it directly:
#   pwsh -File ./app.ps1                          (PowerShell 7.2+, Linux or Windows)
#   powershell.exe -NoProfile -File .\app.ps1     (Windows PowerShell 5.1)
# Or through the manifest:  Start-UI4MEHost -Manifest ./app.toml
$ErrorActionPreference = 'Stop'
# Use the installed module; from a source checkout, fall back to the module here.
try { Import-Module UI4ME.Core -MinimumVersion 1.0.1 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }

$app = Start-UI4MEScript -Name 'PowerShell Frameless Window' -AppId 'org.ui4me.PowerShellFrameless'

$window = New-UI4MEWindow -Application $app -Title 'PowerShell Custom Chrome' -Width 900 -Height 590 -Decorations $false -MinWidth 480 -MinHeight 320
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element minimize -Callback {
    $null = Set-UI4MEWindow -Window $window -Minimized $true
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element maximize -Callback {
    # Get-UI4MEWindow separates requested from observed state; toggle what the
    # desktop actually did.
    $state = Get-UI4MEWindow -Window $window
    $null = Set-UI4MEWindow -Window $window -Maximized (-not [bool]$state.observed.maximized)
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element close -Callback {
    Close-UI4MEWindow -Window $window
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
