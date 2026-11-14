# Counter: click callbacks, atomic DOM batches, native clipboard, CSS from disk.
#
# Run it directly:
#   pwsh -File ./app.ps1                          (PowerShell 7.2+, Linux or Windows)
#   powershell.exe -NoProfile -File .\app.ps1     (Windows PowerShell 5.1)
# Or through the manifest:  Start-UI4MEHost -Manifest ./app.toml
$ErrorActionPreference = 'Stop'
# Use the installed module; from a source checkout, fall back to the module here.
try { Import-Module UI4ME.Core -MinimumVersion 1.0.1 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }

$app = Start-UI4MEScript -Name 'PowerShell Counter' -AppId 'org.ui4me.PowerShellCounter'

$window = New-UI4MEWindow -Application $app -Title 'UI4ME PowerShell Counter' -Width 760 -Height 520
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')
$script:counter = 0

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element increase -Callback {
    $script:counter++
    Invoke-UI4MEBatch -Document $doc -Operations @(
        @{ op = 'set_text'; element = 'count'; text = [string]$script:counter },
        @{ op = 'set_text'; element = 'status'; text = 'Updated by PowerShell at ' + (Get-Date -Format 'HH:mm:ss') }
    )
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element copy -Callback {
    Set-UI4MEClipboard -Application $app -Text "Count = $script:counter"
    Set-UI4MEDocumentText -Document $doc -Element status -Text 'Copied to the system clipboard.'
}

Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
