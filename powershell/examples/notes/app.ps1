# Notes: HTML and CSS files, a form, a list built from PowerShell and a native save dialog.
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

$app = Start-UI4MEScript -Name 'Notes' -AppId 'org.ui4me.PowerShellNotes'

$window = New-UI4MEWindow -Application $app -Title 'Notes' -Width 720 -Height 520
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')
$notes = New-Object System.Collections.Generic.List[string]

function Show-Notes {
    $items = foreach ($note in $notes) { '<li>' + [Net.WebUtility]::HtmlEncode($note) + '</li>' }
    $html = $items -join ''
    if (-not $html) { $html = '<li class="empty">No notes yet.</li>' }
    Set-UI4MEDocumentHtml -Document $doc -Element notes -Html $html
    Set-UI4MEDocumentText -Document $doc -Element count -Text ('{0} note(s)' -f $notes.Count)
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element add -Callback {
    $text = (Get-UI4MEValue -Document $doc -Element note).Trim()
    if (-not $text) {
        Set-UI4MEDocumentText -Document $doc -Element status -Text 'Write something first.'
        return
    }
    $notes.Add($text)
    Set-UI4MEDocumentValue -Document $doc -Element note -Value ''
    Set-UI4MEDocumentText -Document $doc -Element status -Text 'Added.'
    Show-Notes
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element clear -Callback {
    $notes.Clear()
    Set-UI4MEDocumentText -Document $doc -Element status -Text 'Cleared.'
    Show-Notes
}

$null = Register-UI4MEDocumentEvent -Document $doc -Type click -Element export -Callback {
    # The callback may wait for the native dialog; the window stays responsive.
    $result = Show-UI4MEFileDialog -Window $window -Kind SaveFile -FileName 'notes.txt' -Title 'Export notes'
    if ($result.cancelled -or @($result.paths).Count -eq 0) {
        Set-UI4MEDocumentText -Document $doc -Element status -Text 'Export cancelled.'
        return
    }
    $path = @($result.paths)[0]
    $utf8 = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText($path, (($notes -join "`n") + "`n"), $utf8)
    Set-UI4MEDocumentText -Document $doc -Element status -Text "Exported to $path"
}

Show-Notes
Set-UI4MEReady -Application $app
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
