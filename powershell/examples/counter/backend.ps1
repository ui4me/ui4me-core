# Counter: click callbacks, atomic DOM batches, native clipboard, CSS from disk.
# Started by the native host: Start-UI4MEHost -Manifest ./app.toml
$ErrorActionPreference = 'Stop'
try { Import-Module UI4ME.Core -MinimumVersion 1.0.0 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }
$app = Connect-UI4ME
try {
    $win = New-UI4MEWindow -Application $app -Title 'UI4ME PowerShell Counter' -Width 760 -Height 520
    $doc = Open-UI4MEFile -Window $win -Path (Join-Path $PSScriptRoot 'index.html')
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
} finally {
    Close-UI4ME -Application $app
}
