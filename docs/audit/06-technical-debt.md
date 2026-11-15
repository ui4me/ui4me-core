# 6. Technical debt and maintenance assessment

The question here is not "what is imperfect" but "what will cost more every month it stays". Items are split accordingly.

## 6.1 Genuine long-term liabilities

| Liability | Why it compounds | Action |
| --- | --- | --- |
| **IPC event contract (all events, depth-based disconnect)** | Every new event type, every new client and every heavier page makes F-02 more likely; clients may come to depend on unsubscribed events. | Remove the count-based disconnect and fix binding drop policies now (roadmap 1.2, 1.3); type-level subscription in 1.1 (3.1). |
| **Recursion depth not budgeted against stacks** | New CSS features and element types add stack frames silently; the safe depth shrinks without anyone noticing. | Depth-survival test at documented limits (T1) makes this a tested invariant. |
| **Five separate geometry paths** | Every new positioned/transformed feature must be fixed five times. | One geometry service (roadmap 3.2). |
| **Headless ≠ native loop** | Each new frame-driven feature (animations, smooth scroll, timers) is untestable in CI. | Virtual clock + frame pump in headless (roadmap 2.1). |
| **Release machinery that edits itself** (version literals in ~61 files, committed manifest, duplicated build recipes, hand-written ledgers) | Every release is a large diff to CI code, reviewed under time pressure; evidence rots. | Single-source version, generated manifest/records, one reusable build workflow (roadmap 6.x). |
| **Taffy fork (~1,800 added lines, subgrid)** and Stylo patch stack | Upstream security or correctness fixes need manual rebases; subgrid bugs are owned here. | Freeze feature work on the fork, schedule `verify.sh`, upstream where possible (roadmap 8.2). |
| **winit 0.31 beta pin with two patched platform crates** | A beta API can change under you; patches must be re-validated natively on each bump. | Keep until a stable 0.31; then one deliberate upgrade with the native suite. |
| **Unmaintained transitive crates** (`ttf-parser` via `fontdue`, zbus wire crates) | No security SLA; `fontdue` is a direct host dependency used only for a diagnostic probe font path. | Replace `fontdue` with `skrifa`/parley, which are already in the tree; track zbus 6 for AccessKit. |
| **Single maintainer, no review history** (34 direct pushes from one account, 480k-line import commits, mostly AI-written code) | Bus factor of one; bisect and blame stop at "import"; nobody else has read most of the code; the roadmap itself depends on one person's capacity. | Work through PRs even without a second reviewer (CI on every change, readable history); recruit a second maintainer before the stable claim; do not rewrite history. |
| **Lab knowledge outside the repo** (private uinput daemon, KWin scripts, RDP via Xvfb, Proxmox configs) | Only the lab operator can qualify a release; evidence cannot be re-checked. | Move the automatable part into CI (T14); retain raw evidence as release assets. |

## 6.2 Harmless imperfections (leave alone or fix opportunistically)

- Two-syscall frame writes (HOST-09): fix with the transport work, not separately.
- Crate-wide `#[allow(dead_code)]` (RENDER-11): remove when convenient; it hides dead code, it does not cause bugs.
- Raw `text_ptr` in the layout host tree (RENDER-13): sound today; replace with a borrowed reference when the file is next touched.
- Fixed 40 px wheel line delta, hard-coded double-click time (INTERACT-11, -13): platform-polish items.
- `/tmp` fallback runtime directory, endpoint name derived from token prefix, Windows pipe client not verifying the server (HOST-11, -12, -13): low-risk hardening; batch them into one security-hardening PR.
- Sleep-synchronised asset tests (RELEASE-16): no observed flakes; replace with polling helpers when touched.
- String-matching workflow tests (RELEASE-15): replace when the workflows are restructured.

## 6.3 Debt that looks bad but is not

- **The size of the renderer.** Most of it is integration of real engines (Stylo, Taffy, parley, vello). Shrinking it by rewriting is not cheaper than maintaining it.
- **The number of vendored crates.** Each patch fixes a reproduced platform defect (Wayland occluded-window freeze, Win32 IME deadlock, delayed-render DnD stall, GTK clipboard MIME, AT-SPI object leak) and is documented with removal conditions. This is the correct way to carry fixes.
- **Strict, fail-fast protocol handling.** It looks unforgiving; it is what keeps the boundary safe.
- **Dual-role direct start.** "Code above `start()` runs twice" is surprising, but it is documented and taught, and the alternatives (a separate launcher script or an in-process host) are worse for the target users.
