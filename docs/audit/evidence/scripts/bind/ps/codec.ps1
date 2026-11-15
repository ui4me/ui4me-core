Import-Module <repo>/powershell/UI4ME.Core/UI4ME.Core.psd1 -Force
& (Get-Module UI4ME.Core) { Initialize-UI4MEHelper }
function T($label, [scriptblock]$sb) {
  try { $r = & $sb; "{0,-40} => {1}" -f $label, $r } catch { "{0,-40} !! {1}: {2}" -f $label, $_.Exception.GetType().Name, $_.Exception.Message }
}
function S($v) { [UI4ME.Json]::Serialize($v) }
T 'switch param' { function f([switch]$x) { S @{ v = $x } }; f -x }
T 'single 0.1' { S @{ v = [single]0.1 } }
T 'double -0.0' { S @{ v = -0.0 } }
T 'double 1e21' { S @{ v = 1e21 } }
T 'double 5e-324' { S @{ v = 5e-324 } }
T 'decimal 1.50' { S @{ v = [decimal]'1.50' } }
T 'bigint' { S @{ v = [bigint]::Parse('123456789012345678901234567890') } }
T 'guid' { S @{ v = [guid]::Empty } }
T 'enum' { S @{ v = [ConsoleColor]::Red } }
T 'datetime' { S @{ v = Get-Date } }
T 'empty array' { S @{ v = @() } }
T 'single elem array' { S @{ v = @(1) } }
T 'nested single' { S @{ v = @(,@(1)) } }
T 'ordered dict' { S ([ordered]@{ b = 1; a = 2 }) }
T 'int key' { S @{ 1 = 'a' } }
T 'pscustomobject' { S ([pscustomobject]@{ a = 1; b = $null }) }
T 'pscustom+scriptprop' { $o = [pscustomobject]@{ a = 1 }; $o | Add-Member -MemberType ScriptProperty -Name s -Value { 42 }; S $o }
T 'Get-Content string' { $f = New-TemporaryFile; 'hi' | Set-Content $f; $s = Get-Content $f; Remove-Item $f; S @{ v = $s } }
T 'FileInfo' { S @{ v = (Get-Item /etc/hostname) } }
T 'string lone surrogate ser' { S @{ v = "a$([char]0xD800)b" } }
T 'lone surrogate utf8 bytes' { (New-Object Text.UTF8Encoding($false,$true)).GetBytes((S @{ v = "a$([char]0xD800)b" })).Length }
T 'hashtable w/ PSObject-wrapped key' { S @{ ([psobject]'k') = 1 } }
T 'char key' { S @{ [char]'k' = 1 } }
T 'deep 200' { $x = 1; for ($i=0;$i -lt 200;$i++){ $x = @{ a = $x } }; (S $x).Length }
T 'xml' { S @{ v = [xml]'<a/>' } }
T 'array list' { $l = New-Object System.Collections.ArrayList; [void]$l.Add(1); S @{ v = $l } }
T 'string as IEnumerable? (char[])' { S @{ v = [char[]]'ab' } }
T 'byte[]' { S @{ v = [byte[]](1,2) } }
T 'hashset' { $h = New-Object 'System.Collections.Generic.HashSet[int]'; [void]$h.Add(3); S @{ v = $h } }
T 'scriptblock' { S @{ v = { 1 } } }
T 'SecureString' { S @{ v = (New-Object Security.SecureString) } }
'--- parse ---'
function P($t) { $v = [UI4ME.Json]::Parse($t); if ($null -eq $v) { 'null' } else { "$($v.GetType().FullName) : $v" } }
T 'parse -0' { P '-0' }
T 'parse 1.0' { P '1.0' }
T 'parse 1e2' { P '1e2' }
T 'parse big' { P '123456789012345678901234567890' }
T 'parse ulong' { P '18446744073709551615' }
T 'parse -long-1' { P '-9223372036854775809' }
T 'parse 1e400' { P '1e400' }
T 'parse date string' { P '"2024-01-01T00:00:00Z"' }
T 'parse lone surrogate escape' { P '"\ud800"' }
T 'parse case collision type' { P '{"A":1,"a":2}' }
T 'parse empty obj' { $v=[UI4ME.Json]::Parse('{}'); "$($v.GetType().FullName) base=$($v.BaseObject.GetType().FullName)" }
T 'parse empty arr' { $v=[UI4ME.Json]::Parse('[]'); "$($v.GetType().FullName) len=$($v.Length)" }
T 'parse BOM' { P "$([char]0xFEFF){}" }
T 'parse leading zeros' { P '01' }
T 'parse NBSP ws' { P "$([char]0xA0)1" }
T 'parse deep 129' { P (('[' * 130) + (']' * 130)) }
T 'parse deep 128' { (P (('[' * 129) + (']' * 129))) -ne $null }
T 'parse NUL escape' { $v=[UI4ME.Json]::Parse('"a\u0000b"'); $v.Length }
T 'parse PS reserved prop name' { $v=[UI4ME.Json]::Parse('{"PSObject":1,"Count":5,"Length":7}'); "Count=$($v.Count) Length=$($v.Length) PSObject=$($v.PSObject.GetType().Name)" }
T 'parse key empty' { $v=[UI4ME.Json]::Parse('{"":1}'); $v.PSObject.Properties.Name -join ',' }
