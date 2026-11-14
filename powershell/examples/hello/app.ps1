# Hello: the smallest complete UI4ME app in PowerShell.
#
# Run it directly:
#   pwsh -File ./app.ps1                          (PowerShell 7.2+, Linux or Windows)
#   powershell.exe -NoProfile -File .\app.ps1     (Windows PowerShell 5.1)
#
# Start-UI4MEScript starts the native window host, which runs this script a
# second time as its backend. Everything above that call runs twice, so keep
# real work below it.
$ErrorActionPreference = 'Stop'
# Use the installed module; from a source checkout, fall back to the module here.
try { Import-Module UI4ME.Core -MinimumVersion 1.0.1 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }

$app = Start-UI4MEScript -Name 'Hello UI4ME'

$window = New-UI4MEWindow -Application $app -Title 'Hello UI4ME' -Width 560 -Height 360
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')

$script:clicks = 0
$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element greet -Callback {
    $script:clicks++
    $name = (Get-UI4MEValue -Document $doc -Element name).Trim()
    if (-not $name) { $name = 'world' }
    # Text is never parsed as HTML, so user input is safe here.
    Set-UI4MEDocumentText -Document $doc -Element message -Text "Hello, $name! (button pressed $script:clicks time(s))"
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
