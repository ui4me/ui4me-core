# Compatibility entry point: the app lives in app.ps1, which app.toml runs.
# Start-UI4MEScript in app.ps1 only connects when the host started this backend.
& (Join-Path $PSScriptRoot 'app.ps1') @args
