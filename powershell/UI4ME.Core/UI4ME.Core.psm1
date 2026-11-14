# UI4ME Core - PowerShell binding for the versioned native host protocol.
#
# Supported: PowerShell 7.2+ on Linux and Windows, Windows PowerShell 5.1.
# The Rust host owns windows, input, layout, rendering and accessibility. This
# module owns the backend side of the private, authenticated JSON-RPC channel.
#
# Threading: a .NET reader thread parses frames into a queue. Every user
# scriptblock runs on the caller's PowerShell runspace while it pumps the
# queue (Wait-UI4ME, or any RPC that waits for its reply). Notifications are
# delivered one at a time in host order; while a callback runs, nested pumps
# still answer replies and host lifecycle requests but defer further
# notifications until the callback returns.
Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'
$script:UI4MEVersion = '1.0.1'
$script:UI4MEHelperLoaded = $false
$script:UI4MEIsWindows = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
$script:MaximumDeferred = 128
if ($PSVersionTable.PSEdition -eq 'Core' -and $PSVersionTable.PSVersion -lt [version]'7.2') {
    throw "UI4ME.Core requires PowerShell 7.2 or later (or Windows PowerShell 5.1); this is $($PSVersionTable.PSVersion)"
}

# ---------------------------------------------------------------- internals --

function Initialize-UI4MEHelper {
    if ($script:UI4MEHelperLoaded) { return }
    $existing = 'UI4ME.Transport' -as [type]
    if ($null -eq $existing) {
        $source = [IO.File]::ReadAllText((Join-Path $PSScriptRoot 'private/UI4ME.Transport.cs'), [Text.Encoding]::UTF8)
        if ($PSVersionTable.PSEdition -ne 'Core') { $source = "#define UI4ME_NETFX`n" + $source }
        Add-Type -TypeDefinition $source -Language CSharp -ErrorAction Stop
        $existing = 'UI4ME.Transport' -as [type]
    }
    if ($null -eq $existing -or $null -eq $existing.GetMethod('Register') -or $null -eq ('UI4ME.MappedBitmap' -as [type]) -or $null -eq ('UI4ME.HostProcess' -as [type])) {
        throw 'An incompatible UI4ME helper is already loaded in this process. Start a new PowerShell process.'
    }
    $script:UI4MEHelperLoaded = $true
}

function Write-UI4MELog {
    param([string]$Message)
    try { [Console]::Error.WriteLine("UI4ME: $Message"); [Console]::Error.Flush() } catch { $null = $_ }  # logging must never fail the app
}

function Format-UI4MEError {
    param($ErrorRecord)
    $text = "$($ErrorRecord.Exception.GetType().Name): $($ErrorRecord.Exception.Message)"
    $position = $ErrorRecord.InvocationInfo
    if ($null -ne $position -and $position.ScriptName) {
        $text += " ($($position.ScriptName):$($position.ScriptLineNumber))"
    }
    return $text
}

function Get-UI4MEField {
    param($Object, [string]$Name, $Default = $null)
    if ($null -eq $Object) { return $Default }
    if ($Object -is [System.Collections.IDictionary]) {
        if ($Object.Contains($Name)) { return $Object[$Name] }
        return $Default
    }
    $p = $Object.PSObject.Properties[$Name]
    if ($null -ne $p) { return $p.Value }
    return $Default
}

function New-UI4MEStaleError {
    param([string]$Message)
    Initialize-UI4MEHelper
    return (New-Object -TypeName 'UI4ME.StaleObjectException' -ArgumentList $Message)
}

function Test-UI4METype {
    param($Value, [string]$TypeName)
    return ($null -ne $Value -and $Value -is [psobject] -and $Value.PSObject.TypeNames.Count -gt 0 -and
        $Value.PSObject.TypeNames[0] -eq $TypeName)
}

function Assert-UI4MEApplication {
    param($Application)
    if (-not (Test-UI4METype $Application 'UI4ME.Application')) { throw 'Expected a UI4ME.Application from Connect-UI4ME' }
    if ($Application.Closed) { throw (New-UI4MEStaleError 'The UI4ME application connection was closed') }
    if ($Application.Disconnected) {
        throw (New-Object -TypeName 'UI4ME.DisconnectedException' -ArgumentList $Application.Transport.Failure)
    }
}
function Assert-UI4MEWindow {
    param($Window)
    if (-not (Test-UI4METype $Window 'UI4ME.Window')) { throw 'Expected a UI4ME.Window from New-UI4MEWindow' }
    Assert-UI4MEApplication $Window.Application
    if ($Window.Closed) { throw (New-UI4MEStaleError "UI4ME window $($Window.Id) is closed") }
}
function Assert-UI4MEDocument {
    param($Document)
    if (-not (Test-UI4METype $Document 'UI4ME.Document')) { throw 'Expected a UI4ME.Document from Open-UI4MEFile or Open-UI4MEHtml' }
    Assert-UI4MEWindow $Document.Window
    if ($Document.Retired -or $null -eq $Document.Window.Document -or
        -not [object]::ReferenceEquals($Document.Window.Document, $Document)) {
        throw (New-UI4MEStaleError 'This UI4ME document was replaced, unloaded, or its window closed')
    }
}
function Assert-UI4MENode {
    param($Node)
    if (-not (Test-UI4METype $Node 'UI4ME.Node')) { throw 'Expected a UI4ME.Node from Get-UI4MENode' }
    Assert-UI4MEDocument $Node.Document
}
function Get-UI4MEDocumentIds {
    param($Document)
    Assert-UI4MEDocument $Document
    return @{ window_id = $Document.Window.Id; document_id = $Document.Id }
}

# Adds element/node targeting for commands that accept either.
function Add-UI4METarget {
    param([hashtable]$Parameters, $Document, [string]$Element, $Node, [switch]$Required,
          [string]$ElementKey = 'element', [string]$NodeKey = 'node')
    if ($Element -and $null -ne $Node) { throw 'Use -Element or -Node, not both' }
    if ($null -ne $Node) {
        Assert-UI4MENode $Node
        if (-not [object]::ReferenceEquals($Node.Document, $Document)) { throw 'The node belongs to another document' }
        $Parameters[$NodeKey] = $Node.Id
    } elseif ($Element) {
        $Parameters[$ElementKey] = $Element
    } elseif ($Required) {
        throw 'An -Element id or a -Node is required'
    }
}

function Set-UI4MEDisconnected {
    param($Application)
    if ($Application.Disconnected) { return }
    $Application.Disconnected = $true
    foreach ($request in @($Application.Pending.Values)) { $request.Failed = $true }
    foreach ($window in @($Application.Windows.Values)) {
        $window.Closed = $true
        if ($null -ne $window.Document) { Remove-UI4MEDocumentState $window.Document }
    }
    $Application.Windows.Clear()
}

function Invoke-UI4MECallback {
    # Runs one user scriptblock; returns its output as an array.
    param($Application, [scriptblock]$Callback, [object[]]$Arguments)
    if ($Application.Depth -ge $Application.MaxDepth) { throw 'UI4ME callback nesting limit (32) exceeded' }
    $Application.Depth++
    $wasDispatching = $Application.Dispatching
    $Application.Dispatching = $true
    try { return ,@(& $Callback @Arguments) }
    finally {
        $Application.Dispatching = $wasDispatching
        $Application.Depth--
    }
}

function Invoke-UI4MEDocumentHandlers {
    param($Application, $Document, $EventObject)
    if ($null -eq $EventObject) { return }
    foreach ($pair in @(@('window_id', $Document.Window.Id), @('document_id', $Document.Id))) {
        if ($null -eq $EventObject.PSObject.Properties[$pair[0]]) {
            $EventObject.PSObject.Properties.Add((New-Object Management.Automation.PSNoteProperty($pair[0], $pair[1])))
        }
    }
    $kind = Get-UI4MEField $EventObject 'type'
    $target = Get-UI4MEField $EventObject 'element'
    $route = @(Get-UI4MEField $EventObject 'path' @())
    foreach ($entry in @($Document.Handlers.ToArray())) {
        if ($Document.Retired) { break }
        if ($entry.Type -ne $kind) { continue }
        if (-not $entry.Element -or $entry.Element -eq $target -or $route -contains $entry.Element) {
            try { $null = Invoke-UI4MECallback $Application $entry.Callback @($EventObject) }
            catch { Write-UI4MELog "event handler for '$kind' failed: $(Format-UI4MEError $_)" }
        }
    }
}

function Invoke-UI4MENotification {
    param($Application, $Message)
    $method = $Message.Method
    $params = $Message.Params
    switch -Exact ($method) {
        'app.shutdown' { $Application.ShutdownRequested = $true }
        'window.event' {
            $windowId = Get-UI4MEField $params 'window_id'
            if ($windowId -and $Application.Windows.ContainsKey($windowId) -and
                (Get-UI4MEField $params 'type') -eq 'closed') {
                $win = $Application.Windows[$windowId]
                $win.Closed = $true
                if ($null -ne $win.Document) { Remove-UI4MEDocumentState $win.Document }
                [void]$Application.Windows.Remove($windowId)
            }
        }
        { $_ -eq 'document.event' -or $_ -eq 'document.error' } {
            $windowId = Get-UI4MEField $params 'window_id'
            if (-not $windowId -or -not $Application.Windows.ContainsKey($windowId)) { break }
            $win = $Application.Windows[$windowId]
            $doc = $win.Document
            if ($win.Closed -or $null -eq $doc -or $doc.Retired -or $doc.Id -ne (Get-UI4MEField $params 'document_id')) { break }
            if ($method -eq 'document.event') {
                $evt = Get-UI4MEField $params 'event'
                if ($null -ne $evt -and $evt -is [psobject]) { Invoke-UI4MEDocumentHandlers $Application $doc $evt }
            } else {
                $text = [string](Get-UI4MEField $params 'message' 'Document operation failed')
                if ($text.Length -gt 4096) { $text = $text.Substring(0, 4096) }
                $fatal = [bool](Get-UI4MEField $params 'fatal' $false)
                Write-UI4MELog "document error ($($win.Id), $($doc.Id)): $text"
                try {
                    Invoke-UI4MEDocumentHandlers $Application $doc ([pscustomobject]@{ type = 'error'; message = $text; fatal = $fatal })
                } finally {
                    if ($fatal) {
                        # The host already unloaded the failed renderer. An error
                        # handler may have loaded a replacement; keep that one.
                        Remove-UI4MEDocumentState $doc
                        if ([object]::ReferenceEquals($win.Document, $doc)) { $win.Document = $null }
                    }
                }
            }
        }
    }
    if ($Application.Notifications.ContainsKey($method)) {
        try { $null = Invoke-UI4MECallback $Application $Application.Notifications[$method] @($params) }
        catch { Write-UI4MELog "notification handler for '$method' failed: $(Format-UI4MEError $_)" }
    }
}

function ConvertTo-UI4MEDecision {
    param([object[]]$Output)
    if ($Output.Count -eq 0) { return $false }
    return [bool]$Output[$Output.Count - 1]
}

function Invoke-UI4MEHostRequest {
    param($Application, $Message)
    $params = $Message.Params
    $result = $null
    $failure = $null
    try {
        switch -Exact ($Message.Method) {
            'window.before_close' {
                $id = Get-UI4MEField $params 'window_id'
                $callback = $null
                if ($id -and $Application.BeforeClose.ContainsKey($id)) { $callback = $Application.BeforeClose[$id] }
                if ($null -eq $callback) { $result = @{ allow = $true } }
                else { $result = @{ allow = (ConvertTo-UI4MEDecision (Invoke-UI4MECallback $Application $callback @($params))) } }
            }
            'app.before_quit' {
                if ($null -eq $Application.BeforeQuit) { $result = @{ allow = $true } }
                else { $result = @{ allow = (ConvertTo-UI4MEDecision (Invoke-UI4MECallback $Application $Application.BeforeQuit @($params))) } }
            }
            'app.shutdown' { $Application.ShutdownRequested = $true; $result = @{ ok = $true } }
            default { $failure = @{ code = -32601; message = "Unknown method: $($Message.Method)" } }
        }
    } catch {
        Write-UI4MELog "host request '$($Message.Method)' callback failed: $(Format-UI4MEError $_)"
        $failure = @{ code = -32603; message = "Callback failed: $($_.Exception.GetType().Name)" }
    }
    $envelope = @{ jsonrpc = '2.0'; id = $Message.Id }
    if ($null -ne $failure) { $envelope.error = $failure } else { $envelope.result = $result }
    try { $Application.Transport.Send($envelope) }
    catch { Set-UI4MEDisconnected $Application }
}

# Processes at most one queued message. Returns $true when one was handled.
function Invoke-UI4MEPump {
    param($Application, [int]$TimeoutMilliseconds)
    if (-not $Application.Dispatching -and $Application.Deferred.Count -gt 0) {
        Invoke-UI4MENotification $Application $Application.Deferred.Dequeue()
        return $true
    }
    if ($Application.Disconnected) {
        throw (New-Object -TypeName 'UI4ME.DisconnectedException' -ArgumentList $Application.Transport.Failure)
    }
    try { $message = $Application.Transport.Receive($TimeoutMilliseconds) }
    catch {
        Set-UI4MEDisconnected $Application
        throw (New-Object -TypeName 'UI4ME.DisconnectedException' -ArgumentList $Application.Transport.Failure)
    }
    if ($null -eq $message) { return $false }
    switch -Exact ($message.Kind) {
        'response' {
            if ($Application.Pending.ContainsKey($message.Id)) { $Application.Replies[$message.Id] = $message }
        }
        'request' { Invoke-UI4MEHostRequest $Application $message }
        default {
            if ($message.Method -eq 'app.shutdown') { $Application.ShutdownRequested = $true }
            if ($Application.Dispatching) {
                if ($Application.Deferred.Count -ge $script:MaximumDeferred) { $Application.DroppedNotifications++ }
                else { $Application.Deferred.Enqueue($message) }
            } else {
                Invoke-UI4MENotification $Application $message
            }
        }
    }
    return $true
}

# ------------------------------------------------------------ connection/RPC --

function Connect-UI4ME {
    <#
    .SYNOPSIS
    Connects this PowerShell backend to the native UI4ME host that launched it.
    .DESCRIPTION
    Reads UI4ME_ENDPOINT and UI4ME_TOKEN, opens the private Unix socket or Windows
    named pipe, and authenticates with host.hello (protocol version 1).
    #>
    [CmdletBinding()]
    param(
        [string]$Endpoint = $env:UI4ME_ENDPOINT,
        [string]$Token = $env:UI4ME_TOKEN,
        [ValidateRange(1, 120000)][int]$ConnectTimeoutMilliseconds = 15000
    )
    if ([string]::IsNullOrWhiteSpace($Endpoint) -or [string]::IsNullOrWhiteSpace($Token)) {
        throw 'UI4ME_ENDPOINT and UI4ME_TOKEN come from the native host. To run a script directly, use $app = Start-UI4MEScript instead of Connect-UI4ME; or start it with Start-UI4MEHost -Manifest app.toml.'
    }
    Initialize-UI4MEHelper
    $transport = New-Object -TypeName 'UI4ME.Transport' -ArgumentList @($Endpoint, $ConnectTimeoutMilliseconds)
    $app = [pscustomobject]@{
        Transport = $transport
        Hello = $null
        Windows = @{}
        Replies = @{}
        Pending = @{}
        Deferred = (New-Object 'System.Collections.Generic.Queue[object]')
        NextId = [long]0
        Depth = 0
        MaxDepth = 32
        Dispatching = $false
        Closed = $false
        Disconnected = $false
        ShutdownRequested = $false
        Ready = $false
        BeforeQuit = $null
        BeforeClose = @{}
        Notifications = @{}
        DroppedNotifications = [long]0
        DirectStart = $false
    }
    $app.PSObject.TypeNames.Insert(0, 'UI4ME.Application')
    try {
        $app.Hello = Invoke-UI4MERpc -Application $app -Method 'host.hello' -Parameters @{
            token = $Token; protocol_version = 1
            client = @{ name = 'ui4me-powershell'; version = $script:UI4MEVersion }
        }
        return $app
    } catch {
        $app.Closed = $true
        $transport.Dispose()
        throw
    }
}

function Start-UI4MERpc {
    <# .SYNOPSIS Sends a host request without waiting; returns a UI4ME.Request for Wait-UI4MERpc or Stop-UI4MERpc. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][string]$Method,
          [System.Collections.IDictionary]$Parameters = @{})
    Assert-UI4MEApplication $Application
    $Application.NextId++
    $id = 'ps-' + $Application.NextId.ToString([Globalization.CultureInfo]::InvariantCulture)
    $request = [pscustomobject]@{ Application = $Application; Id = $id; Method = $Method
        Completed = $false; Cancelled = $false; Failed = $false }
    $request.PSObject.TypeNames.Insert(0, 'UI4ME.Request')
    $Application.Transport.Register($id)
    $Application.Pending[$id] = $request
    try {
        $Application.Transport.Send(@{ jsonrpc = '2.0'; id = $id; method = $Method; params = $Parameters })
    } catch {
        [void]$Application.Pending.Remove($id)
        $Application.Transport.Forget($id)
        throw
    }
    return $request
}

function Stop-UI4MERpc {
    <# .SYNOPSIS Requests cancellation (for example closes a native dialog). Not a rollback. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Request)
    if (-not (Test-UI4METype $Request 'UI4ME.Request')) { throw 'Expected a UI4ME.Request from Start-UI4MERpc' }
    if ($Request.Completed -or $Request.Cancelled) { return }
    $app = $Request.Application
    $Request.Cancelled = $true
    [void]$app.Pending.Remove($Request.Id)
    [void]$app.Replies.Remove($Request.Id)
    $app.Transport.Forget($Request.Id)
    if (-not $app.Closed -and -not $app.Disconnected) {
        try { $app.Transport.Send(@{ jsonrpc = '2.0'; method = 'request.cancel'; params = @{ id = $Request.Id } }) }
        catch { Set-UI4MEDisconnected $app }
    }
}

function Wait-UI4MERpc {
    <#
    .SYNOPSIS
    Waits for a request's result while pumping events. -1 waits indefinitely.
    .DESCRIPTION
    On timeout the request is cancelled and UI4ME.RpcTimeoutException is thrown;
    the native outcome is then unknown. Host errors throw UI4ME.RpcException
    with Code, RpcMessage and RpcData.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Request, [ValidateRange(-1, 2147483647)][int]$TimeoutMilliseconds = 15000)
    if (-not (Test-UI4METype $Request 'UI4ME.Request')) { throw 'Expected a UI4ME.Request from Start-UI4MERpc' }
    $app = $Request.Application
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($true) {
        if ($Request.Cancelled) { throw (New-UI4MEStaleError "UI4ME request $($Request.Method) was cancelled") }
        if ($Request.Completed) { throw 'This UI4ME request was already awaited' }
        if ($app.Replies.ContainsKey($Request.Id)) {
            $message = $app.Replies[$Request.Id]
            [void]$app.Replies.Remove($Request.Id)
            [void]$app.Pending.Remove($Request.Id)
            $Request.Completed = $true
            if ($message.IsError) {
                throw (New-Object -TypeName 'UI4ME.RpcException' -ArgumentList @(
                    $Request.Method, $message.ErrorCode, $message.ErrorMessage, $message.ErrorData))
            }
            return $message.Result
        }
        if ($Request.Failed -or $app.Closed -or $app.Disconnected) {
            [void]$app.Pending.Remove($Request.Id)
            $reason = if ($app.Closed) { 'the application connection was closed' } else { $app.Transport.Failure }
            throw (New-Object -TypeName 'UI4ME.DisconnectedException' -ArgumentList "UI4ME $($Request.Method) did not complete: $reason")
        }
        $interval = 250
        if ($TimeoutMilliseconds -ge 0) {
            $remaining = $TimeoutMilliseconds - $clock.ElapsedMilliseconds
            if ($remaining -le 0) {
                Stop-UI4MERpc $Request
                throw (New-Object -TypeName 'UI4ME.RpcTimeoutException' -ArgumentList "UI4ME $($Request.Method) timed out after $TimeoutMilliseconds ms. Its outcome is unknown; cancellation was requested.")
            }
            $interval = [int][Math]::Min(250, $remaining)
        }
        [void](Invoke-UI4MEPump $app $interval)
    }
}

function Invoke-UI4MERpc {
    <#
    .SYNOPSIS
    Calls a native host JSON-RPC method and returns its result.
    .DESCRIPTION
    Events and lifecycle requests keep being processed while waiting. The default
    timeout is 15 seconds, or unlimited for dialog.* methods (user operated).
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][string]$Method,
          [System.Collections.IDictionary]$Parameters = @{}, [ValidateRange(-1, 2147483647)][int]$TimeoutMilliseconds = 15000)
    if (-not $PSBoundParameters.ContainsKey('TimeoutMilliseconds') -and $Method.StartsWith('dialog.')) { $TimeoutMilliseconds = -1 }
    $request = Start-UI4MERpc -Application $Application -Method $Method -Parameters $Parameters
    return (Wait-UI4MERpc -Request $request -TimeoutMilliseconds $TimeoutMilliseconds)
}

function Send-UI4MENotification {
    <# .SYNOPSIS Sends a JSON-RPC notification (no reply) to the host. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][string]$Method,
          [System.Collections.IDictionary]$Parameters = @{})
    Assert-UI4MEApplication $Application
    $Application.Transport.Send(@{ jsonrpc = '2.0'; method = $Method; params = $Parameters })
}

function Wait-UI4ME {
    <#
    .SYNOPSIS
    Pumps host events and runs callbacks on this PowerShell runspace.
    .DESCRIPTION
    Waits up to -TimeoutMilliseconds for the first message, then drains up to 64
    queued messages and returns how many were handled. -UntilShutdown pumps until
    the host requests shutdown, returns no output, and throws
    UI4ME.DisconnectedException if the host disappears first. -Once handles at
    most one message.
    #>
    [CmdletBinding(DefaultParameterSetName = 'Pump')]
    param([Parameter(Mandatory)]$Application,
          [Parameter(ParameterSetName = 'Pump')][ValidateRange(-1, 2147483647)][int]$TimeoutMilliseconds = 100,
          [Parameter(ParameterSetName = 'Pump')][switch]$Once,
          [Parameter(ParameterSetName = 'Run')][switch]$UntilShutdown)
    if (-not (Test-UI4METype $Application 'UI4ME.Application')) { throw 'Expected a UI4ME.Application from Connect-UI4ME' }
    if ($UntilShutdown) {
        # No output: a script's last statement must not print a stray value.
        if ($Application.Closed) { return }
        while (-not $Application.ShutdownRequested) {
            try { [void](Invoke-UI4MEPump $Application 1000) }
            catch [UI4ME.DisconnectedException] {
                if ($Application.ShutdownRequested) { break }
                throw
            }
        }
        return
    }
    if ($Application.Closed) { return 0 }
    $processed = 0
    try {
        if (Invoke-UI4MEPump $Application $TimeoutMilliseconds) {
            $processed++
            while (-not $Once -and $processed -lt 64 -and (Invoke-UI4MEPump $Application 0)) { $processed++ }
        }
    } catch [UI4ME.DisconnectedException] {
        if (-not $Application.ShutdownRequested) { throw }
    }
    return $processed
}

function Close-UI4ME {
    <# .SYNOPSIS Closes the host connection and retires every window, document and bitmap handle. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)][AllowNull()]$Application)
    if ($null -eq $Application -or -not (Test-UI4METype $Application 'UI4ME.Application') -or $Application.Closed) { return }
    $Application.Closed = $true
    foreach ($win in @($Application.Windows.Values)) {
        $win.Closed = $true
        if ($null -ne $win.Document) { Remove-UI4MEDocumentState $win.Document }
    }
    $Application.Windows.Clear(); $Application.Pending.Clear(); $Application.Replies.Clear(); $Application.Deferred.Clear()
    if ($Application.DirectStart -and -not $Application.ShutdownRequested -and -not $Application.Disconnected) {
        # A Start-UI4MEScript backend is a dedicated pwsh -File process that ends with the script.
        # Closing early (an error is usually unwinding through a finally block) would make the
        # host stop this process before PowerShell reports that error: process exit closes it.
        return
    }
    $Application.Transport.Dispose()
}

# ------------------------------------------------------------------ windows --

$script:WindowFields = [ordered]@{
    Title = 'title'; Width = 'width'; Height = 'height'; Visible = 'visible'; Resizable = 'resizable'
    Decorations = 'decorations'; Minimized = 'minimized'; Maximized = 'maximized'; Fullscreen = 'fullscreen'
    Focus = 'focus'; X = 'x'; Y = 'y'; MinWidth = 'min_width'; MinHeight = 'min_height'
    MaxWidth = 'max_width'; MaxHeight = 'max_height'
}

function New-UI4MEWindow {
    <# .SYNOPSIS Creates a native window. Load content with Open-UI4MEFile or Open-UI4MEHtml. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [string]$Title = '',
          [ValidateRange(1, 8192)][int]$Width = 900, [ValidateRange(1, 8192)][int]$Height = 600,
          [bool]$Visible = $true, [bool]$Resizable = $true, [bool]$Decorations = $true,
          [ValidateRange(1, 8192)][int]$MinWidth, [ValidateRange(1, 8192)][int]$MinHeight,
          [ValidateRange(1, 8192)][int]$MaxWidth, [ValidateRange(1, 8192)][int]$MaxHeight)
    Assert-UI4MEApplication $Application
    $p = @{ title = $Title; width = $Width; height = $Height; visible = $Visible; resizable = $Resizable; decorations = $Decorations }
    foreach ($key in @('MinWidth', 'MinHeight', 'MaxWidth', 'MaxHeight')) {
        if ($PSBoundParameters.ContainsKey($key)) { $p[$script:WindowFields[$key]] = $PSBoundParameters[$key] }
    }
    $res = Invoke-UI4MERpc -Application $Application -Method 'window.create' -Parameters $p
    $id = [string](Get-UI4MEField $res 'window_id')
    if ([string]::IsNullOrEmpty($id)) { throw 'The host did not return a window_id' }
    $win = [pscustomobject]@{ Application = $Application; Id = $id; Document = $null; Closed = $false }
    $win.PSObject.TypeNames.Insert(0, 'UI4ME.Window')
    $Application.Windows[$id] = $win
    return $win
}

function Set-UI4MEWindow {
    <#
    .SYNOPSIS
    Changes native window state: title, size, position, minimized/maximized/fullscreen, focus, constraints.
    .DESCRIPTION
    Pass typed parameters or a -Properties hashtable of host field names. Size constraints
    are set in pairs; pass $null in -Properties (e.g. @{min_width=$null;min_height=$null}) to clear.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window, [System.Collections.IDictionary]$Properties = @{},
          [string]$Title, [ValidateRange(1, 8192)][int]$Width, [ValidateRange(1, 8192)][int]$Height,
          [bool]$Visible, [bool]$Resizable, [bool]$Decorations, [bool]$Minimized, [bool]$Maximized,
          [bool]$Fullscreen, [bool]$Focus, [int]$X, [int]$Y,
          [ValidateRange(1, 8192)][int]$MinWidth, [ValidateRange(1, 8192)][int]$MinHeight,
          [ValidateRange(1, 8192)][int]$MaxWidth, [ValidateRange(1, 8192)][int]$MaxHeight)
    Assert-UI4MEWindow $Window
    $p = @{}
    foreach ($key in $Properties.Keys) { $p[[string]$key] = $Properties[$key] }
    foreach ($key in $script:WindowFields.Keys) {
        if ($PSBoundParameters.ContainsKey($key)) { $p[$script:WindowFields[$key]] = $PSBoundParameters[$key] }
    }
    $p.window_id = $Window.Id
    return (Invoke-UI4MERpc -Application $Window.Application -Method 'window.set' -Parameters $p)
}

function Get-UI4MEWindow {
    <# .SYNOPSIS Returns the host's description of a native window. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window)
    Assert-UI4MEWindow $Window
    return (Invoke-UI4MERpc -Application $Window.Application -Method 'window.get' -Parameters @{ window_id = $Window.Id })
}

function Get-UI4MEWindowList {
    <# .SYNOPSIS Lists the host's open windows of this application. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Application)
    return ,@(Invoke-UI4MERpc -Application $Application -Method 'window.list')
}

function Close-UI4MEWindow {
    <# .SYNOPSIS Requests a graceful close; Set-UI4MEBeforeClose policy may veto it. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window)
    Assert-UI4MEWindow $Window
    $null = Invoke-UI4MERpc -Application $Window.Application -Method 'window.request_close' -Parameters @{ window_id = $Window.Id }
}

function Request-UI4MEAttention {
    <# .SYNOPSIS Asks the desktop to flag the window (informational or critical). #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Window,
        [ValidateSet('informational', 'critical', 'none')][string]$Kind = 'informational')
    Assert-UI4MEWindow $Window
    $value = if ($Kind -eq 'none') { $null } else { $Kind }
    return (Invoke-UI4MERpc -Application $Window.Application -Method 'window.request_attention' -Parameters @{ window_id = $Window.Id; kind = $value })
}

function Set-UI4MEBeforeClose {
    <# .SYNOPSIS Sets the window close policy. The scriptblock's last output decides (true = allow). #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][AllowNull()][scriptblock]$Callback)
    Assert-UI4MEWindow $Window
    if ($null -eq $Callback) { [void]$Window.Application.BeforeClose.Remove($Window.Id) }
    else { $Window.Application.BeforeClose[$Window.Id] = $Callback }
}

function Set-UI4MEBeforeQuit {
    <# .SYNOPSIS Sets the application quit policy. The scriptblock's last output decides (true = allow). #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][AllowNull()][scriptblock]$Callback)
    Assert-UI4MEApplication $Application
    $Application.BeforeQuit = $Callback
}

function Set-UI4MEReady {
    <# .SYNOPSIS Tells the host that startup finished. Requires at least one window. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Application)
    Assert-UI4MEApplication $Application
    if (-not $Application.Ready) {
        $null = Invoke-UI4MERpc -Application $Application -Method 'app.ready'
        $Application.Ready = $true
    }
}

function Request-UI4MEQuit {
    <# .SYNOPSIS Requests an orderly application quit; Set-UI4MEBeforeQuit policy may veto it. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Application)
    return (Invoke-UI4MERpc -Application $Application -Method 'app.request_quit')
}

function Register-UI4MENotification {
    <#
    .SYNOPSIS
    Runs a scriptblock for a host notification method, e.g. desktop.preferences_changed or window.event.
    .DESCRIPTION
    One callback per method; pass $null to remove it. Built-in window and document
    bookkeeping always runs first.
    #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][string]$Method,
        [Parameter(Mandatory)][AllowNull()][scriptblock]$Callback)
    Assert-UI4MEApplication $Application
    if ($null -eq $Callback) { [void]$Application.Notifications.Remove($Method) }
    else { $Application.Notifications[$Method] = $Callback }
}

# ---------------------------------------------------------------- documents --

function New-UI4MEDocumentObject {
    param($Window, $Result)
    $id = [string](Get-UI4MEField $Result 'document_id')
    if ([string]::IsNullOrEmpty($id)) { throw 'The host did not return a document_id' }
    $doc = [pscustomobject]@{ Window = $Window; Id = $id; Retired = $false
        Handlers = (New-Object System.Collections.ArrayList); Bitmaps = (New-Object System.Collections.ArrayList) }
    $doc.PSObject.TypeNames.Insert(0, 'UI4ME.Document')
    # Retire the previous facade only after the atomic replacement succeeded.
    if ($null -ne $Window.Document) { Remove-UI4MEDocumentState $Window.Document }
    $Window.Document = $doc
    return $doc
}

function Remove-UI4MEDocumentState {
    param($Document)
    if ($null -eq $Document -or $Document.Retired) { return }
    $Document.Retired = $true
    $Document.Handlers.Clear()
    foreach ($bitmap in @($Document.Bitmaps.ToArray())) { Remove-UI4MELiveBitmap -Bitmap $bitmap -Quiet }
}

function Open-UI4MEHtml {
    <#
    .SYNOPSIS
    Loads generated HTML into a window, atomically replacing its document.
    .DESCRIPTION
    -AssetRoot names the folder that relative stylesheet, image and font URLs resolve against.
    The previous document object, its handlers, nodes and bitmaps become stale.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][AllowEmptyString()][string]$Html, [string]$AssetRoot)
    Assert-UI4MEWindow $Window
    $p = @{ window_id = $Window.Id; html = $Html }
    if ($AssetRoot) { $p.asset_root = Resolve-UI4MEPath $AssetRoot }
    $res = Invoke-UI4MERpc -Application $Window.Application -Method 'document.load' -Parameters $p -TimeoutMilliseconds 30000
    return (New-UI4MEDocumentObject $Window $res)
}

function Open-UI4MEFile {
    <# .SYNOPSIS Loads an HTML file into a window, atomically replacing its document. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window, [Parameter(Mandatory)][string]$Path, [string]$AssetRoot)
    Assert-UI4MEWindow $Window
    $p = @{ window_id = $Window.Id; path = (Resolve-UI4MEPath $Path) }
    if ($AssetRoot) { $p.asset_root = Resolve-UI4MEPath $AssetRoot }
    $res = Invoke-UI4MERpc -Application $Window.Application -Method 'document.load' -Parameters $p -TimeoutMilliseconds 30000
    return (New-UI4MEDocumentObject $Window $res)
}

function Resolve-UI4MEPath {
    # Resolves against the PowerShell location (not the process directory), keeping non-ASCII names intact.
    param([string]$Path)
    return $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
}

function Remove-UI4MEDocument {
    <# .SYNOPSIS Unloads the window's document; the window stays open and empty. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document)
    $ids = Get-UI4MEDocumentIds $Document
    $null = Invoke-UI4MERpc -Application $Document.Window.Application -Method 'document.unload' -Parameters $ids
    Remove-UI4MEDocumentState $Document
    if ([object]::ReferenceEquals($Document.Window.Document, $Document)) { $Document.Window.Document = $null }
}

function Register-UI4MEDocumentEvent {
    <#
    .SYNOPSIS
    Subscribes a scriptblock to document events (click, input, change, keydown, contextmenu, auxclick, submit, ...).
    .DESCRIPTION
    -Element delegates to an element id on the event's route. The callback receives the event
    object (type, element, path, plus window_id and document_id) after native dispatch. Inside a
    function, pass ({ ... }.GetNewClosure()) to capture local variables.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Type,
          [Parameter(Mandatory)][scriptblock]$Callback, [string]$Element)
    Assert-UI4MEDocument $Document
    if ($Document.Handlers.Count -ge 256) { throw 'Document callback limit (256) reached' }
    $sub = [pscustomobject]@{ Document = $Document; Type = $Type; Element = $Element; Callback = $Callback }
    $sub.PSObject.TypeNames.Insert(0, 'UI4ME.Subscription')
    [void]$Document.Handlers.Add($sub)
    return $sub
}

function Unregister-UI4MEDocumentEvent {
    <# .SYNOPSIS Removes a subscription returned by Register-UI4MEDocumentEvent. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Subscription)
    if (-not (Test-UI4METype $Subscription 'UI4ME.Subscription')) { throw 'Expected a UI4ME.Subscription' }
    [void]$Subscription.Document.Handlers.Remove($Subscription)
}

function Invoke-UI4MEDocumentCommand {
    <#
    .SYNOPSIS
    Runs any document command from the Core protocol (docs/reference/protocol.md) and returns its result.
    .EXAMPLE
    Invoke-UI4MEDocumentCommand -Document $doc -Operation get_html -Parameters @{ element = 'status'; outer = $true }
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Operation,
          [System.Collections.IDictionary]$Parameters = @{})
    $p = Get-UI4MEDocumentIds $Document
    $cmd = @{ op = $Operation }
    foreach ($key in $Parameters.Keys) {
        if ($key -eq 'op') { throw 'The operation is given by -Operation' }
        $cmd[[string]$key] = $Parameters[$key]
    }
    $p.command = $cmd
    return (Invoke-UI4MERpc -Application $Document.Window.Application -Method 'document.command' -Parameters $p)
}

function Invoke-UI4MEBatch {
    <# .SYNOPSIS Applies several id-addressed DOM operations as one atomic transaction. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][object[]]$Operations)
    $null = Invoke-UI4MEDocumentCommand -Document $Document -Operation 'batch' -Parameters @{ ops = $Operations }
}

function Get-UI4MEDocumentText {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element)
    return [string](Get-UI4MEField (Invoke-UI4MEDocumentCommand $Document 'get_text' @{ element = $Element }) 'text')
}
function Set-UI4MEDocumentText {
    <# .SYNOPSIS Replaces an element's content with text (never parsed as HTML). #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
          [Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    $null = Invoke-UI4MEDocumentCommand $Document 'set_text' @{ element = $Element; text = $Text }
}
function Get-UI4MEValue {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element)
    return [string](Get-UI4MEField (Invoke-UI4MEDocumentCommand $Document 'get_value' @{ element = $Element }) 'value')
}
function Set-UI4MEDocumentValue {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
          [Parameter(Mandatory)][AllowEmptyString()][string]$Value)
    $null = Invoke-UI4MEDocumentCommand $Document 'set_value' @{ element = $Element; value = $Value }
}
function Get-UI4MEChecked {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element)
    return [bool](Get-UI4MEField (Invoke-UI4MEDocumentCommand $Document 'get_checked' @{ element = $Element }) 'checked')
}
function Set-UI4MEChecked {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
        [Parameter(Mandatory)][bool]$Checked)
    $null = Invoke-UI4MEDocumentCommand $Document 'set_checked' @{ element = $Element; checked = $Checked }
}
function Set-UI4MEDocumentAttribute {
    <# .SYNOPSIS Sets or (-Remove) removes an attribute. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
          [Parameter(Mandatory)][string]$Name, [AllowEmptyString()][string]$Value = '', [switch]$Remove)
    if ($Remove) { $null = Invoke-UI4MEDocumentCommand $Document 'remove_attribute' @{ element = $Element; name = $Name } }
    else { $null = Invoke-UI4MEDocumentCommand $Document 'set_attribute' @{ element = $Element; name = $Name; value = $Value } }
}
function Set-UI4MEClass {
    <# .SYNOPSIS Adds a class to an element, or removes it with -Remove. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
          [Parameter(Mandatory)][string]$Name, [switch]$Remove)
    $op = if ($Remove) { 'remove_class' } else { 'add_class' }
    $null = Invoke-UI4MEDocumentCommand $Document $op @{ element = $Element; name = $Name }
}
function Set-UI4MEDocumentHtml {
    <# .SYNOPSIS Replaces (default) or inserts trusted HTML. Use Set-UI4MEDocumentText for untrusted text. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
          [Parameter(Mandatory)][AllowEmptyString()][string]$Html,
          [ValidateSet('replace', 'beforebegin', 'afterbegin', 'beforeend', 'afterend')][string]$Position = 'replace')
    if ($Position -eq 'replace') {
        $null = Invoke-UI4MEDocumentCommand $Document 'replace_children_html' @{ element = $Element; html = $Html }
    } else {
        $null = Invoke-UI4MEDocumentCommand $Document 'insert_html' @{ element = $Element; html = $Html; position = $Position }
    }
}
function Remove-UI4MEElement {
    <# .SYNOPSIS Removes an element (and its subtree) by id. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element)
    $null = Invoke-UI4MEDocumentCommand $Document 'remove_element' @{ element = $Element }
}
function Find-UI4MEElement {
    <# .SYNOPSIS Runs a CSS selector query; returns paginated element ids. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Selector,
        [ValidateRange(0, 2147483647)][int]$Offset = 0, [ValidateRange(1, 4096)][int]$Limit = 1024)
    return (Invoke-UI4MEDocumentCommand $Document 'query' @{ selector = $Selector; offset = $Offset; limit = $Limit })
}
function Add-UI4MEStylesheet {
    <# .SYNOPSIS Adds a stylesheet; returns its sheet handle for Set-UI4MEStylesheet. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][AllowEmptyString()][string]$Css)
    return (Invoke-UI4MEDocumentCommand $Document 'add_stylesheet' @{ css = $Css })
}
function Get-UI4MEStylesheets {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document)
    return ,@(Get-UI4MEField (Invoke-UI4MEDocumentCommand $Document 'list_stylesheets') 'sheets' @())
}
function Set-UI4MEStylesheet {
    <# .SYNOPSIS Replaces the CSS of a sheet, or removes it with -Remove. #>
    [CmdletBinding(DefaultParameterSetName = 'Replace')]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][long]$Sheet,
        [Parameter(Mandatory, ParameterSetName = 'Replace')][AllowEmptyString()][string]$Css,
        [Parameter(Mandatory, ParameterSetName = 'Remove')][switch]$Remove)
    if ($Remove) { $null = Invoke-UI4MEDocumentCommand $Document 'remove_stylesheet' @{ sheet = $Sheet }; return }
    return (Invoke-UI4MEDocumentCommand $Document 'replace_stylesheet' @{ sheet = $Sheet; css = $Css })
}
function Get-UI4MEComputedStyle {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string[]]$Properties,
        [string]$Element, $Node)
    $p = @{ properties = $Properties }
    Add-UI4METarget $p $Document $Element $Node
    return (Invoke-UI4MEDocumentCommand $Document 'get_computed_style' $p)
}
function Get-UI4MEEnvironment {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document)
    return (Invoke-UI4MEDocumentCommand $Document 'get_environment')
}
function Set-UI4MEEnvironment {
    <# .SYNOPSIS Overrides prefers-color-scheme / prefers-reduced-motion for this document. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$ColorScheme, [string]$ReducedMotion)
    $p = @{}
    if ($PSBoundParameters.ContainsKey('ColorScheme')) { $p.color_scheme = $ColorScheme }
    if ($PSBoundParameters.ContainsKey('ReducedMotion')) { $p.reduced_motion = $ReducedMotion }
    return (Invoke-UI4MEDocumentCommand $Document 'set_environment' $p)
}
function Get-UI4MEScroll {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Element)
    $p = @{}
    if ($Element) { $p.element = $Element }
    return (Invoke-UI4MEDocumentCommand $Document 'get_scroll' $p)
}
function Set-UI4MEScroll {
    <# .SYNOPSIS Scrolls the document or an element to X/Y (CSS px). #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Element, [double]$X, [double]$Y,
        [ValidateSet('auto', 'instant', 'smooth')][string]$Behavior = 'auto')
    $p = @{ behavior = $Behavior }
    if ($Element) { $p.element = $Element }
    if ($PSBoundParameters.ContainsKey('X')) { $p.x = $X }
    if ($PSBoundParameters.ContainsKey('Y')) { $p.y = $Y }
    return (Invoke-UI4MEDocumentCommand $Document 'scroll_to' $p)
}
function Get-UI4MEFocusedElement {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document)
    return (Invoke-UI4MEDocumentCommand $Document 'get_focused_element')
}
function Set-UI4MEFocus {
    <# .SYNOPSIS Focuses an element or node, or with -Blur removes focus. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Element, $Node, [switch]$Blur)
    $p = @{}
    Add-UI4METarget $p $Document $Element $Node -Required:(-not $Blur)
    $op = if ($Blur) { 'blur' } else { 'focus' }
    $null = Invoke-UI4MEDocumentCommand $Document $op $p
}
function Get-UI4MESelection {
    <# .SYNOPSIS Returns a text control's selection (UTF-16 offsets). #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Element, $Node)
    $p = @{}
    Add-UI4METarget $p $Document $Element $Node -Required
    return (Invoke-UI4MEDocumentCommand $Document 'get_control_selection' $p)
}
function Set-UI4MESelection {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Element, $Node,
        [Parameter(Mandatory)][ValidateRange(0, 2147483647)][int]$Start, [Parameter(Mandatory)][ValidateRange(0, 2147483647)][int]$End,
        [ValidateSet('none', 'forward', 'backward')][string]$Direction = 'none')
    $p = @{ start = $Start; end = $End; direction = $Direction }
    Add-UI4METarget $p $Document $Element $Node -Required
    return (Invoke-UI4MEDocumentCommand $Document 'set_control_selection' $p)
}
function Invoke-UI4MEEditCommand {
    <#
    .SYNOPSIS
    Runs an editing command (undo, redo, cut, copy, paste, selectAll, delete, ...) through the native editor.
    .DESCRIPTION
    Clipboard commands complete asynchronously; observe input/error events. Acceptance does not
    mean the OS transfer succeeded.
    #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Command,
        [string]$Element, $Node)
    $p = @{ command = $Command }
    Add-UI4METarget $p $Document $Element $Node
    return (Invoke-UI4MEDocumentCommand $Document 'edit_command' $p)
}
function Get-UI4MEFormData {
    <# .SYNOPSIS Returns ordered form entries (repeated names and file metadata preserved). #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Form, $FormNode, [string]$Submitter, $SubmitterNode)
    $p = @{}
    Add-UI4METarget $p $Document $Form $FormNode -Required
    Add-UI4METarget $p $Document $Submitter $SubmitterNode -ElementKey 'submitter' -NodeKey 'submitter_node'
    return (Invoke-UI4MEDocumentCommand $Document 'get_form_data' $p)
}
function Submit-UI4MEForm {
    <# .SYNOPSIS Requests form submission (validation, then a submit event). #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Form, $FormNode, [string]$Submitter, $SubmitterNode)
    $p = @{}
    Add-UI4METarget $p $Document $Form $FormNode -Required
    Add-UI4METarget $p $Document $Submitter $SubmitterNode -ElementKey 'submitter' -NodeKey 'submitter_node'
    return (Invoke-UI4MEDocumentCommand $Document 'request_submit' $p)
}
function Reset-UI4MEForm {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Form, $FormNode)
    $p = @{}
    Add-UI4METarget $p $Document $Form $FormNode -Required
    return (Invoke-UI4MEDocumentCommand $Document 'reset_form' $p)
}
function Get-UI4MESelectOptions {
    <# .SYNOPSIS Returns a select element's options (value, label, selected, disabled). #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element)
    return ,@(Get-UI4MEField (Invoke-UI4MEDocumentCommand $Document 'get_options' @{ element = $Element }) 'options' @())
}
function Set-UI4MESelectedValues {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
        [Parameter(Mandatory)][AllowEmptyCollection()][string[]]$Values)
    $null = Invoke-UI4MEDocumentCommand $Document 'set_selected_values' @{ element = $Element; values = $Values }
}
function Show-UI4MEPopover {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
        [ValidateSet('show', 'hide', 'toggle')][string]$Action = 'show')
    $op = switch ($Action) { 'show' { 'show_popover' } 'hide' { 'hide_popover' } 'toggle' { 'toggle_popover' } }
    $res = Invoke-UI4MEDocumentCommand $Document $op @{ element = $Element }
    if ($Action -eq 'toggle') { return [bool](Get-UI4MEField $res 'open') }
}
function Show-UI4MEHtmlDialog {
    <# .SYNOPSIS Shows an HTML <dialog> (modal by default) or closes it with -Close. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
        [switch]$Close, [bool]$Modal = $true, [AllowEmptyString()][string]$ReturnValue = '')
    if ($Close) { $null = Invoke-UI4MEDocumentCommand $Document 'close_dialog' @{ element = $Element; return_value = $ReturnValue }; return }
    $null = Invoke-UI4MEDocumentCommand $Document 'show_dialog' @{ element = $Element; modal = $Modal }
}
function Set-UI4MEPreventDefault {
    <# .SYNOPSIS Registers, before input happens, whether the native default action of an event is prevented. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Element,
        [Parameter(Mandatory)][Alias('Event')][string]$EventType, [bool]$Prevent = $true, [string]$Key,
        [bool]$Shift, [bool]$Ctrl, [bool]$Alt, [bool]$Meta)
    $p = @{ element = $Element; event = $EventType; prevent = $Prevent }
    if ($PSBoundParameters.ContainsKey('Key')) { $p.key = $Key }
    foreach ($m in @('Shift', 'Ctrl', 'Alt', 'Meta')) {
        if ($PSBoundParameters.ContainsKey($m)) { $p[$m.ToLowerInvariant()] = $PSBoundParameters[$m] }
    }
    $null = Invoke-UI4MEDocumentCommand $Document 'set_default_action' $p
}
function Get-UI4MEDiagnostics {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document)
    return (Invoke-UI4MEDocumentCommand $Document 'get_diagnostics')
}
function Get-UI4MEResourceStats {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document)
    return (Invoke-UI4MEDocumentCommand $Document 'get_resource_stats')
}
function Clear-UI4MEUnusedResources {
    [CmdletBinding()] param([Parameter(Mandatory)]$Document, [string]$Url)
    $p = @{}
    if ($Url) { $p.url = $Url }
    return (Invoke-UI4MEDocumentCommand $Document 'purge_unused_resources' $p)
}

function ConvertTo-UI4MEBytes {
    # PowerShell unrolls byte[] returned from functions into object[]; accept that.
    param($Value, [string]$What)
    if ($Value -is [byte[]]) { return ,$Value }
    if ($Value -is [System.Collections.IList]) {
        try { return ,([byte[]]$Value) } catch { throw "$What must contain only byte values 0-255" }
    }
    throw "$What must be a byte[]"
}

function Register-UI4MEResources {
    <#
    .SYNOPSIS
    Atomically replaces one document-local ui4me-res://NAME/ bundle.
    .DESCRIPTION
    -Entries maps relative paths to byte[] or @{ Bytes = [byte[]]; Mime = 'image/png' }.
    At most 256 entries and 1 MiB decoded per registration.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Name,
          [Parameter(Mandatory)][System.Collections.IDictionary]$Entries)
    Assert-UI4MEDocument $Document
    if ($Entries.Count -gt 256) { throw 'A resource registration permits at most 256 entries' }
    $size = 0
    $items = New-Object System.Collections.ArrayList
    foreach ($key in $Entries.Keys) {
        $entry = $Entries[$key]
        $mime = 'application/octet-stream'
        if ($entry -is [System.Collections.IDictionary]) {
            $bytes = Get-UI4MEField $entry 'Bytes'; $mime = [string](Get-UI4MEField $entry 'Mime' $mime)
        } else { $bytes = $entry }
        $bytes = ConvertTo-UI4MEBytes $bytes "Resource '$key'"
        if ($mime -notmatch '^[^,\r\n]+$') { throw "Resource '$key' has an invalid MIME type" }
        $size += $bytes.Length
        if ($size -gt 1048576) { throw 'A resource registration permits at most 1 MiB of decoded bytes' }
        [void]$items.Add(@{ path = [string]$key; data_url = ('data:' + $mime + ';base64,' + [Convert]::ToBase64String($bytes)) })
    }
    return (Invoke-UI4MEDocumentCommand $Document 'register_resource_bundle' @{ name = $Name; entries = $items.ToArray() })
}
function Unregister-UI4MEResources {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Name)
    return (Invoke-UI4MEDocumentCommand $Document 'unregister_resource_bundle' @{ name = $Name })
}

# ------------------------------------------------------------------- nodes --

function Get-UI4MENode {
    <# .SYNOPSIS Resolves an element id (or the root) to a generation-bound node, or wraps a host reference. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [string]$Element, [string]$Reference)
    Assert-UI4MEDocument $Document
    if ($Element -and $Reference) { throw 'Use -Element or -Reference, not both' }
    if ($Reference) { $ref = $Reference } else {
        $p = @{}
        if ($Element) { $p.element = $Element }
        $ref = [string](Get-UI4MEField (Invoke-UI4MEDocumentCommand $Document 'node_of' $p) 'node')
    }
    if (-not $ref) { throw 'The host did not return a node reference' }
    $node = [pscustomobject]@{ Document = $Document; Id = $ref }
    $node.PSObject.TypeNames.Insert(0, 'UI4ME.Node')
    return $node
}
function Get-UI4MENodes {
    <# .SYNOPSIS Paginated node references for a selector, including elements without ids. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Selector,
        [ValidateRange(0, 2147483647)][int]$Offset = 0, [ValidateRange(1, 4096)][int]$Limit = 1024)
    return (Invoke-UI4MEDocumentCommand $Document 'query_nodes' @{ selector = $Selector; offset = $Offset; limit = $Limit })
}
function Get-UI4MENodeInfo {
    [CmdletBinding()] param([Parameter(Mandatory)]$Node)
    Assert-UI4MENode $Node
    return (Invoke-UI4MEDocumentCommand $Node.Document 'get_node' @{ node = $Node.Id })
}
function Get-UI4MENodeChildren {
    [CmdletBinding()] param([Parameter(Mandatory)]$Node,
        [ValidateRange(0, 2147483647)][int]$Offset = 0, [ValidateRange(1, 4096)][int]$Limit = 256)
    Assert-UI4MENode $Node
    return (Invoke-UI4MEDocumentCommand $Node.Document 'node_children' @{ node = $Node.Id; offset = $Offset; limit = $Limit })
}
function Invoke-UI4MENodeBatch {
    <# .SYNOPSIS Applies node-addressed operations (@{op=...;node=...}) as one atomic transaction. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][object[]]$Operations)
    $null = Invoke-UI4MEDocumentCommand -Document $Document -Operation 'node_batch' -Parameters @{ ops = $Operations }
}
function Invoke-UI4MENodeOperation {
    <#
    .SYNOPSIS
    Applies one node operation: set_text, append_text, set_attribute, remove_attribute, add_class,
    remove_class, insert_html, replace_children_html, trim_children, move_element, remove_element.
    #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Node, [Parameter(Mandatory)][string]$Operation,
        [System.Collections.IDictionary]$Parameters = @{})
    Assert-UI4MENode $Node
    $p = @{ op = $Operation; node = $Node.Id }
    foreach ($key in $Parameters.Keys) {
        if ($key -in @('op', 'node')) { throw "Reserved node operation key: $key" }
        $value = $Parameters[$key]
        if (Test-UI4METype $value 'UI4ME.Node') {
            Assert-UI4MENode $value
            if (-not [object]::ReferenceEquals($value.Document, $Node.Document)) { throw 'Cannot move nodes between documents' }
            $value = $value.Id
        }
        $p[[string]$key] = $value
    }
    $null = Invoke-UI4MEDocumentCommand $Node.Document 'node_batch' @{ ops = @($p) }
}

# ----------------------------------------------------------- desktop services --

function Get-UI4MECapabilities {
    [CmdletBinding()] param([Parameter(Mandatory)]$Application)
    return (Invoke-UI4MERpc -Application $Application -Method 'host.capabilities')
}
function Get-UI4MEPreferences {
    <# .SYNOPSIS Observed desktop preferences (color scheme, contrast, motion, ...) and their source. #>
    [CmdletBinding()] param([Parameter(Mandatory)]$Application)
    return (Invoke-UI4MERpc -Application $Application -Method 'desktop.preferences')
}
function Get-UI4MEClipboard {
    [CmdletBinding()] param([Parameter(Mandatory)]$Application)
    return [string](Get-UI4MEField (Invoke-UI4MERpc -Application $Application -Method 'clipboard.read_text') 'text')
}
function Set-UI4MEClipboard {
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][AllowEmptyString()][string]$Text)
    $null = Invoke-UI4MERpc -Application $Application -Method 'clipboard.write_text' -Parameters @{ text = $Text }
}
function Open-UI4MEExternal {
    <#
    .SYNOPSIS
    Opens a URL or document with the desktop's default handler, or with -Reveal shows a file in the file manager.
    .DESCRIPTION
    Host policy refuses executables, scripts and shortcuts as open targets.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Application, [Parameter(Mandatory)][string]$Target, $Window, [switch]$Reveal)
    Assert-UI4MEApplication $Application
    $p = @{}
    if ($Reveal) { $p.path = Resolve-UI4MEPath $Target; $method = 'desktop.reveal_file' }
    else { $p.target = $Target; $method = 'desktop.open_external' }
    if ($null -ne $Window) {
        Assert-UI4MEWindow $Window
        if (-not [object]::ReferenceEquals($Window.Application, $Application)) { throw 'The window belongs to another application' }
        $p.window_id = $Window.Id
    }
    return (Invoke-UI4MERpc -Application $Application -Method $method -Parameters $p -TimeoutMilliseconds 60000)
}
function Show-UI4MEFileDialog {
    <#
    .SYNOPSIS
    Shows a native open/save/folder dialog. Returns the host result (paths), waiting without a deadline.
    .DESCRIPTION
    -NoWait returns a UI4ME.Request; Wait-UI4MERpc awaits it and Stop-UI4MERpc closes the dialog.
    -Filters takes @(@{name='Images';extensions=@('png','jpg')}).
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Window,
        [ValidateSet('OpenFiles', 'SaveFile', 'ChooseFolder')][string]$Kind = 'OpenFiles',
        [string]$Title, [string]$Directory, [string]$FileName, [object[]]$Filters = @(), [bool]$Multiple = $false,
        [switch]$NoWait)
    Assert-UI4MEWindow $Window
    $method = switch ($Kind) { 'OpenFiles' { 'dialog.open_files' } 'SaveFile' { 'dialog.save_file' } 'ChooseFolder' { 'dialog.choose_folder' } }
    $p = @{ window_id = $Window.Id }
    if ($PSBoundParameters.ContainsKey('Title')) { $p.title = $Title }
    if ($PSBoundParameters.ContainsKey('Directory')) { $p.directory = Resolve-UI4MEPath $Directory }
    if ($PSBoundParameters.ContainsKey('FileName')) {
        if ($Kind -ne 'SaveFile') { throw '-FileName applies to -Kind SaveFile' }
        $p.file_name = $FileName
    }
    if ($Kind -eq 'OpenFiles') { $p.multiple = $Multiple }
    if ($Kind -ne 'ChooseFolder') { $p.filters = $Filters }
    if ($NoWait) { return (Start-UI4MERpc -Application $Window.Application -Method $method -Parameters $p) }
    return (Invoke-UI4MERpc -Application $Window.Application -Method $method -Parameters $p -TimeoutMilliseconds -1)
}

# ------------------------------------------------------------ live bitmaps --

function New-UI4MELiveBitmap {
    <#
    .SYNOPSIS
    Creates a bounded RGBA8 live image for <img src="ui4me-bitmap://ID">, shared through a host-created file mapping.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)]$Document, [Parameter(Mandatory)][string]$Id,
        [Parameter(Mandatory)][ValidateRange(1, 4096)][int]$Width, [Parameter(Mandatory)][ValidateRange(1, 4096)][int]$Height,
        [ValidateSet(2, 3)][int]$Slots = 2)
    Assert-UI4MEDocument $Document
    if ($Id -cnotmatch '^[A-Za-z0-9_-]{1,64}$') { throw 'Bitmap ID must be 1-64 ASCII letters, digits, _ or -' }
    $app = $Document.Window.Application
    $ids = @{ window_id = $Document.Window.Id; document_id = $Document.Id; bitmap_id = $Id }
    $p = @{ window_id = $Document.Window.Id; document_id = $Document.Id; bitmap_id = $Id
        width = $Width; height = $Height; slots = $Slots; pixel_format = 'RGBA8' }
    $res = Invoke-UI4MERpc -Application $app -Method 'bitmap.create' -Parameters $p
    $bytes = [long]$Width * [long]$Height * 4
    $stride = [long](Get-UI4MEField $res 'stride' 0)
    $slotBytes = [long](Get-UI4MEField $res 'slot_bytes' 0)
    $slotCount = [int](Get-UI4MEField $res 'slots' 0)
    $mapping = $null
    try {
        if ($stride -ne [long]$Width * 4 -or $slotBytes -ne $bytes -or $slotCount -ne $Slots) {
            throw 'The host returned invalid bitmap geometry'
        }
        $mapping = New-Object -TypeName 'UI4ME.MappedBitmap' -ArgumentList @(
            [string](Get-UI4MEField $res 'path'), [long]($bytes * $Slots), [bool]$script:UI4MEIsWindows)
    } catch {
        # A successful bitmap.create owns host resources even if mapping fails.
        try { $null = Invoke-UI4MERpc -Application $app -Method 'bitmap.close' -Parameters $ids } catch { $null = $_ }  # best effort; the original error is rethrown
        throw
    }
    $bitmap = [pscustomobject]@{ Document = $Document; Id = $Id; Width = $Width; Height = $Height
        Stride = [int]$stride; SlotBytes = $slotBytes; Slots = $Slots; Sequence = [long]0
        Closed = $false; Busy = $false; Mapping = $mapping }
    $bitmap.PSObject.TypeNames.Insert(0, 'UI4ME.LiveBitmap')
    [void]$Document.Bitmaps.Add($bitmap)
    return $bitmap
}

function Send-UI4MELiveBitmap {
    <#
    .SYNOPSIS
    Writes one complete straight-alpha RGBA8 frame into the next slot and presents it.
    .DESCRIPTION
    Returns after the host has snapshotted the slot (synchronous backpressure). With -Try,
    returns $null instead of re-entering while this bitmap's previous present is in progress
    (for example from a callback that runs during that present).
    #>
    [CmdletBinding()]
    # -Pixels is deliberately untyped: Windows PowerShell 5.1 inspects every element when a
    # mandatory parameter is typed [byte[]] (about 150 ms for a 1024x768 frame).
    param([Parameter(Mandatory)]$Bitmap, [Parameter(Mandatory)]$Pixels, [switch]$Try)
    if (-not (Test-UI4METype $Bitmap 'UI4ME.LiveBitmap')) { throw 'Expected a UI4ME.LiveBitmap from New-UI4MELiveBitmap' }
    if ($Pixels -isnot [byte[]]) { $Pixels = ConvertTo-UI4MEBytes $Pixels 'Pixels' }
    if ($Pixels.LongLength -ne $Bitmap.SlotBytes) { throw "Expected $($Bitmap.SlotBytes) RGBA8 bytes, received $($Pixels.LongLength)" }
    if ($Bitmap.Busy) {
        if ($Try) { return $null }
        throw 'A present of this bitmap is already in progress (use -Try from callbacks)'
    }
    if ($Bitmap.Closed) { throw (New-UI4MEStaleError "Live bitmap $($Bitmap.Id) is closed") }
    Assert-UI4MEDocument $Bitmap.Document
    $Bitmap.Busy = $true
    try {
        $Bitmap.Sequence++
        $slot = [int]($Bitmap.Sequence % $Bitmap.Slots)
        $Bitmap.Mapping.Write([long]$slot * $Bitmap.SlotBytes, $Pixels)
        $doc = $Bitmap.Document
        return (Invoke-UI4MERpc -Application $doc.Window.Application -Method 'bitmap.present' -Parameters @{
            window_id = $doc.Window.Id; document_id = $doc.Id; bitmap_id = $Bitmap.Id; sequence = $Bitmap.Sequence; slot = $slot })
    } finally { $Bitmap.Busy = $false }
}

function Remove-UI4MELiveBitmap {
    <# .SYNOPSIS Unmaps and releases a live bitmap. Idempotent. -Quiet suppresses host errors. #>
    [CmdletBinding()]
    param([Parameter(Mandatory)][AllowNull()]$Bitmap, [switch]$Quiet)
    if ($null -eq $Bitmap -or $Bitmap.Closed) { return }
    $Bitmap.Closed = $true
    if ($null -ne $Bitmap.Mapping) { $Bitmap.Mapping.Dispose() }
    $doc = $Bitmap.Document
    [void]$doc.Bitmaps.Remove($Bitmap)
    $app = $doc.Window.Application
    # Closing a document or window releases its bitmaps in the host already.
    if ($doc.Retired -or $doc.Window.Closed -or $app.Closed -or $app.Disconnected) { return }
    try {
        $null = Invoke-UI4MERpc -Application $app -Method 'bitmap.close' -Parameters @{
            window_id = $doc.Window.Id; document_id = $doc.Id; bitmap_id = $Bitmap.Id }
    } catch {
        $code = $null
        if ($_.Exception -is [UI4ME.RpcException]) { $code = $_.Exception.Code }
        $benign = ($_.Exception -is [UI4ME.DisconnectedException]) -or ($code -in @(-32004, -32008))
        if (-not $Quiet -and -not $benign) { throw }
    }
}

# --------------------------------------------------- host, projects, skills --

function Get-UI4MEHostPath {
    <# .SYNOPSIS Returns the bundled native host for this platform (or UI4ME_HOST_BINARY). #>
    [CmdletBinding()]
    param()
    if ($env:UI4ME_HOST_BINARY) {
        if (-not (Test-Path -LiteralPath $env:UI4ME_HOST_BINARY -PathType Leaf)) { throw "UI4ME_HOST_BINARY does not exist: $env:UI4ME_HOST_BINARY" }
        return [IO.Path]::GetFullPath($env:UI4ME_HOST_BINARY)
    }
    $arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture
    if ($arch -ne [Runtime.InteropServices.Architecture]::X64) { throw "UI4ME.Core 1.0.1 ships x64 hosts only; this system is $arch" }
    if ($script:UI4MEIsWindows) { $relative = 'runtimes/win-x64/ui4me_core/_bin/ui4me-host.exe' }
    elseif ([Runtime.InteropServices.RuntimeInformation]::IsOSPlatform([Runtime.InteropServices.OSPlatform]::Linux)) {
        $relative = 'runtimes/linux-x64/ui4me_core/_bin/ui4me-host'
    } else { throw 'UI4ME.Core 1.0.1 supports Linux and Windows only' }
    $found = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot $relative))
    if (-not (Test-Path -LiteralPath $found -PathType Leaf)) {
        throw 'The native UI4ME host is missing. Install UI4ME.Core from the PowerShell Gallery, or set UI4ME_HOST_BINARY.'
    }
    if (-not $script:UI4MEIsWindows) {
        # NuGet package extraction drops Unix execute permissions.
        & /bin/sh -c 'test -x "$1" || chmod u+x -- "$1"' ui4me-host-check $found
        if ($LASTEXITCODE -ne 0) { throw "The native host is not executable and could not be made executable: $found" }
    }
    return $found
}

function Start-UI4MEHost {
    <#
    .SYNOPSIS
    Runs an app manifest (app.toml) through the native host and waits for it to exit.
    .DESCRIPTION
    -NoWait starts the host and returns its process object.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Manifest, [switch]$NoWait)
    $hostBinary = Get-UI4MEHostPath
    $full = Resolve-UI4MEPath $Manifest
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) { throw "Manifest not found: $full" }
    if ($NoWait) {
        $quoted = '"' + $full.Replace('"', '\"') + '"'
        return (Start-Process -FilePath $hostBinary -ArgumentList @('run', '--app', $quoted) -PassThru)
    }
    & $hostBinary run --app $full
    if ($LASTEXITCODE -ne 0) { throw "UI4ME host exited with code $LASTEXITCODE" }
}

function ConvertTo-UI4METomlString {
    # A TOML basic string: escape quotes, backslashes and control characters; keep Unicode literal.
    param([AllowEmptyString()][string]$Text)
    if ($Text.IndexOf([char]0) -ge 0) { throw 'Paths and arguments must not contain NUL characters' }
    $builder = New-Object Text.StringBuilder
    [void]$builder.Append('"')
    foreach ($c in $Text.ToCharArray()) {
        if ($c -eq '"') { [void]$builder.Append('\"') }
        elseif ($c -eq '\') { [void]$builder.Append('\\') }
        elseif ([int]$c -lt 32 -or [int]$c -eq 127) { [void]$builder.Append(('\u{0:X4}' -f [int]$c)) }
        else { [void]$builder.Append($c) }
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function Get-UI4MEDefaultAppId {
    # Stable per script: a second launch of the same script reaches the running instance.
    param([string]$ScriptPath, [string]$Label)
    $identity = if ($script:UI4MEIsWindows) { $ScriptPath.ToLowerInvariant() } else { $ScriptPath }
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $bytes = $sha.ComputeHash([Text.Encoding]::UTF8.GetBytes('script:' + $identity)) } finally { $sha.Dispose() }
    $digest = (-join ($bytes | ForEach-Object { $_.ToString('x2') })).Substring(0, 12)
    $folded = $Label.Normalize([Text.NormalizationForm]::FormD)
    $folded = -join ($folded.ToCharArray() | Where-Object { [int]$_ -lt 128 })
    $slug = ($folded -replace '[^A-Za-z0-9]+', '-').Trim('-')
    if ($slug.Length -gt 40) { $slug = $slug.Substring(0, 40).Trim('-') }
    if (-not $slug) { $slug = 'app' } elseif ($slug -notmatch '^[A-Za-z]') { $slug = 'app-' + $slug }
    return "local.ui4me.$slug.h$digest"
}

function Get-UI4MEScriptArguments {
    # Arguments that followed the script on this process's command line
    # (pwsh -File app.ps1 a b). A script started from an open prompt has none here.
    param([string]$ScriptPath)
    $all = [Environment]::GetCommandLineArgs()
    $comparison = if ($script:UI4MEIsWindows) { [StringComparison]::OrdinalIgnoreCase } else { [StringComparison]::Ordinal }
    for ($i = 1; $i -lt $all.Count; $i++) {
        try { $full = [IO.Path]::GetFullPath([IO.Path]::Combine([Environment]::CurrentDirectory, $all[$i])) } catch { continue }
        if ([string]::Equals($full, $ScriptPath, $comparison)) {
            if ($i + 1 -ge $all.Count) { return ,@() }
            return ,@($all[($i + 1)..($all.Count - 1)])
        }
    }
    return ,@()
}

function Clear-UI4MEStaleStartFolders {
    # Manifests left behind by a launcher that was killed outright. The host
    # reads its manifest only while starting, so a day-old one is never in use.
    $limit = [DateTime]::UtcNow.AddDays(-1)
    try { $folders = @(Get-ChildItem -LiteralPath ([IO.Path]::GetTempPath()) -Directory -Filter 'ui4me-start-*' -ErrorAction Stop) } catch { return }
    foreach ($folder in $folders) {
        if ($folder.LastWriteTimeUtc -lt $limit -and ([int]$folder.Attributes -band [int][IO.FileAttributes]::ReparsePoint) -eq 0) {
            Remove-Item -LiteralPath $folder.FullName -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}

function Start-UI4MEScript {
    <#
    .SYNOPSIS
    Runs a normal .ps1 file as a UI4ME app: starts the native host when needed, returns the connected application.
    .DESCRIPTION
    Call it near the top of a script that you run directly (pwsh -File ./app.ps1, or
    powershell.exe -File .\app.ps1 on Windows PowerShell 5.1):

        $app = Start-UI4MEScript -Name 'My App'

    Run directly, it writes a private temporary manifest, starts the packaged native host and
    waits. The host runs the same script again, with the same PowerShell edition and arguments,
    as its supervised backend; the first run then ends with the host's exit code, so the code
    below this call never runs there. In the second run, Start-UI4MEScript connects to the
    host and returns the application object (like Connect-UI4ME).

    Everything above this call runs twice: keep side effects below it. Inside a backend started
    by a manifest (Start-UI4MEHost, ui4me-host run --app) it only connects.
    .PARAMETER Name
    Application name (default: the script file name).
    .PARAMETER AppId
    Reverse-DNS identity such as org.example.MyApp. Default: a stable ID derived from the script path.
    .PARAMETER Instance
    single (default) forwards a second launch to the running app; multiple allows several.
    .PARAMETER ArgumentList
    Arguments for the backend run. By default, the arguments that followed the script on the
    pwsh/powershell.exe command line are reused.
    .EXAMPLE
    $app = Start-UI4MEScript -Name 'Hello UI4ME'
    #>
    [CmdletBinding()]
    param(
        [string]$Name, [string]$AppId, [string]$Version, [string]$Icon,
        [ValidateSet('single', 'multiple')][string]$Instance = 'single',
        [ValidateRange(1000, 300000)][int]$StartupTimeoutMilliseconds = 30000,
        [ValidateRange(100, 120000)][int]$ShutdownTimeoutMilliseconds = 5000,
        [ValidateRange(1, 64)][int]$MaxWindows = 16,
        [string[]]$ArgumentList,
        [string]$ScriptPath
    )
    foreach ($pair in @(@('Name', $Name), @('Version', $Version))) {
        if ($pair[1] -and ($pair[1].Trim().Length -eq 0 -or $pair[1].Length -gt 128 -or $pair[1] -match '[\x00-\x1f\x7f]')) {
            throw "$($pair[0]) must be 1-128 characters without control characters"
        }
    }
    if ($AppId -and ($AppId.Length -gt 128 -or $AppId -cnotmatch '^[A-Za-z][A-Za-z0-9_-]*(\.[A-Za-z][A-Za-z0-9_-]*)+$')) {
        throw 'AppId must be a dot-separated ASCII identity such as org.example.MyApp'
    }
    $endpoint = $env:UI4ME_ENDPOINT
    $token = $env:UI4ME_TOKEN
    if ($endpoint -and $token) {
        # Backend role: started by the native host.
        if ($script:UI4MEIsWindows) { try { [Console]::OutputEncoding = New-Object Text.UTF8Encoding($false) } catch { } }
        $app = Connect-UI4ME -Endpoint $endpoint -Token $token
        $app.DirectStart = $true
        # Keep the connection secret out of processes this script starts.
        foreach ($variable in @('UI4ME_ENDPOINT', 'UI4ME_TOKEN', 'UI4ME_APP_ID', 'UI4ME_PROTOCOL_VERSION', 'UI4ME_DIRECT_START')) {
            [Environment]::SetEnvironmentVariable($variable, $null)
        }
        return $app
    }
    if ($endpoint -or $token) {
        throw 'Incomplete UI4ME host environment: UI4ME_ENDPOINT and UI4ME_TOKEN must both be set by the native host. Remove them to run the script directly.'
    }
    if ($env:UI4ME_DIRECT_START) {
        throw 'This backend was started by a UI4ME launcher but received no host connection. Refusing to start a second native host.'
    }

    # Launcher role.
    if (-not $ScriptPath) { $ScriptPath = $MyInvocation.PSCommandPath }
    if (-not $ScriptPath) { throw 'Start-UI4MEScript must be called from a .ps1 script file (for example pwsh -File ./app.ps1).' }
    $ScriptPath = [IO.Path]::GetFullPath((Resolve-UI4MEPath $ScriptPath))
    if (-not (Test-Path -LiteralPath $ScriptPath -PathType Leaf)) { throw "Script not found: $ScriptPath" }
    if ($Icon) {
        $Icon = [IO.Path]::GetFullPath((Resolve-UI4MEPath $Icon))
        if (-not (Test-Path -LiteralPath $Icon -PathType Leaf)) { throw "Application icon not found: $Icon" }
    }
    if (-not $PSBoundParameters.ContainsKey('ArgumentList')) { $ArgumentList = Get-UI4MEScriptArguments $ScriptPath }
    if ($null -eq $ArgumentList) { $ArgumentList = @() }
    $label = [IO.Path]::GetFileNameWithoutExtension($ScriptPath)
    if (-not $Name) { $Name = $label }
    if (-not $AppId) { $AppId = Get-UI4MEDefaultAppId -ScriptPath $ScriptPath -Label $Name }
    $edition = if ($PSVersionTable.PSEdition -eq 'Core') { if ($script:UI4MEIsWindows) { 'pwsh.exe' } else { 'pwsh' } } else { 'powershell.exe' }
    $shell = Join-Path $PSHOME $edition
    if (-not (Test-Path -LiteralPath $shell -PathType Leaf)) { throw "Cannot find this PowerShell executable: $shell" }
    $hostBinary = Get-UI4MEHostPath
    Initialize-UI4MEHelper

    $backendArguments = @('-NoLogo', '-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $ScriptPath) + @($ArgumentList)
    $lines = New-Object Collections.Generic.List[string]
    $lines.Add('[app]'); $lines.Add('id = ' + (ConvertTo-UI4METomlString $AppId)); $lines.Add('name = ' + (ConvertTo-UI4METomlString $Name))
    if ($Version) { $lines.Add('version = ' + (ConvertTo-UI4METomlString $Version)) }
    if ($Icon) { $lines.Add('icon = ' + (ConvertTo-UI4METomlString $Icon)) }
    $lines.Add(''); $lines.Add('[backend]'); $lines.Add('program = ' + (ConvertTo-UI4METomlString $shell))
    $lines.Add('args = [' + ((@($backendArguments) | ForEach-Object { ConvertTo-UI4METomlString $_ }) -join ', ') + ']')
    $lines.Add('cwd = ' + (ConvertTo-UI4METomlString ([IO.Path]::GetFullPath((Get-Location -PSProvider FileSystem).ProviderPath))))
    $lines.Add(''); $lines.Add('[host]')
    $lines.Add("startup_timeout_ms = $StartupTimeoutMilliseconds"); $lines.Add("shutdown_timeout_ms = $ShutdownTimeoutMilliseconds")
    $lines.Add("max_windows = $MaxWindows"); $lines.Add('instance = ' + (ConvertTo-UI4METomlString $Instance)); $lines.Add('')

    Clear-UI4MEStaleStartFolders
    $folder = Join-Path ([IO.Path]::GetTempPath()) ('ui4me-start-' + [guid]::NewGuid().ToString('N'))
    [void](New-Item -ItemType Directory -Path $folder)
    $runner = $null
    $code = 1
    $previousMarker = $env:UI4ME_DIRECT_START
    try {
        $manifest = Join-Path $folder 'app.toml'
        [IO.File]::WriteAllText($manifest, ($lines -join "`n"), (New-Object Text.UTF8Encoding($false)))
        $hostArguments = @('run', '--app', $manifest)
        if ($env:UI4ME_TEST_MODE -eq '1') { $hostArguments += '--headless' }
        $program = $hostBinary
        if (-not $script:UI4MEIsWindows) {
            # If this PowerShell process dies, even by SIGKILL, the host is told to stop.
            $setpriv = Get-Command setpriv -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($setpriv) { $program = $setpriv.Source; $hostArguments = @('--pdeathsig', 'TERM', '--', $hostBinary) + $hostArguments }
        }
        $verbose = @('', '0', 'false', 'no') -notcontains ([string]$env:UI4ME_VERBOSE).Trim().ToLowerInvariant()
        $env:UI4ME_DIRECT_START = [string]$PID
        try {
            $runner = New-Object UI4ME.HostProcess -ArgumentList @($program, [string[]]$hostArguments, [Environment]::CurrentDirectory, $verbose)
        } finally {
            $env:UI4ME_DIRECT_START = $previousMarker
        }
        # Short waits keep Ctrl+C responsive; the finally block stops the host.
        while (-not $runner.WaitForExit(200)) { }
        $code = $runner.ExitCode
    } finally {
        if ($null -ne $runner) {
            if (-not $runner.HasExited) { $runner.Stop() }
            $runner.Dispose()
        }
        Remove-Item -LiteralPath $folder -Recurse -Force -ErrorAction SilentlyContinue
    }
    exit $code
}

function New-UI4MEProject {
    <#
    .SYNOPSIS
    Generates a runnable HTML + CSS + PowerShell app (app.ps1, index.html, style.css, optional app.toml).
    .DESCRIPTION
    Run the generated app directly: pwsh -File ./app.ps1 (or powershell.exe -File .\app.ps1 on
    Windows PowerShell 5.1). app.toml is optional, for launchers and deployments that start the
    host explicitly with Start-UI4MEHost; it names pwsh, or powershell.exe with -WindowsPowerShell.
    #>
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Path, [string]$Name = 'UI4ME PowerShell App',
          [string]$AppId = 'org.example.PowerShellApp', [Alias('WindowsPowerShell51')][switch]$WindowsPowerShell, [switch]$Force)
    if ($AppId -cnotmatch '^[A-Za-z][A-Za-z0-9_-]*(\.[A-Za-z][A-Za-z0-9_-]*)+$' -or $AppId.Length -gt 128) {
        throw 'AppId must be a reverse-domain ASCII identifier such as org.example.MyApp'
    }
    if ([string]::IsNullOrWhiteSpace($Name) -or $Name.Length -gt 128 -or $Name -match '[\x00-\x1f"\\]') {
        throw 'Name must be 1-128 characters without quotes, backslashes or control characters'
    }
    $folder = Resolve-UI4MEPath $Path
    $files = @('app.ps1', 'app.toml', 'index.html', 'style.css')
    if (Test-Path -LiteralPath $folder -PathType Leaf) { throw "Path is a file: $folder" }
    if (-not $Force) {
        foreach ($f in $files) { if (Test-Path -LiteralPath (Join-Path $folder $f)) { throw "Refusing to overwrite $f in $folder; use -Force" } }
    }
    [void](New-Item -ItemType Directory -Path $folder -Force)
    $program = if ($WindowsPowerShell) { 'powershell.exe' } else { 'pwsh' }
    $htmlName = [Net.WebUtility]::HtmlEncode($Name)
    $psName = $Name.Replace("'", "''")
    $manifest = @"
# Optional. For daily development just run: pwsh -File ./app.ps1
# This manifest lets a launcher or deployment start the app explicitly with:
#   Start-UI4MEHost -Manifest ./app.toml
[app]
id = "$AppId"
name = "$Name"
version = "1.0.0"

[backend]
program = "$program"
args = ["-NoLogo", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", "{app_dir}/app.ps1"]
cwd = "{app_dir}"

[host]
startup_timeout_ms = 30000
shutdown_timeout_ms = 5000
max_windows = 8
instance = "single"
"@
    $backend = @"
# $psName - a native desktop window built with HTML, CSS and PowerShell.
#
# Run it:  pwsh -File ./app.ps1
#     or:  powershell.exe -NoProfile -File .\app.ps1   (Windows PowerShell 5.1)
#
# UI4ME starts its native window host, which runs this script a second time to
# connect it to the window. Keep start-up work below Start-UI4MEScript.
`$ErrorActionPreference = 'Stop'
Import-Module UI4ME.Core -MinimumVersion 1.0.1

`$app = Start-UI4MEScript -Name '$psName' -AppId '$AppId'

`$window = New-UI4MEWindow -Application `$app -Title '$psName' -Width 840 -Height 540
`$doc = Open-UI4MEFile -Window `$window -Path (Join-Path `$PSScriptRoot 'index.html')

`$script:clicks = 0
`$null = Register-UI4MEDocumentEvent -Document `$doc -Type click -Element greet -Callback {
    `$script:clicks++
    `$name = (Get-UI4MEValue -Document `$doc -Element name).Trim()
    if (-not `$name) { `$name = 'world' }
    # Text is never parsed as HTML, so user input is safe here.
    Set-UI4MEDocumentText -Document `$doc -Element result -Text "Hello, `$name! (`$(`$script:clicks))"
}

Set-UI4MEReady -Application `$app
Wait-UI4ME -Application `$app -UntilShutdown
Close-UI4ME -Application `$app
"@
    $html = @"
<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>$htmlName</title>
<link rel="stylesheet" href="style.css">
</head>
<body>
<main>
  <h1>$htmlName</h1>
  <p>Native HTML and CSS with PowerShell logic. No browser or JavaScript.</p>
  <label for="name">Your name</label>
  <div class="row"><input id="name" type="text" autocomplete="off"><button id="greet">Greet</button></div>
  <p id="result" aria-live="polite">Ready.</p>
</main>
</body>
</html>
"@
    $css = @'
:root { color-scheme: light dark; --bg: #f4f6fb; --card: #ffffff; --text: #17233a; --accent: #3268d8; }
@media (prefers-color-scheme: dark) { :root { --bg: #151a24; --card: #1f2633; --text: #e6ebf5; --accent: #6f9bff; } }
body { margin: 0; padding: 32px; font-family: system-ui, sans-serif; background: var(--bg); color: var(--text); }
main { max-width: 640px; margin: 0 auto; padding: 28px 32px; background: var(--card); border-radius: 14px; }
.row { display: flex; gap: 10px; margin-top: 6px; }
input { flex: 1; padding: 9px 11px; font-size: 15px; border-radius: 8px; border: 1px solid #8894a8; }
button { padding: 9px 18px; font-size: 15px; border: 0; border-radius: 8px; background: var(--accent); color: #fff; }
button:focus-visible, input:focus-visible { outline: 3px solid var(--accent); outline-offset: 2px; }
'@
    $utf8 = New-Object Text.UTF8Encoding($false)
    [IO.File]::WriteAllText((Join-Path $folder 'app.toml'), $manifest.Replace("`r`n", "`n"), $utf8)
    # Windows PowerShell 5.1 reads BOM-less scripts as the ANSI code page.
    [IO.File]::WriteAllText((Join-Path $folder 'app.ps1'), $backend, (New-Object Text.UTF8Encoding($true)))
    [IO.File]::WriteAllText((Join-Path $folder 'index.html'), $html.Replace("`r`n", "`n"), $utf8)
    [IO.File]::WriteAllText((Join-Path $folder 'style.css'), $css.Replace("`r`n", "`n"), $utf8)
    return (Get-Item -LiteralPath $folder)
}

function Get-UI4MESkillHashes {
    param([string]$Path)
    $hashes = @{}
    $root = [IO.Path]::GetFullPath($Path).TrimEnd([char]'\', [char]'/')
    foreach ($entry in @(Get-ChildItem -LiteralPath $root -Recurse -Force)) {
        if (([int]$entry.Attributes -band [int][IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Links are not supported in skill folders: $($entry.FullName)" }
        if ($entry.PSIsContainer -or $entry.Name -eq '.ui4me-managed.json') { continue }
        $relative = $entry.FullName.Substring($root.Length).TrimStart([char]'\', [char]'/').Replace('\', '/')
        $hashes[$relative] = (Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    if (-not $hashes.ContainsKey('SKILL.md')) { throw 'A skill must contain SKILL.md' }
    return $hashes
}
function Get-UI4MESkillState {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return 'missing' }
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer -or (([int]$item.Attributes -band [int][IO.FileAttributes]::ReparsePoint) -ne 0)) { return 'unmanaged' }
    $marker = Join-Path $Path '.ui4me-managed.json'
    if (-not (Test-Path -LiteralPath $marker -PathType Leaf)) { return 'unmanaged' }
    try {
        $record = ConvertFrom-Json -InputObject ([IO.File]::ReadAllText($marker, [Text.Encoding]::UTF8))
        if ($record.name -ne 'ui4me-powershell') { return 'unmanaged' }
        $actual = Get-UI4MESkillHashes $Path
        $known = @($record.files.PSObject.Properties)
        if ($actual.Count -ne $known.Count) { return 'modified' }
        foreach ($e in $known) { if (-not $actual.ContainsKey($e.Name) -or $actual[$e.Name] -ne $e.Value) { return 'modified' } }
        return 'installed'
    } catch { return 'modified' }
}
function Get-UI4MESkillSource {
    # A Gallery install bundles skills/ in the module; a source checkout keeps it at the repo root.
    foreach ($candidate in @((Join-Path $PSScriptRoot 'skills/ui4me-powershell'), (Join-Path $PSScriptRoot '../../skills/ui4me-powershell'))) {
        $full = [IO.Path]::GetFullPath($candidate)
        if (Test-Path -LiteralPath (Join-Path $full 'SKILL.md') -PathType Leaf) { return $full }
    }
    return $null
}

function Install-UI4MESkill {
    <#
    .SYNOPSIS
    Installs, inspects or removes the ui4me-powershell coding-agent skill for Claude Code, Codex,
    OpenCode, GitHub Copilot, Cursor and Gemini CLI.
    .DESCRIPTION
    User scope writes under your home directory; project scope under -ProjectPath. Folders that were
    not installed by this command, or were edited since, are never replaced or removed without -Force.
    .EXAMPLE
    Install-UI4MESkill -Agents all -Scope user
    .EXAMPLE
    Install-UI4MESkill -Action status -Agents claude,codex -Scope project -ProjectPath .
    #>
    [CmdletBinding()]
    param([ValidateSet('install', 'status', 'uninstall')][string]$Action = 'install',
      [string[]]$Agents = @('all'), [ValidateSet('user', 'project')][string]$Scope = 'user',
      [string]$ProjectPath, [switch]$Force, [switch]$DryRun)
    if ($Scope -eq 'project' -and -not $ProjectPath) { throw '-ProjectPath is required with -Scope project' }
    if ($Scope -eq 'user' -and $ProjectPath) { throw '-ProjectPath applies only to -Scope project' }
    $source = Get-UI4MESkillSource
    if ($Action -eq 'install' -and $null -eq $source) {
        throw 'The ui4me-powershell skill is missing from this module. Reinstall UI4ME.Core from the PowerShell Gallery.'
    }
    $supported = @('claude', 'codex', 'opencode', 'copilot', 'cursor', 'gemini')
    $tokens = @($Agents | ForEach-Object { $_.Split(',') } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Where-Object { $_ } | Select-Object -Unique)
    $targets = if ($tokens -contains 'all') { $supported } else { $tokens }
    foreach ($n in $targets) { if ($n -notin $supported) { throw "Unsupported agent '$n'. Use: $($supported -join ', '), all" } }
    $map = @{ claude = '.claude/skills'; codex = '.agents/skills'; opencode = '.config/opencode/skills'
        copilot = '.copilot/skills'; cursor = '.cursor/skills'; gemini = '.gemini/skills' }
    if ($Scope -eq 'project') {
        $base = Resolve-UI4MEPath $ProjectPath
        if (-not (Test-Path -LiteralPath $base -PathType Container)) { throw "-ProjectPath must be an existing folder: $base" }
        $map.opencode = '.opencode/skills'; $map.copilot = '.github/skills'
    } else {
        $base = [Environment]::GetFolderPath('UserProfile')
        if (-not $base) { $base = $HOME }
    }
    foreach ($agent in $targets) {
        $dest = [IO.Path]::GetFullPath((Join-Path (Join-Path $base $map[$agent]) 'ui4me-powershell'))
        # Never write through a link/junction in the destination path.
        $ancestor = $dest
        while ($ancestor) {
            if (Test-Path -LiteralPath $ancestor) {
                $f = Get-Item -LiteralPath $ancestor -Force
                if (([int]$f.Attributes -band [int][IO.FileAttributes]::ReparsePoint) -ne 0 -and $ancestor.Length -gt $base.TrimEnd([char]'\', [char]'/').Length) {
                    throw "Refusing to use a linked folder: $ancestor"
                }
            }
            $parent = Split-Path -Parent $ancestor
            if (-not $parent -or $parent -eq $ancestor) { break }
            $ancestor = $parent
        }
        $state = Get-UI4MESkillState $dest
        if ($Action -eq 'status' -or $DryRun) {
            [pscustomobject]@{ Agent = $agent; Scope = $Scope; State = $state; Path = $dest
                Action = $(if ($DryRun) { "would-$Action" } else { 'status' }) }
            continue
        }
        if ($Action -eq 'uninstall') {
            if ($state -eq 'missing') { [pscustomobject]@{ Agent = $agent; Scope = $Scope; State = 'missing'; Path = $dest; Action = 'none' }; continue }
            if ($state -ne 'installed' -and -not $Force) { throw "Refusing to remove $state skill folder $dest (use -Force)" }
            Remove-Item -LiteralPath $dest -Recurse -Force
            [pscustomobject]@{ Agent = $agent; Scope = $Scope; State = 'missing'; Path = $dest; Action = 'removed' }
            continue
        }
        if ($state -in @('modified', 'unmanaged') -and -not $Force) {
            throw "Refusing to overwrite $state skill folder $dest; inspect it or use -Force"
        }
        $parent = Split-Path -Parent $dest
        [void](New-Item -ItemType Directory -Path $parent -Force)
        $temp = Join-Path $parent ('.ui4me-skill-new-' + [guid]::NewGuid().ToString('N'))
        $backup = Join-Path $parent ('.ui4me-skill-old-' + [guid]::NewGuid().ToString('N'))
        try {
            Copy-Item -LiteralPath $source -Destination $temp -Recurse
            $files = Get-UI4MESkillHashes $temp
            $record = @{ name = 'ui4me-powershell'; version = $script:UI4MEVersion; files = $files } | ConvertTo-Json -Depth 5
            [IO.File]::WriteAllText((Join-Path $temp '.ui4me-managed.json'), $record, (New-Object Text.UTF8Encoding($false)))
            if (Test-Path -LiteralPath $dest) { Move-Item -LiteralPath $dest -Destination $backup }
            try { Move-Item -LiteralPath $temp -Destination $dest }
            catch { if (Test-Path -LiteralPath $backup) { Move-Item -LiteralPath $backup -Destination $dest }; throw }
            if (Test-Path -LiteralPath $backup) { Remove-Item -LiteralPath $backup -Recurse -Force }
            [pscustomobject]@{ Agent = $agent; Scope = $Scope; State = 'installed'; Path = $dest; Action = 'installed' }
        } finally {
            if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp -Recurse -Force }
        }
    }
}

Export-ModuleMember -Function @(
    'Connect-UI4ME', 'Close-UI4ME', 'Wait-UI4ME', 'Invoke-UI4MERpc', 'Send-UI4MENotification',
    'Start-UI4MERpc', 'Wait-UI4MERpc', 'Stop-UI4MERpc',
    'New-UI4MEWindow', 'Set-UI4MEWindow', 'Get-UI4MEWindow', 'Get-UI4MEWindowList', 'Close-UI4MEWindow',
    'Request-UI4MEAttention', 'Set-UI4MEBeforeClose', 'Set-UI4MEBeforeQuit', 'Set-UI4MEReady', 'Request-UI4MEQuit',
    'Register-UI4MENotification',
    'Open-UI4MEHtml', 'Open-UI4MEFile', 'Remove-UI4MEDocument',
    'Register-UI4MEDocumentEvent', 'Unregister-UI4MEDocumentEvent', 'Invoke-UI4MEDocumentCommand', 'Invoke-UI4MEBatch',
    'Get-UI4MEDocumentText', 'Set-UI4MEDocumentText', 'Get-UI4MEValue', 'Set-UI4MEDocumentValue',
    'Get-UI4MEChecked', 'Set-UI4MEChecked', 'Set-UI4MEDocumentAttribute', 'Set-UI4MEClass', 'Set-UI4MEDocumentHtml',
    'Remove-UI4MEElement', 'Find-UI4MEElement',
    'Add-UI4MEStylesheet', 'Get-UI4MEStylesheets', 'Set-UI4MEStylesheet', 'Get-UI4MEComputedStyle',
    'Get-UI4MEEnvironment', 'Set-UI4MEEnvironment', 'Get-UI4MEScroll', 'Set-UI4MEScroll',
    'Get-UI4MEFocusedElement', 'Set-UI4MEFocus', 'Get-UI4MESelection', 'Set-UI4MESelection', 'Invoke-UI4MEEditCommand',
    'Get-UI4MEFormData', 'Submit-UI4MEForm', 'Reset-UI4MEForm', 'Get-UI4MESelectOptions', 'Set-UI4MESelectedValues',
    'Show-UI4MEPopover', 'Show-UI4MEHtmlDialog', 'Set-UI4MEPreventDefault',
    'Get-UI4MEDiagnostics', 'Get-UI4MEResourceStats', 'Clear-UI4MEUnusedResources',
    'Register-UI4MEResources', 'Unregister-UI4MEResources',
    'Get-UI4MENode', 'Get-UI4MENodes', 'Get-UI4MENodeInfo', 'Get-UI4MENodeChildren', 'Invoke-UI4MENodeBatch', 'Invoke-UI4MENodeOperation',
    'Get-UI4MECapabilities', 'Get-UI4MEPreferences', 'Get-UI4MEClipboard', 'Set-UI4MEClipboard',
    'Open-UI4MEExternal', 'Show-UI4MEFileDialog',
    'New-UI4MELiveBitmap', 'Send-UI4MELiveBitmap', 'Remove-UI4MELiveBitmap',
    'Get-UI4MEHostPath', 'Start-UI4MEHost', 'Start-UI4MEScript', 'New-UI4MEProject', 'Install-UI4MESkill'
)
