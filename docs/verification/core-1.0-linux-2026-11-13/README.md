# Core 1.0.0-rc.1 — Linux/KDE Plasma Wayland checkpoint (2026-11-13)

Evidence for the first milestone of `LOCAL_FINISH_PROMPT.md`: Python + Linux KDE
on native Wayland. **Windows Server 2022 has not been run for this candidate.**

| Item | Value |
| --- | --- |
| Source | commit `d94ecfc`, ledger `source_sha256` `856a0fb2dc6b6e56…` |
| Host | installed `~/.cargo/bin/ui4me-host` 1.0.0-rc.1, 40,460,784 bytes, SHA-256 `fc422be508059785022d3937d277e51d14e80be4f3731a47fdfff6828bdecc33` |
| OS / session | Debian 13.7, kernel 6.12.116, KDE Plasma 6.3.6, KWin native Wayland, normal user |
| Display | one output, 1920×1080, scale 1 (1.5 for R06/N08). The physical monitor was disconnected during the run; the HDMI connector was forced on with a generated 1920×1080 EDID so KWin had a real DRM output |
| Input | kernel uinput keyboard and absolute pointer through libinput (no physical mouse attached); fcitx5 5.1.12 pinyin + us-intl (temporary) |
| Assistive tech | AT-SPI 2.56.2, Orca 48.1 |

## Results

* Automated gates: [linux-final-gates.txt](linux-final-gates.txt) — all verifier
  stages PASS, 1,085 Rust tests passed / 0 failed (11 pre-existing ignored
  harnesses, nothing filtered), real AF_UNIX IPC fixtures included; clippy clean.
* Ledgers, each `qualification: PASS` with its automated subset:
  [host N01–N15](linux-host-acceptance.json),
  [renderer R01–R12](linux-renderer-acceptance.json),
  [completion C01–C12](linux-completion-acceptance.json).
  Per-check actions/observations/limitations and evidence-file hashes:
  [linux-evidence-summary.json](linux-evidence-summary.json).
* Browser oracle: [R12 fixtures](linux-browser-report.json) and focused
  [C10 comparisons](linux-c10-browser-comparison.json) at 640, 360 and 900 px.
* Measurements: [linux-measurements.txt](linux-measurements.txt) and raw reports.
  Renderer reference: ready median 569 ms (0.2: 527 ms), idle host 0.22 % /
  backend 0.28 % CPU, host+backend PSS 59.6 MB (0.2: 60.1 MB), DOM churn flat,
  window cycles ~18–21 KiB/cycle with AT-SPI on (known zbus residual). Probe:
  131 ms, 33.2 MB, 3.3 KiB/cycle. Explicit reduced motion changes nothing because
  the default reference is static. Caret blink costs ~1.4 % host CPU.

## Defects found and fixed (before/after in [defects/](defects/))

| ID | Defect | Fix |
| --- | --- | --- |
| D1 | Clipboard writes always failed on a keyboard-only Wayland seat ("no seat has received input") | winit-wayland seat patch: selection devices for keyboard or pointer seats |
| D2 | Dropdown `<select>` exposed no options/selection; Orca said "Priority combo box Priority" | Options exposed as combobox children; assistive option pick with input/change |
| D3 | A node reference from a replaced document modified the replacement | References are `node:<document>:<index>:<generation>` |
| D4 | On a dark desktop, pages without `color-scheme` got dark UA control colors (light text on author light backgrounds) | Stylo patch P-006-20 + canvas rule: `normal` uses the light UA scheme |
| D5 | Replacing/removing displayed bundle images retained every decoded raster (+256 KiB per replacement) | Picture table prunes superseded and retired requests |
| D6 | Removing the stylesheet of an infinite animation kept repainting (~36 % CPU) | Cancel unstyled animations after sheet changes |
| D7 | AT-SPI published no table indices, tree levels, live/atomic metadata or ARIA relations | accesskit_atspi_common attribute/relation patch |

Each has a regression test that fails before and passes after, and native
re-verification with the final binary.

## Open findings and limits (not claimed fixed)

* **Bundle-replacement residual:** repeatedly replacing a *displayed* bundle image
  still grows host PSS by ~17–19 KiB per replacement (decoded cache flat at one
  raster; independent of AT-SPI and of the glibc mmap threshold; plain DOM churn
  and unused bundle registration are flat). Not yet attributed.
* **IME blur semantics (C04):** a composition interrupted by a focus change keeps
  its composed text in the field that owned it and the platform preedit is reset
  (browser behavior, retained per CORE_1.0_SPEC). Nothing leaks into the next
  field. C04's wording could be read as requiring the text to be discarded; this
  needs a reviewer decision.
* AT-SPI has no Table/TableCell interfaces in accesskit_unix 0.24.1; indices are
  object attributes. `prefers-contrast` is not a supported media feature; KDE's
  portal exposes no reduced-motion value; the KDE folder chooser is unparented;
  resize grips are physical lower-right (CSS-03); AT-SPI ~19 KiB per closed
  window (zbus).
* Evidence was gathered with a forced (EDID-override) output and virtual uinput
  devices rather than a physically attached monitor/mouse.
