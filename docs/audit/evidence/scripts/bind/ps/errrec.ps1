Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
& (Get-Module UI4ME.Core) { Initialize-UI4MEHelper }
$fakeWin = [pscustomobject]@{ Application = $null; Id = 'w'; Closed = $true; Document = $null }; $fakeWin.PSObject.TypeNames.Insert(0,'UI4ME.Window')
try { Get-UI4MEWindow -Window ([pscustomobject]@{}) -ErrorAction SilentlyContinue; 'no throw (SilentlyContinue honored)' } catch { "throws despite -ErrorAction SilentlyContinue: FQID=$($_.FullyQualifiedErrorId) Category=$($_.CategoryInfo.Category) Ex=$($_.Exception.GetType().Name) Target=$($_.TargetObject)" }
$r = & (Get-Module UI4ME.Core) { try { throw (New-Object -TypeName 'UI4ME.RpcException' -ArgumentList @('document.command', -32004, 'gone', $null)) } catch { $_ } }
"RpcException record: FQID=$($r.FullyQualifiedErrorId) Category=$($r.CategoryInfo.Category) Target=$($r.TargetObject)"
$r = & (Get-Module UI4ME.Core) { try { New-UI4MEStaleError 'x' | ForEach-Object { throw $_ } } catch { $_ } }
"Stale record: FQID=$($r.FullyQualifiedErrorId) Category=$($r.CategoryInfo.Category)"
# pipeline input?
$cmds = Get-Command -Module UI4ME.Core
$pipe = foreach ($c in $cmds) { foreach ($p in $c.Parameters.Values) { foreach ($a in $p.Attributes) { if ($a -is [System.Management.Automation.ParameterAttribute] -and ($a.ValueFromPipeline -or $a.ValueFromPipelineByPropertyName)) { "$($c.Name)-$($p.Name)" } } } }
"pipeline-bound parameters: $(@($pipe).Count)"
$sp = @($cmds | Where-Object { $_.CmdletBinding -and ($_.Parameters.ContainsKey('WhatIf')) })
"SupportsShouldProcess commands: $($sp.Count)"
$verbs = (Get-Verb).Verb
$bad = @($cmds | Where-Object { $verbs -notcontains $_.Verb } | ForEach-Object Name)
"unapproved verbs: $($bad -join ', ')"
$plural = @($cmds | Where-Object { $_.Noun -match 's$' -and $_.Noun -notmatch '(ss|Diagnostics|Capabilities|Preferences|Resources|Stats|Values|Options|Nodes|Stylesheets|Children)$' } | ForEach-Object Name)
"plural nouns (all): $(@($cmds | Where-Object { $_.Noun -match '(s)$' -and $_.Noun -notmatch 'ss$' } | ForEach-Object Name) -join ', ')"
"OutputType declared: $(@($cmds | Where-Object { $_.OutputType.Count -gt 0 }).Count) of $($cmds.Count)"
"exported count: $($cmds.Count)"
$m = Test-ModuleManifest <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1
"manifest version=$($m.Version) editions=$($m.CompatiblePSEditions -join ',') psver=$($m.PowerShellVersion) exportedFns=$($m.ExportedFunctions.Count)"
Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force -Verbose 4>&1 | Select-String -Pattern 'unapproved|WARNING' | Select-Object -First 3
