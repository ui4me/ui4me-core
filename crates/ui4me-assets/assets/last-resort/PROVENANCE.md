# Embedded last-resort font (OD-24, FR-068, R-54)

| Item | Value |
|---|---|
| File | `NotoSans-Regular.ttf` |
| Family / style | Noto Sans, Regular |
| Version string | `Version 2.004; ttfautohint (v1.8.3) -l 8 -r 50 -G 200 -x 14 -D latn -f none -a qsq -X ""` |
| Size | 512,672 bytes |
| SHA-256 | `89c3c497f618fdaa0b2d1e98fef93582f28c71debd2c4a8cdf41f190ced2909d` |
| Licence | SIL Open Font License 1.1 (`OFL.txt`, SHA-256 of this copy in `SHA256SUMS`) |
| Source | Debian/Ubuntu package `fonts-noto-core` (`/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf`); upstream https://github.com/googlei18n/noto-fonts |
| How obtained | Byte-identical copy of the file already pinned by 002 in `verification/fixtures/fonts/noto-sans/` (R-46; `FONT.json` records the same SHA-256). No new acquisition. |
| Coverage note | Latin, Latin Extended, Greek, Cyrillic, Vietnamese and common punctuation/symbols. It is not a pan-Unicode font (no CJK, Arabic, Indic, emoji); characters outside its coverage still render as notdef. |
| Use | Compiled into `ui4me-resources` with `include_bytes!` (`src/fonts/last_resort.rs`); `build.rs` fails the build if the bytes do not match `SHA256SUMS`. The final fallback after the configured fallback list, in system and pinned-fonts-only modes, unless the runtime configuration's `last_resort_fallback` flag is off. No runtime disk discovery. It cannot be unregistered. |
| Distribution | The licence text ships with every distributable (014 FR-070); the size is tracked by 014's footprint report. |
| Approval | **OD-24 (the choice of this font) is recorded as pending owner approval. Nothing here assumes it approved.** The file is copied under the lane brief (an already-pinned, same-SHA, OFL-1.1 file). |
