# Frameless window: HTML title bar, native drag/resize regions, window controls.
$ErrorActionPreference = 'Stop'
try { Import-Module UI4ME.Core -MinimumVersion 1.0.0 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }
$app = Connect-UI4ME
try {
    $win = New-UI4MEWindow -Application $app -Title 'PowerShell Custom Chrome' -Width 900 -Height 590 -Decorations $false -MinWidth 480 -MinHeight 320
    $doc = Open-UI4MEFile -Window $win -Path (Join-Path $PSScriptRoot 'index.html')
    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element minimize -Callback {
        $null = Set-UI4MEWindow -Window $win -Minimized $true
    }
    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element maximize -Callback {
        $state = Get-UI4MEWindow -Window $win
        $null = Set-UI4MEWindow -Window $win -Maximized (-not [bool]$state.observed.maximized)
    }
    $null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element close -Callback {
        Close-UI4MEWindow -Window $win
    }
    Set-UI4MEReady -Application $app
    Wait-UI4ME -Application $app -UntilShutdown
} finally {
    Close-UI4ME -Application $app
}
