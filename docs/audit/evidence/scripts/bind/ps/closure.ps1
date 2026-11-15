function Update-Status([string]$t) { "Update-Status($t)" }
$window = 'script-window'
$script:counter = 0
function Add-Handler([string]$Label) {
    return { 
        $a = try { Update-Status $Label } catch { "function call failed: $($_.Exception.Message)" }
        $b = "window=[$window]"
        $script:counter++
        "$a | $b | script:counter(in closure)=$script:counter"
    }.GetNewClosure()
}
$sb = Add-Handler 'L1'
& $sb
"script counter after = $script:counter"
