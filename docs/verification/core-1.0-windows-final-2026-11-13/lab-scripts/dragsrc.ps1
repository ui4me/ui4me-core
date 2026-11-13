param([string]$Mode)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -Path (Join-Path $PSScriptRoot 'DragSrc.cs') -ReferencedAssemblies System.Windows.Forms
[DragSrc]::Run($Mode, 'F:\ui4me\lab', 'F:\ui4me\lab\out\dragsrc.log')
