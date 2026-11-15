# Workstream N: native Linux GUI verification (F-07, F-08, F-17, F-17b)

Date: 2026-11-15. Base: `stabilization/integration` at 67f5336 (workstream D fixes merged).

## Environment

| Item | Value |
| --- | --- |
| Machine | `linuxDEV` lab VM: Ubuntu 26.04.1, KDE Plasma 6.6.6, KWin 6.6.6 **native Wayland** session (`wayland-0`, SDDM autologin), virtio-gpu 1280x800, scale 1.0 |
| Host build | `cargo build --release -p ui4me-host` in `~/projects/ui4me-core-stab` (rsync of the repo, no `.git`/`target`) |
| Backend | system `python3` 3.14 with `PYTHONPATH=python`, `UI4ME_HOST_BINARY=target/release/ui4me-host` |
| Input | `ydotool` through `/dev/uinput` (a real kernel input device; KWin applies the active XKB keymap, then `wl_keyboard`/`wl_pointer` to the host). Pointer moves are relative (`lab-move` homes the pointer first). |
| Accessibility | real AT-SPI bus; host's AccessKit unix adapter; queried with `pyatspi` 2.58 (`org.a11y.Status.IsEnabled` set to true for the run, restored to false) |
| Screenshots | `spectacle -b -n -f` (KWin ScreenShot2), full screen. The probe pages use the background `#e8f0ff`; its bounding box in the screenshot gives the window content origin (240,141), so screen px are converted to window px without knowing the Wayland window position. |
| Packages installed on linuxDEV | `python3-pyatspi`, `gir1.2-atspi-2.0`, `orca`, `fcitx5`, `fcitx5-chinese-addons` (and their dependencies), `python3-pil` |

Note on AT-SPI coordinates: on Wayland the host does not know its window position, so AccessKit reports
`DESKTOP_COORDS` equal to `WINDOW_COORDS` (both are window-relative). All comparisons below use window coordinates.

## Files

| File | Purpose |
| --- | --- |
| `probe_app.py` | Probe app (Python API). Logs document events to `events.jsonl`; executes backend commands appended by the driver to `cmd.jsonl` (focus, state, get_rect, scroll_to, prevent_default, clipboard ...). |
| `nlab.py` | Driver helpers: injected keys/clicks (ydotool), screenshots and pixel measurement (PIL), AT-SPI tree walking (pyatspi). |
| `focus.html`, `keys.html`, `geom.html` | Probe documents. |
| `run_focus.py` | F-08 + F-17. |
| `run_keys.py` | F-17b (Russian layout shortcuts, press/release click target). Switches KWin to `us,ru` and restores the original configuration. |
| `run_geom.py` | F-07 geometry: AT-SPI extents vs pixels, pointer to caret. |
| `run_ime.py` | F-07 IME: fcitx5 candidate popup placement. |
| `run_orca.py` | Optional: Orca speech for Tab focus changes. |
| `results/*.json*`, `results/orca-speech.txt` | Raw outputs of the runs below. |
| `shots/*.png` | Cropped, quantized screenshots (window content area). |

Reproduce (from the workstation):

```bash
rsync -a --delete --exclude target --exclude .git --exclude 'docs/audit.zip' --exclude n-probe ./ linuxDEV:projects/ui4me-core-stab/
rsync -a docs/stabilization/evidence/n-native-linux/ linuxDEV:projects/ui4me-core-stab/n-probe/
ssh linuxDEV 'source ~/.cargo/env; cd ~/projects/ui4me-core-stab && cargo build --release -p ui4me-host'
ssh linuxDEV 'busctl --user set-property org.a11y.Bus /org/a11y/bus org.a11y.Status IsEnabled b true'   # with the session env
ssh linuxDEV 'python3 ~/projects/ui4me-core-stab/n-probe/run_focus.py'   # likewise run_keys / run_geom / run_ime / run_orca
```

## Summary

| Item | Verdict | Channel |
| --- | --- | --- |
| F-08 Tab reveals off-screen focus | **PASS** | injected Tab, pixels, AT-SPI, backend |
| F-08 Tab into nested `overflow:auto` | **PASS** | injected Tab, pixels, AT-SPI |
| F-08 pointer focus does not scroll | **PASS** | injected click |
| F-08 AT-SPI `grabFocus` reveals | **PASS** | AT-SPI call, pixels |
| F-08 backend `focus()` reveals | **PASS** | Python API, pixels, AT-SPI |
| F-17 Enter on a focused link | **PASS** | injected Enter, backend events |
| F-17 Space on a link does not activate | **PASS** | injected Space |
| F-17b editing/clipboard shortcuts on Russian layout | **PASS** | real KWin `ru` keymap, injected keys |
| F-17b `prevent_default(key=...)` rule on Russian layout | **FAIL on 67f5336, fixed in this branch, PASS natively after the fix** | as above |
| F-17b app-defined Ctrl+S via `event["key"]` | **Limitation** (documented pattern misses on non-Latin layouts; `code` works) | backend events |
| F-17b press/release click target | **PASS** | injected pointer drag |
| F-07 AT-SPI extents (fixed, transformed, scaled) vs pixels | **PASS** (within 2 px) | AT-SPI + pixels |
| F-07 pointer to caret under fixed/translate/scale | **PASS** | injected click, backend + AT-SPI caret |
| F-07 IME candidate window placement (fcitx5 pinyin) | **PASS** (popup x = caret x, 3 px below the field) | real IME, pixels |
| Optional: Orca announces focused link/button names | **PASS** (Orca's speech generator output; audio not checked) | Orca debug log |
| Optional: `tools/windowed_smoke.py` on the live session | **PASS 4/4** | windowed host |

## 1. F-08 focus scroll-into-view (`run_focus.py`)

Viewport 800x500, document height 8951 (`max_y` 8451). Targets: `far` at y=1535, `farlink` at 3006, `inner` inside a
120 px `overflow:auto` box (document y 5086, 600 px down inside the box), `atbtn` at 6019, `apibtn` at 7490.
The `:focus` outline is `6px solid #ff2a00`, found in each screenshot by colour.

| Step | Action | focused | scroll y | AT-SPI focused extents (window) | focus ring in window px |
| --- | --- | --- | --- | --- | --- |
| 0 | load ([shot](shots/focus-s0.png)) | none | 0 | | |
| 1 | injected click on `b1` ([shot](shots/focus-01-pointer-focus-b1.png)) | b1 | **0** (pointer focus does not scroll) | Top button 20,64 141x51 | 12,56 - 169,123 |
| 2 | injected Tab ([shot](shots/focus-02-tab-far.png)) | far | **1086** | Far button 20,449 218x51 | 12,441 - 246,500 |
| 3 | injected Tab ([shot](shots/focus-03-tab-farlink.png)) | farlink | **2553** | link 20,453 185x47 | 12,445 - 213,500 |
| 4 | injected Tab ([shot](shots/focus-04-tab-inner.png)) | inner | **4096** | Inner button 43,449 317x51 | 35,441 - 368,500 |
| 5 | backend `scroll_to(0)`, then AT-SPI `Component.grabFocus()` on "AT-SPI target" (was at y=6019) ([shot](shots/focus-05-atspi-grabfocus.png)) | atbtn | **5570** | AT-SPI target 20,449 159x51 | 12,441 - 187,500 |
| 6 | backend `scroll_to(0)`, then `doc.focus("apibtn")` ([shot](shots/focus-06-backend-focus.png)) | apibtn | **7041** | Backend focus target 20,449 237x51 | 12,441 - 265,500 |

In every step the AT-SPI focused object (the object with `STATE_FOCUSED`) is the element the backend reports and its
extents lie inside the 800x500 viewport, and the focus ring is visible in the screenshot. `grabFocus` returned true.
Reveal is "nearest": the element's border box ends exactly at the viewport bottom (449+51=500), so the outer 2 px
of the outline (offset 2 px + 6 px width) are clipped by the window edge. This matches browser behaviour
(scroll-into-view aligns the border box, not the outline) and is not a defect.

## 2. F-17 Enter on a focused link (`run_focus.py`, step 3)

Focus on `<a id="farlink" href="next.html">` by injected Tab, then injected Enter. Events received by the Python
backend (`results/focus-events.jsonl`):

```text
keydown        farlink key=Enter code=Enter
click          farlink
link_activated farlink href=next.html url=file://~/projects/ui4me-core-stab/n-probe/next.html
```

Injected Space on the same focused link: only `keydown key=" " code=Space`, no click and no `link_activated`
(browser behaviour). **PASS.**

## 3. F-17b shortcuts by layout and click target (`run_keys.py`)

Layout switching on KWin 6.6 Wayland: `setxkbmap` does not apply; writing `~/.config/kxkbrc` and emitting the legacy
`org.kde.keyboard.reloadConfig` D-Bus signal had **no effect** on this KWin, and neither did `localectl`. What works
is a KConfig change notification: `kwriteconfig6 --notify --file kxkbrc --group Layout --key LayoutList us,ru`
(KWin's `KConfigWatcher` reloads), then `qdbus6 org.kde.keyboard /Layouts setLayout 1`. Proof that the Russian
keymap was active: injected physical A, S, C typed `фыс` into the textarea, with keydown `key/code` `ф/KeyA`,
`ы/KeyS`, `с/KeyC`. Afterwards the layout list was set back to `us`, `kxkbrc` (absent before) was deleted after
KWin's own rewrite, and `getLayoutsList` again reports only `us`.

**What winit reports natively** for Ctrl+letter on the Russian layout on Wayland: `key` is the Cyrillic letter, `code`
the physical key, `ctrl: true` (for example `key "ф" code "KeyA"`). This is the input the workstream-D fallback
(`keyboard::shortcut_key`) was written for; it is now confirmed natively.

Input `editor` = "hello world" (US rows), textarea `target`:

| Layout | Keys | keydown key / code | Result |
| --- | --- | --- | --- |
| us | Ctrl+A | a / KeyA | `select`, selection 0..11 |
| us | Ctrl+C | c / KeyC | `copy`; clipboard "hello world" |
| ru | Ctrl+A (value set to "ru copy 42" first) | ф / KeyA | `select`, selection 0..10 **PASS** |
| ru | Ctrl+C | с / KeyC | `copy`; system clipboard read back by `app.clipboard_text()` = "ru copy 42" **PASS** |
| ru | Ctrl+V (in `target`) | м / KeyV | `paste` + `input`, value "ru copy 42" **PASS** |
| ru | Ctrl+Z (in `target`) | я / KeyZ | `input`, value "" (undo) **PASS** |
| ru | Ctrl+S (in `editor`) | ы / KeyS | keydown delivered with `code "KeyS"`; nothing typed |

### Finding N-1 (fixed in this branch): `prevent_default` key rules missed on non-Latin layouts

`doc.prevent_default("editor", "keydown", key="a", ctrl=True)`, then Ctrl+A with the caret collapsed:

| Build | us layout | ru layout |
| --- | --- | --- |
| 67f5336 (integration) | prevented, selection stays 0..0 | **not prevented: `select`, selection 0..11** |
| this branch | prevented, 0..0 | prevented, 0..0 (`results/keys-report-fixed-build.json`) |

Root cause: `Document::apply_prevent_rules` (`crates/ui4me-renderer/src/document.rs`) compared the rule's `key`
with the event's literal `key` ("ф"), while the default action itself (select-all) is chosen from
`keyboard::shortcut_key(key, code, modifiers)` ("a"). So on non-Latin layouts an application could not cancel an
editing shortcut that the engine still performed. Fix: the rule also matches the `shortcut_key` letter (only differs
from `key` with Ctrl/Meta held and a single non-ASCII key). Regression test:
`document::interact_tests::prevent_default_shortcut_rules_match_the_physical_key_on_a_non_latin_layout`
(fails on 67f5336 with "RU layout: rule stops select-all, left (0, 11) right (0, 0)", passes with the fix).

### Limitation N-2 (not fixed, documentation): app-defined shortcuts that test `event["key"]`

The example in `docs/reference/python-api.md` recognises Ctrl+S with
`event["ctrl"] and event["key"].lower() == "s"`. On the Russian layout the keydown carries `key "ы"`, so this
handler never fires. Browsers behave the same (`KeyboardEvent.key` is the layout character), so the engine is not
wrong; the documentation should recommend `event["code"] == "KeyS"` for letter shortcuts, or the protocol could add a
`shortcut_key` field. With the fix above, `prevent_default(key="s", ctrl=True)` now matches Ctrl+ы, so only the
handler test needs to change. Owner: bindings/docs (workstream E or lead).

### Click press/release target (INTERACT-07), injected pointer, `ru` layout still active

| Press | Release | Events | Verdict |
| --- | --- | --- | --- |
| `<span id=pspan>` inside `#pbtn` | `#pbtn` padding (74 px left) | pointerdown@pspan, pointerup@pbtn, **click@pbtn** | PASS |
| `#pbtn` | `#other` (another button) | pointerdown@pbtn, pointerup@other, **no click** | PASS |
| `#pbtn` | `#pbtn` (8 px away) | pointerdown@pbtn, pointerup@pbtn, **click@pbtn** | PASS |

## 4. F-07 geometry (`run_geom.py`, `run_ime.py`)

`geom.html`, page scrolled to y=400 (instant):
`#fixed` (`position:fixed; top:60px`) with `finput`; `#modal` (`position:fixed; left:600px; top:250px;
transform: translate(-50%,-50%) translateX(-20px)`) with `tinput`; `#scaled` (`position:absolute; top:690px;
transform: scale(1.5)`) with `sinput`. Inputs have a 2 px border and a white background, so the white area in the
screenshot is the padding box (border box inset by 2 px).
Shots: [scrolled](shots/geom-scrolled.png), [after clicks](shots/geom-after-clicks.png).

| Input | `get_rect` | AT-SPI entry extents | white padding box (pixels) | text ink (pixels) | AT-SPI char 0 left .. last right |
| --- | --- | --- | --- | --- | --- |
| finput (fixed) | 42,96 288x33 | 42,96 288x33 | 44,98 - 328,127 | 48 - 176 | 46 - 178 |
| tinput (fixed + translate) | 430,245.5 288x33 | 430,245 288x33 | 432,248 - 716,276 | 435 - 564 | 434 - 566 |
| sinput (scroll + scale 1.5) | 442,339.5 312x37.5 | 442,339 312x37 | 445,343 - 751,374 | 450 - 519 | 448 - 520 |

The AT-SPI entry extents equal the border box inferred from the pixels (padding box minus the 2 px border, within
1 px), and the character extents bracket the glyph ink (side bearings of 1-3 px). The scaled input's character cells
are 12x28 px for a 14 px font at 1.5x (advance ~12.6 px), i.e. the transform is applied. **PASS.**

Pointer to caret: an injected click on the left quarter of visual character 5 (position computed from pixels only)
gave caret 5 in all three inputs (backend `control_selection` and AT-SPI `Text.caretOffset` agree; the entry has
`STATE_FOCUSED`). This is the INTERACT-03 scenario that put the caret at the end before the fix. **PASS.**

### IME candidate window (fcitx5 5.1.19, pinyin)

Set up: `kwinrc [Wayland] InputMethod=/usr/share/applications/fcitx5-wayland-launcher.desktop` (KWin starts fcitx5
as its `zwp_input_method_v1`), fcitx5 profile with `keyboard-us` + `pinyin`, trigger Ctrl+Space. The host talks
`text-input-v3` (KWin reports `activeClientSupportsTextInput: true`). For each input: click character 3, Ctrl+Space
(`fcitx5-remote` = 2, active), type `ni`, screenshot, Escape, Ctrl+Space.

| Input | caret during composition | AT-SPI cell at caret (window) | candidate popup (screen) | popup - caret (dx, dy below cell bottom) | popup top - field bottom |
| --- | --- | --- | --- | --- | --- |
| finput ([shot](shots/ime-finput.png)) | 5 | 106,99 12x27 | 346,273 - 639,300 | 0, 6 | 3 px |
| tinput ([shot](shots/ime-tinput.png)) | 5 | 494,248 12x27 | 734,422 - 1027,450 | 0, 6 | 3 px |
| sinput ([shot](shots/ime-sinput.png)) | 5 | 508,344 12x28 | 748,520 - 1041,547 | 0, 7 | 3 px |

The popup's left edge is exactly at the caret, directly below the field, in the fixed box, the transformed modal and
the scaled box on a scrolled page. For comparison, the audit's headless probe on 1.0.1 measured the IME cursor area
at y=-290 for a fixed-box textarea and at untransformed x for a translated input (INTERACT-02/03). The preedit is shown inline (`compositionstart`/`compositionupdate`/
`input` events; value "fixnied field" during composition) and Escape cancels it (value restored). **PASS.**
Only pinyin through fcitx5 was tested; ibus was not set up.

## 5. Optional checks

**Orca 50.3** (`orca --replace --debug-file=...`), injected click on "Top button" then three Tabs. Speech generator
output from the debug log (`results/orca-speech.txt`): "N-probe focus", "focus probe document web",
"Top button button.", "Far button (y~1500) button.", "Far link (y~2900) link.",
"Inner button (in overflow box) button.". Names and roles are announced for each focus change, including the link
and the button revealed inside the scrolled box. Audio output (speech-dispatcher) was not verified; the VM has no
audio sink in use.

**Windowed smoke** on the live session: `python3 tools/windowed_smoke.py --compositor existing --socket wayland-0
--host target/release/ui4me-host` (run from an SSH shell without `WAYLAND_DISPLAY`) -> PASS 4/4
(first-frame 1.56 s, depth 250, svgdepth 700, burst 100 controls / 10148 events); `results/windowed-smoke-report.json`.

## Not verified here

- Windows (UIA/Narrator, winit-win32 key values for Ctrl+letter on a Russian layout, Windows IME): needs the lead's
  Windows run.
- ibus as IME; Orca audio; HiDPI scale factors other than 1.0; rotation transforms.
- The F-08 "nearest" behaviour inside a `position:fixed` scroll container was not probed natively (unit test only).
