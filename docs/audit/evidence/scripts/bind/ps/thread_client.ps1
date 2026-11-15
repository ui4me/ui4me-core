$ErrorActionPreference = 'Stop'
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
$app = Connect-UI4ME
$w = New-UI4MEWindow -Application $app
$doc = Open-UI4MEHtml -Window $w -Html '<p id=p>x</p>'
$job = Start-ThreadJob -ScriptBlock {
    Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1
    $d = $using:doc; $ok = 0; $errs = @()
    for ($i = 0; $i -lt 300; $i++) { try { Set-UI4MEDocumentText -Document $d -Element p -Text "j$i"; $ok++ } catch { $errs += $_.Exception.GetType().Name + ': ' + $_.Exception.Message } }
    "job ok=$ok errors=$($errs.Count) first=$($errs | Select-Object -First 2)"
}
$mainOk = 0; $mainErr = @()
for ($i = 0; $i -lt 300; $i++) { try { Set-UI4MEDocumentText -Document $doc -Element p -Text "m$i"; $mainOk++ } catch { $mainErr += $_.Exception.GetType().Name + ': ' + $_.Exception.Message } }
$r = $job | Wait-Job -Timeout 60 | Receive-Job
"main ok=$mainOk errors=$($mainErr.Count) first=$($mainErr | Select-Object -First 2)"
"job: $r (state=$($job.State))"
"pending=$($app.Pending.Count) replies=$($app.Replies.Count)"
try { $null = Invoke-UI4MERpc -Application $app -Method 'test.shutdown' -TimeoutMilliseconds 5000; Wait-UI4ME -Application $app -UntilShutdown } catch { "shutdown: $($_.Exception.Message)" }
Close-UI4ME -Application $app
