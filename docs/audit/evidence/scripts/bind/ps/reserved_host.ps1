param([string]$Attr = 'psbase')
$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Start-UI4MEScript -Name 'reserved' -Instance multiple
$w = New-UI4MEWindow -Application $app -Title t
$doc = Open-UI4MEHtml -Window $w -Html "<div id='x' $Attr='1' data-k='v'>hi</div>"
Set-UI4MEReady -Application $app
try {
  $r = Invoke-UI4MEDocumentCommand -Document $doc -Operation get_attributes -Parameters @{ element = 'x' }
  "attributes ok: $(@($r.attributes.PSObject.Properties.Name) -join ',')"
} catch {
  "get_attributes failed: $($_.Exception.GetType().FullName): $($_.Exception.Message)"
  "transport failure: $($app.Transport.Failure)"
}
try { Set-UI4MEDocumentText -Document $doc -Element x -Text 'after'; 'later command ok' } catch { "later command failed: $($_.Exception.GetType().Name): $($_.Exception.Message)" }
try { $null = Request-UI4MEQuit -Application $app; Wait-UI4ME -Application $app -UntilShutdown } catch { "quit failed: $($_.Exception.Message)" }
Close-UI4ME -Application $app
