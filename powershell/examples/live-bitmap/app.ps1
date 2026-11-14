# Live bitmap: PowerShell writes raw RGBA8 frames into a host-created shared buffer.
#
# Run it directly:
#   pwsh -File ./app.ps1                          (PowerShell 7.2+, Linux or Windows)
#   powershell.exe -NoProfile -File .\app.ps1     (Windows PowerShell 5.1)
# Or through the manifest:  Start-UI4MEHost -Manifest ./app.toml
#
# Frames are assembled with [Buffer]::BlockCopy from a precomputed colour
# wave; per-pixel PowerShell loops are far too slow for animation. The script
# pumps events between frames, paced to about 60 frames/s.
$ErrorActionPreference = 'Stop'
# Use the installed module; from a source checkout, fall back to the module here.
try { Import-Module UI4ME.Core -MinimumVersion 1.0.1 } catch { Import-Module (Join-Path $PSScriptRoot '../../UI4ME.Core/UI4ME.Core.psd1') }

$app = Start-UI4MEScript -Name 'PowerShell Pixel Preview' -AppId 'org.ui4me.PowerShellLiveBitmap'

$width = 320; $height = 200
$window = New-UI4MEWindow -Application $app -Title 'PowerShell Live Pixels' -Width 780 -Height 560
$doc = Open-UI4MEFile -Window $window -Path (Join-Path $PSScriptRoot 'index.html')
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
while (-not $app.ShutdownRequested -and -not $window.Closed) {
    # Handle events until the next frame is due.
    $wait = [int][Math]::Max(0, ($step + 1) * $frameMs - $clock.Elapsed.TotalMilliseconds)
    [void](Wait-UI4ME -Application $app -TimeoutMilliseconds $wait)
    if ($app.ShutdownRequested -or $window.Closed) { break }
    if ($clock.Elapsed.TotalMilliseconds -lt ($step + 1) * $frameMs) { continue }
    for ($y = 0; $y -lt $height; $y++) {
        $shift = ($step * 3 + [int](40 * [Math]::Sin(($y + $step) / 23.0))) % 512
        if ($shift -lt 0) { $shift += 512 }
        [Buffer]::BlockCopy($wave, $shift * 4, $frame, $y * $rowBytes, $rowBytes)
    }
    try {
        # Returns once the host has taken the frame: natural backpressure.
        $null = Send-UI4MELiveBitmap -Bitmap $bitmap -Pixels $frame
        $step++
        if (($step % 30) -eq 0) {
            $fps = $step / [Math]::Max(0.001, $clock.Elapsed.TotalSeconds)
            Set-UI4MEDocumentText -Document $doc -Element fps -Text ('Frame {0} · {1} × {2} RGBA8 · {3:0} frames/s' -f $step, $width, $height, $fps)
        }
    } catch {
        # Closing the window or quitting retires the bitmap mid-animation.
        $ending = $app.ShutdownRequested -or $window.Closed -or $bitmap.Closed
        if ($_.Exception -is [UI4ME.RpcException] -and $_.Exception.Code -eq -32008) { $ending = $true }   # shutting down
        if ($ending) { break }
        throw
    }
}
Remove-UI4MELiveBitmap -Bitmap $bitmap -Quiet
# Keep answering the host until it finishes shutting down.
Wait-UI4ME -Application $app -UntilShutdown
Close-UI4ME -Application $app
