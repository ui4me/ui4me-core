# Qualifying a release candidate

Roadmap 6.0 (audit F-11): the published Windows binary must be the binary that
was qualified. MSVC builds are not reproducible across machines, so the
Windows wheel is qualified **after** CI built it and **before** it is
promoted to PyPI, and the workflow checks the hash.

The probes live in `tools/release/qualification/` (cleaned copies of the
audit's evidence scripts) and share one runner:

| Probe | What it checks | Default |
| --- | --- | --- |
| `first_frame.py` | a window opens and the host logs `first frame presented` | |
| `depth.py N` | a document nested N levels deep renders without aborting (F-01) | 250 |
| `svgdepth.py N` | an image SVG with N nested groups does not abort the host (F-03) | 700 |
| `burst.py N R` | N transitioned controls, R colour-scheme switches; the backend stays connected (F-02, issue #1) | 100, 6 |

A probe passes only if it exits 0, prints `RESULT PASS <probe>`, and (in a
windowed run) the host logged its first presented frame. Each probe runs in
its own process group with a deadline.

## Windows: qualify the exact wheel

1. Dispatch a **dry run** of the publish workflow on the release commit:

   ```bash
   gh workflow run publish.yaml --repo ui4me/ui4me-core --ref main \
     -f version=X.Y.Z -f confirmation=publish-X.Y.Z -f dry_run=true
   ```

   It runs the CI gate, builds the sdist and both wheels, verifies them and
   keeps them as the run artifacts `ui4me-release-files` and
   `ui4me-release-provenance`. Nothing is published.

2. Download the Windows wheel of that run
   (`gh run download <run-id> -n ui4me-release-files`) to a Windows x64
   machine and run, **in an interactive desktop session** (RDP or console; an
   SSH session has no desktop and cannot present frames):

   ```powershell
   py -3.12 tools\release\qualify_windows_rc.py `
     --wheel ui4me_core-X.Y.Z-py3-none-win_amd64.whl `
     --output qualification-windows-X.Y.Z.json
   ```

   The tool verifies the wheel (PE32+, 32 MiB stack reserve, no debug host),
   installs exactly that file into a fresh virtual environment, runs the four
   probes against the installed package and writes a record with the wheel's
   and the host's SHA-256 and every probe's exit status and output.

3. Promote the qualified artifacts instead of rebuilding them:

   ```bash
   gh workflow run publish.yaml --repo ui4me/ui4me-core --ref main \
     -f version=X.Y.Z -f confirmation=publish-X.Y.Z \
     -f candidate_run_id=<dry-run id> \
     -f expected_windows_wheel_sha256=<wheel.sha256 from the record>
   ```

   The workflow checks that the candidate run is a successful dry run of
   `publish.yaml` for the same commit and refuses unless its Windows wheel
   has exactly the recorded hash.

4. After publishing, compare once more (acceptance of roadmap 6.0):

   ```bash
   python tools/release/qualify_windows_rc.py --compare qualification-windows-X.Y.Z.json --pypi X.Y.Z
   ```

Attach the record to the release (or commit it under `tools/release/records/`
in the next change).

## Linux: windowed smoke under a private compositor

`tools/windowed_smoke.py` runs the same probes from the checkout with real
Wayland windows. It never uses your desktop: it starts its own compositor on a
unique socket and stops it afterwards.

```bash
cargo build --locked --release -p ui4me-host
python3 tools/windowed_smoke.py --host target/release/ui4me-host                 # weston --backend=headless
python3 tools/windowed_smoke.py --compositor kwin --socket ui4me-smoke-0          # kwin_wayland --virtual
dbus-run-session -- python3 tools/windowed_smoke.py --host target/release/ui4me-host   # plus a private session bus
```

To reuse a nested compositor you started yourself, for example
`env -u WAYLAND_DISPLAY kwin_wayland --virtual --socket ui4me-wsf-0 --width 1280 --height 900 --no-lockscreen &`,
pass `--compositor existing --socket ui4me-wsf-0`; the tool refuses the socket
of your own session. The report is written to
`verification/windowed-smoke/report.json`. Use a release host: the debug host
produces events too slowly to reproduce the F-02 burst. The nightly workflow
runs this under `weston --backend=headless`; it is the first step towards the
30-minute windowed soak (audit gate R4b), which is not implemented yet.
