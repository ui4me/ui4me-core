# Live bitmap: PowerShell writes raw RGBA8 frames into a host-created shared
# buffer. Frames are assembled with Buffer.BlockCopy from a precomputed
# gradient; per-pixel PowerShell loops are far too slow for animation.
$ErrorActionPreference = 'Stop'
try { Import-Module UI4ME.Core -MinimumVersion 1.0.0 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }
$app = Connect-UI4ME
$width = 320; $height = 200
try {
    $win = New-UI4MEWindow -Application $app -Title 'PowerShell Live Pixels' -Width 780 -Height 560
    $doc = Open-UI4MEFile -Window $win -Path (Join-Path $PSScriptRoot 'index.html')
    $bitmap = New-UI4MELiveBitmap -Document $doc -Id preview -Width $width -Height $height
    Set-UI4MEReady -Application $app

    # One 512-pixel colour wave, repeated so any 320-pixel window can be copied.
    $wave = New-Object byte[] (1024 * 4)
    for ($x = 0; $x -lt 1024; $x++) {
        $t = ($x % 512) / 512.0 * 2 * [Math]::PI
        $wave[$x * 4] = [byte](127 + 127 * [Math]::Sin($t))
        $wave[$x * 4 + 1] = [byte](127 + 127 * [Math]::Sin($t + 2.094))
        $wave[$x * 4 + 2] = [byte](127 + 127 * [Math]::Sin($t + 4.189))
        $wave[$x * 4 + 3] = 255
    }
    $frame = New-Object byte[] ($width * $height * 4)
    $rowBytes = $width * 4
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $step = 0
    $frameMs = 1000 / 60   # pace to ~60 frames/s; send faster only if you need it
    while (-not $app.ShutdownRequested -and -not $win.Closed) {
        # Handle events until the next frame is due.
        $wait = [int][Math]::Max(0, ($step + 1) * $frameMs - $clock.Elapsed.TotalMilliseconds)
        [void](Wait-UI4ME -Application $app -TimeoutMilliseconds $wait)
        if ($app.ShutdownRequested -or $win.Closed) { break }
        if ($clock.Elapsed.TotalMilliseconds -lt ($step + 1) * $frameMs) { continue }
        for ($y = 0; $y -lt $height; $y++) {
            $shift = ($step * 3 + [int](40 * [Math]::Sin(($y + $step) / 23.0))) % 512
            if ($shift -lt 0) { $shift += 512 }
            [Buffer]::BlockCopy($wave, $shift * 4, $frame, $y * $rowBytes, $rowBytes)
        }
        # Returns once the host has taken the frame: natural backpressure.
        try { $null = Send-UI4MELiveBitmap -Bitmap $bitmap -Pixels $frame }
        catch {
            # Closing the window or quitting retires the bitmap mid-animation.
            if ($app.ShutdownRequested -or $win.Closed -or $bitmap.Closed) { break }
            throw
        }
        $step++
        if (($step % 30) -eq 0) {
            $fps = $step / [Math]::Max(0.001, $clock.Elapsed.TotalSeconds)
            Set-UI4MEDocumentText -Document $doc -Element fps -Text ('Frame {0} · {1} × {2} RGBA8 · {3:0} frames/s' -f $step, $width, $height, $fps)
        }
    }
    # Keep answering the host until it finishes shutting down.
    Wait-UI4ME -Application $app -UntilShutdown
} finally {
    Close-UI4ME -Application $app
}
