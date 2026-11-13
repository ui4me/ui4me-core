# Deterministic local JSON-RPC peer for transport tests. It is NOT a native
# host and proves nothing about rendering or desktop integration; see the
# native GUI scenarios for that. It listens on the same kind of endpoint the
# Rust host uses (Unix socket or Windows named pipe) and speaks the same
# length-prefixed JSON-RPC framing.
[CmdletBinding()]
param([Parameter(Mandatory)][string]$Endpoint, [Parameter(Mandatory)][string]$ReadyFile,
      [Parameter(Mandatory)][string]$ModulePath, [string]$Token = 'testing-token')
$ErrorActionPreference = 'Stop'
Import-Module $ModulePath -Force
& (Get-Module UI4ME.Core) { Initialize-UI4MEHelper }
$utf8 = New-Object Text.UTF8Encoding($false, $true)
$stream = $null; $server = $null; $listener = $null
$bitmapFiles = New-Object System.Collections.ArrayList
$unix = [Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT
$stats = @{ cancels = New-Object System.Collections.ArrayList; codes = New-Object System.Collections.ArrayList; notifications = 0 }

function Read-Bytes([IO.Stream]$s, [int]$count) {
    $buf = New-Object byte[] $count
    $pos = 0
    while ($pos -lt $count) { $n = $s.Read($buf, $pos, $count - $pos); if ($n -le 0) { return $null }; $pos += $n }
    return , $buf
}
function Read-Envelope {
    $h = Read-Bytes $stream 4; if ($null -eq $h) { return $null }
    $count = ([int]$h[0] -shl 24) -bor ([int]$h[1] -shl 16) -bor ([int]$h[2] -shl 8) -bor [int]$h[3]
    $data = Read-Bytes $stream $count; if ($null -eq $data) { return $null }
    return [UI4ME.Json]::Parse($utf8.GetString($data))
}
function Write-Raw([byte[]]$body) {
    $n = $body.Length
    $stream.Write([byte[]]@([byte](($n -shr 24) -band 255), [byte](($n -shr 16) -band 255), [byte](($n -shr 8) -band 255), [byte]($n -band 255)), 0, 4)
    $stream.Write($body, 0, $n); $stream.Flush()
}
function Write-Envelope($message) { Write-Raw $utf8.GetBytes([UI4ME.Json]::Serialize($message)) }
function Respond($id, $result) { Write-Envelope @{ jsonrpc = '2.0'; id = [string]$id; result = $result } }
function RespondError($id, [int]$code, [string]$message) { Write-Envelope @{ jsonrpc = '2.0'; id = [string]$id; error = @{ code = $code; message = $message; data = @{ detail = 'mock' } } } }
function Notify($method, $params) { Write-Envelope @{ jsonrpc = '2.0'; method = $method; params = $params } }
# Reads frames until the response with this id; counts other traffic.
function Await-Response([string]$id) {
    while ($true) {
        $r = Read-Envelope
        if ($null -eq $r) { throw "Peer closed while waiting for $id" }
        if ($r.PSObject.Properties['method']) {
            if ($r.method -eq 'request.cancel') { [void]$stats.cancels.Add($r.params.id) }
            continue
        }
        if ($r.id -eq $id) { return $r }
        if ($r.PSObject.Properties['error']) { [void]$stats.codes.Add([int]$r.error.code) }
    }
}

try {
    if ($unix) {
        $listener = [System.Net.Sockets.Socket]::new([System.Net.Sockets.AddressFamily]::Unix,
            [System.Net.Sockets.SocketType]::Stream, [System.Net.Sockets.ProtocolType]::Unspecified)
        $listener.Bind([System.Net.Sockets.UnixDomainSocketEndPoint]::new($Endpoint))
        $listener.Listen(1)
        [IO.File]::WriteAllText($ReadyFile, 'ready')
        $stream = [System.Net.Sockets.NetworkStream]::new($listener.Accept(), $true)
    } else {
        $server = New-Object IO.Pipes.NamedPipeServerStream($Endpoint.Substring(9), [IO.Pipes.PipeDirection]::InOut, 1,
            [IO.Pipes.PipeTransmissionMode]::Byte, [IO.Pipes.PipeOptions]::Asynchronous)
        [IO.File]::WriteAllText($ReadyFile, 'ready')
        $server.WaitForConnection(); $stream = $server
    }
    $text = @{}
    $wid = 'w-1'; $did = 'd-0'; $documents = 0
    while ($true) {
        $m = Read-Envelope
        if ($null -eq $m) { break }
        if (-not $m.PSObject.Properties['method']) {
            if ($m.PSObject.Properties['error']) { [void]$stats.codes.Add([int]$m.error.code) }
            continue
        }
        if (-not $m.PSObject.Properties['id']) {
            if ($m.method -eq 'request.cancel') { [void]$stats.cancels.Add($m.params.id) } else { $stats.notifications++ }
            continue
        }
        $id = $m.id
        switch ([string]$m.method) {
            'host.hello' {
                if ($m.params.token -ne $Token -or $m.params.protocol_version -ne 1) { RespondError $id -32001 'Authentication failed'; break }
                Respond $id @{ protocol_version = 1; host = @{ name = 'mock'; version = '1.0.0' } }
            }
            'window.create' { Respond $id @{ window_id = $wid; title = $m.params.title } }
            'window.set' { Respond $id @{ window_id = $wid; echo = $m.params } }
            'document.load' { $documents++; $did = 'd-' + $documents; Respond $id @{ document_id = $did; window_id = $wid } }
            'document.unload' { Respond $id @{ unloaded = $true } }
            'app.ready' { Respond $id @{ ready = $true } }
            'mock.stats' { Respond $id @{ cancels = $stats.cancels.ToArray(); codes = $stats.codes.ToArray(); notifications = $stats.notifications } }
            'mock.echo' { Respond $id @{ params = $m.params } }
            'mock.never' { }  # never answered: exercises timeout and cancellation
            'mock.late' {
                # Answers the previous never-answered request after it was cancelled.
                Respond $m.params.target @{ late = $true }
                Respond $id @{ ok = $true }
            }
            'mock.disconnect' { Respond $id @{ ok = $true }; $stream.Flush(); $stream.Dispose(); $stream = $null; break }
            'mock.garbage' { Respond $id @{ ok = $true }; Write-Raw $utf8.GetBytes('{"jsonrpc":"2.0","id":7,"result":{}}') }
            'mock.flood_notifications' {
                $count = [int]$m.params.count
                for ($i = 0; $i -lt $count; $i++) { Notify 'test.tick' @{ n = $i; pad = ('x' * 64) } }
                Respond $id @{ sent = $count }
            }
            'mock.flood_requests' {
                $count = [int]$m.params.count
                for ($i = 0; $i -lt $count; $i++) { Write-Envelope @{ jsonrpc = '2.0'; id = "flood-$i"; method = 'test.unknown'; params = @{} } }
                Respond $id @{ sent = $count }
                $seen = 0
                while ($seen -lt $count) {
                    $r = Read-Envelope
                    if ($null -eq $r) { throw 'Peer closed during flood' }
                    if (-not $r.PSObject.Properties['method'] -and ([string]$r.id).StartsWith('flood-')) { [void]$stats.codes.Add([int]$r.error.code); $seen++ }
                }
            }
            'document.command' {
                $cmd = $m.params.command
                if ($m.params.document_id -ne $did) { RespondError $id -32004 'Stale document'; break }
                switch ([string]$cmd.op) {
                    'set_text' { $text[[string]$cmd.element] = [string]$cmd.text; Respond $id @{ updated = $true } }
                    'get_text' { Respond $id @{ text = $text[[string]$cmd.element] } }
                    'raise_error' { RespondError $id -32003 'Synthetic policy rejection' }
                    'emit' {
                        Respond $id @{ ok = $true }
                        foreach ($e in @($cmd.events)) {
                            Notify 'document.event' @{ window_id = $wid; document_id = $did; event = @{ type = $e.type; element = $e.element; path = @($e.element, 'root') } }
                        }
                    }
                    'emit_stale' {
                        Respond $id @{ ok = $true }
                        Notify 'document.event' @{ window_id = $wid; document_id = 'd-old'; event = @{ type = 'click'; element = 'save'; path = @('save') } }
                    }
                    'emit_fatal' {
                        Respond $id @{ ok = $true }
                        Notify 'document.error' @{ window_id = $wid; document_id = $did; message = 'Renderer failed'; fatal = $true }
                    }
                    'echo' { Respond $id @{ command = $cmd } }
                    default { Respond $id @{ updated = $true; op = $cmd.op } }
                }
            }
            'window.request_close' {
                Write-Envelope @{ jsonrpc = '2.0'; id = 'decision-close'; method = 'window.before_close'; params = @{ window_id = $wid; reason = 'backend_request' } }
                $decision = Await-Response 'decision-close'
                Respond $id @{ requested = $true }
                if ($decision.result.allow) { Notify 'window.event' @{ window_id = $wid; type = 'closed' } }
            }
            'window.get' { Respond $id @{ window_id = $wid; title = 'Mock' } }
            'host.capabilities' { Respond $id @{ native = $false } }
            'bitmap.create' {
                $count = [long]$m.params.width * [long]$m.params.height * 4
                $tmp = [IO.Path]::Combine([IO.Path]::GetTempPath(), 'ui4me-mock-' + [IO.Path]::GetRandomFileName())
                $f = [IO.File]::Open($tmp, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]'ReadWrite, Delete')
                $f.SetLength($count * [int]$m.params.slots); $f.Dispose()
                [void]$bitmapFiles.Add(@{ path = $tmp; bytes = $count })
                Respond $id @{ path = $tmp; stride = ([int]$m.params.width * 4); slot_bytes = $count; slots = [int]$m.params.slots }
            }
            'bitmap.present' {
                # Read back the first pixel of the presented slot to prove the mapping.
                $entry = $bitmapFiles[$bitmapFiles.Count - 1]
                $first = -1
                if (Test-Path -LiteralPath $entry.path) {
                    $fs = [IO.File]::Open($entry.path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]'ReadWrite, Delete')
                    try { [void]$fs.Seek([long]$m.params.slot * $entry.bytes, [IO.SeekOrigin]::Begin); $first = $fs.ReadByte() } finally { $fs.Dispose() }
                }
                Respond $id @{ presented = $true; sequence = $m.params.sequence; slot = $m.params.slot; first_byte = $first }
            }
            'bitmap.close' { Respond $id @{ closed = $true } }
            'app.request_quit' {
                Write-Envelope @{ jsonrpc = '2.0'; id = 'decision-quit'; method = 'app.before_quit'; params = @{ reason = 'backend_request' } }
                $decision = Await-Response 'decision-quit'
                Respond $id @{ requested = $true; allowed = [bool]$decision.result.allow }
                if ($decision.result.allow) {
                    Write-Envelope @{ jsonrpc = '2.0'; id = 'shutdown-1'; method = 'app.shutdown'; params = @{ deadline_ms = 5000 } }
                    $null = Await-Response 'shutdown-1'
                    break
                }
            }
            default { RespondError $id -32601 "Unknown method: $($m.method)" }
        }
        if ($null -eq $stream) { break }
    }
} finally {
    if ($null -ne $stream) { $stream.Dispose() }
    if ($null -ne $server) { $server.Dispose() }
    if ($null -ne $listener) { $listener.Dispose() }
    foreach ($entry in $bitmapFiles) { Remove-Item -LiteralPath $entry.path -Force -ErrorAction SilentlyContinue }
    if ($unix -and (Test-Path -LiteralPath $Endpoint)) { Remove-Item -LiteralPath $Endpoint -Force }
    Remove-Item -LiteralPath $ReadyFile -Force -ErrorAction SilentlyContinue
}
