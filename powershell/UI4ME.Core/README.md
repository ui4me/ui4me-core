# UI4ME.Core — native HTML/CSS desktop apps in PowerShell

UI4ME.Core runs desktop windows whose interface is local HTML and CSS and whose logic is
PowerShell. A native Rust host (included) owns the windows, rendering, input, IME,
accessibility and dialogs; your script runs as its backend. No browser engine, JavaScript,
WPF, Python or web server is involved.

```powershell
Install-Module UI4ME.Core -Scope CurrentUser
New-UI4MEProject -Path ./my-app -Name 'My App' -AppId org.example.MyApp
Start-UI4MEHost -Manifest ./my-app/app.toml
```

- PowerShell 7.2+ on Linux x86-64 (glibc 2.34+) and Windows x64; Windows PowerShell 5.1 on Windows x64.
- Guide: `docs/powershell.md`. Command reference: `docs/powershell-api.md`.
- Coding-agent skill: `Install-UI4MESkill -Agents all -Scope user`.
- The native hosts in `runtimes/` are the same binaries as in the `ui4me-core` 1.0.0
  Python wheels; `HOST-SOURCE-MANIFEST.json` records their SHA-256. License texts are in
  `licenses/` and `runtimes/*/ui4me_core/_bin/THIRD_PARTY_LICENSES.txt`.

Source, issues and documentation: https://github.com/ui4me/ui4me-core
