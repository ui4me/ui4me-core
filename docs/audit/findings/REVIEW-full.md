# Adversarial review of the draft UI4ME Core 1.0.1 audit

Reviewer: independent principal-engineer pass, read-only. Inputs: `docs/audit/README.md`, `01`…`08`, `findings/{HOST,RENDER,INTERACT,INPUTSEC,RELEASE}.md`, the draft `BIND.md` in the session scratchpad, and `docs/audit/evidence/`.

How I checked: I read every document. I spot-checked the code claims that were cheap to verify (`transport.rs`, `host/mod.rs`, `client.py`, `document.rs`, `environment.rs`, `fuzz_gate.rs`, `build.rs`, `host.manifest`, the vendored winit crates, the trust statements in the docs). I diffed the evidence logs and measured the maximum DOM depth of the repository's own HTML with a throw-away parser (`<scratch>/review/maxdepth.py`). I ran no cargo builds or tests and nothing remote.

Confidence scale: **High** = verified in code or evidence. **Medium** = sound reasoning, not executed. **Low** = an informed guess that needs a check.

---

## A. Must fix before delivery

### M1. Broken deliverable: missing files, a placeholder, non-durable paths
- **Target:** README document table; `BIND.md`.
- **Objection:** The README links `09-adversarial-review.md` and `findings/BIND.md`, and neither file exists under `docs/audit/`. `BIND.md` still contains `MATRIX_PLACEHOLDER` (§BIND-19, "The repeated matrix … `matrix.out` (MATRIX_PLACEHOLDER)"). It also cites absolute session paths (`/tmp/claude-1000/…/PS_FINDINGS.md`, `…/exp/BIND/py/`), which the other area files replaced with `<scratch>/` plus a retained-evidence note. It opens with "Two people did this audit", but they were two agents.
- **Proposed change:**
  - Copy BIND into `findings/`.
  - Either fill in the matrix or delete that sentence.
  - Replace the absolute paths with `<scratch>/…` and add the same "not retained" coordinator note the other files carry. Retain `PS_FINDINGS.md` under `evidence/scripts/bind/` if it is cited.
  - Change the first line to "Two review agents did this audit."
  - Create `09` (from this review), or remove the row.
- **Confidence:** High.

### M2. The headline stack-cost figure for F-01 is wrong
- **Target:** 02 F-01 ("~6–24 KiB of stack per element"), 03 §3.3 B ("~6–24 KiB per element"), INPUTSEC-01 ("stack-pointer delta ~6.4 KiB per element").
- **Objection:** At 6 KiB per level, 40 levels would use about 240 KiB and could not overflow 1 MiB. The real figures:
  - RENDER-02 measured **23.6 KiB per nesting level** (0x5C30, opt-3) for `<div>`.
  - INPUTSEC's 6.4 KiB is per *frame*, with about 4 frames per element.
  - INPUTSEC-01's own data puts `<fieldset>` at about **32–34 KiB per level**: it overflows 8 MiB at 252 (8192/252 ≈ 32.5) and 1 MiB at about 30.
- **Proposed wording (02 F-01):** "(1 MiB main-thread stack; the recursive box-tree builder uses ~24 KiB of stack per nesting level for `<div>` and ~32 KiB for `<fieldset>`)". Make the same change in 03 B. In INPUTSEC-01, replace "~6.4 KiB per element" with "~6.4 KiB per frame, ~4 frames (~24 KiB) per element".
- **Confidence:** High.

### M3. The stack remediation does not meet its own invariant, and it does not fix Linux
- **Target:** 03 §3.3 B (the invariant "survive the documented maximum … with 2× headroom … test on a thread whose stack equals half the production size"), 07 1.1(a)/(e), T1, RENDER-02 remediation.
- **Objection:**
  1. **Windows.** With the proposed `/STACK:16777216`, T1 runs at half of that, 8 MiB. 256 `<fieldset>` levels need about 256 × 32.5 KiB ≈ 8.3 MiB, so T1 fails for fieldset as specified. RENDER-02's "~2× headroom" is computed for `<div>` only.
  2. **Linux.** The production main stack is `RLIMIT_STACK`, normally 8 MiB, so T1 would run at 4 MiB. Fieldset already overflows 8 MiB at depth 252 (INPUTSEC-01). The PE reserve does nothing on Linux. Item (e), "measure … and either lower `MAX_DEPTH` or reduce frame sizes", is therefore mandatory, not optional. That makes 1.1 an M, not "S–M".
- **Is `/STACK` itself correct?** Yes (see D6). The main thread of a Windows process takes its reserve from `SizeOfStackReserve` in the PE header. winit runs the loop and every window procedure on that thread, including re-entrant calls from modal move/resize loops, so all layout runs within that reserve. On x64 a larger reserve costs address space only. Rust `std::thread::spawn` threads pass an explicit 2 MiB and are unaffected. The flag must be emitted under `CARGO_CFG_TARGET_ENV == "msvc"` from the existing `crates/ui4me-host/build.rs`, which today only calls `embed_resource`. It does not reach test binaries, so T1 must keep using explicitly sized threads.
- **Proposed change:** Choose one of these and write it into 07 1.1:
  - **(i)** Use a 32 MiB reserve on Windows, run the engine on an explicitly sized thread on Linux, and set T1 at reserve ÷ 2.
  - **(ii) Preferred.** Run the event loop on one explicitly sized thread (e.g. `std::thread::Builder::stack_size(32 << 20)`) on **both** OSes. This is deterministic, independent of linker flags and `ulimit -s`, and directly testable.
  - **(iii)** Use `stacker::maybe_grow` at the `Builder::element` recursion point.

  In every case, keep the `&BoxStyle` frame-shrinking as the 1.1 or 1.2 durable fix.
- **Confidence:** High for the arithmetic. Medium that MSVC frame sizes match LLVM-on-Linux closely (both use the LLVM backend, but calling conventions differ).

### M4. The reason given for rejecting the "large-stack engine thread" is factually wrong
- **Target:** 08 "Postponed" row ("winit and AccessKit are main-thread bound on Windows"), RENDER-02 alternatives ("bigger refactor: winit and AccessKit are main-thread bound").
- **Objection:**
  - The vendored winit-win32 supports `EventLoopBuilderExtWindows::with_any_thread`: `third_party/winit-win32-0.31.0-beta.4/src/event_loop.rs:172, 188, 226-231`, where the check is only `if !attributes.any_thread && thread_id != main_thread_id()`.
  - winit-wayland has the same extension (`third_party/winit-wayland-0.31.0-beta.4/src/lib.rs:83`).
  - No evidence is offered for AccessKit. Its Windows adapter is tied to the window's thread, not the process's main thread.
- **Proposed change:** Replace the 08 row with: "Run the event loop on an explicitly sized thread (`with_any_thread`). Viable on both platforms. The cost is moving `main` to a join-and-propagate wrapper and re-checking COM/OLE initialisation on that thread. Preferred if the Linux half of R2 is to be met without frame surgery." Fix RENDER-02 to match.
- **Confidence:** High for winit. Medium for AccessKit.

### M5. T1 and R2 cannot pass in 1.0.2 as written, because inline-block layout is exponential
- **Target:** 04 T1 (list includes `inline-block`), 07 1.1(e) and its acceptance criterion, 05 R2 ("depth 256 for every element type"), 05 §5.4 ("1.0.2 … R2 … met").
- **Objection:** INPUTSEC-06 measures nested `display:inline-block` as roughly doubling per level: 11.5 s at depth 26. At depth 256 the test would not finish. The fix (memoise shrink-to-fit) is roadmap 1.4, which is **P1**, not P0.
- **Proposed change:** Pick one.
  - Drop `inline-block` from T1 and R2 for 1.0.2. Add it back with 1.4, and add a note to R2: "abort-free, not time-bounded; time is R5".
  - Promote INPUTSEC-06 memoisation to P0.
- **Confidence:** High.

### M6. The F-02 fix moves the failure into the bindings, and that defect is filed under the wrong finding
- **Target:** 02 F-05 (area refs "HOST-02, BIND-02"), 07 1.2 and 2.2, 05 §5.4.
- **Objection:**
  - **The Python client drops when its queues fill.** The ordered-notification and callback queues each hold 128 messages (`python/ui4me_core/client.py:300, 304`). When full, notifications are dropped and only counted (`:436-442`). That includes `window.event closed` and `document.error` (BIND-02, reproduced: 191 dropped, `closed` never seen).
  - **PowerShell drops too.** Admission is first-come at 128. A 300-event burst dropped 173 events, including a following `click` (BIND-PS-02), and `closed` can be dropped (BIND-PS-03).
  - **BIND-17 measured the Python drop rate.** It dropped 22–28% under GIL contention and 150 of 20,000 even uncontended.
  - **Consequence for 1.0.2.** Once 1.2 stops the host from disconnecting, a theme-switch burst lands in these queues. The 1.2 acceptance test (`burst2.py 100 6 env` passes) would still pass, because `burst2.py` checks liveness only with a response round-trip (`d.get_text("s")`). Lifecycle notifications could still be lost silently. This is the same failure F-05 describes for PR #3, moved to the client.
  - **BIND-02 has no row of its own.** It is a confirmed defect in shipped software, filed inside a finding about a draft PR.
- **Proposed change:**
  - Add a register row: "F-xx: Bindings drop control notifications under event bursts (`closed`, `document.error`, `app.activated`), silently. BIND-02, BIND-PS-02, BIND-PS-03. Medium-High / High once F-02 is fixed. Reproduced (BIND). Phase 1."
  - Put the typed drop policy (never drop control notifications; drop coalescible types first; warn once) into 1.2 as P0 for both bindings.
  - Extend the 1.2 acceptance criteria: "`app.rpc.dropped_notifications` counts no control notification, and a `request_close()` during a burst retires the window, in Python and PowerShell 5.1."
- **Confidence:** High.

### M7. Roadmap 1.2 is called a "stopgap" but is the redesign, and a one-constant fix already meets R3
- **Target:** 07 1.2 (lanes, reserved capacity, stall-based disconnect, byte budget, coalescing, single syscall, log-once, rated "S–M"), 03 §3.3 A ("stopgap (bigger queue, single-syscall frames, rate-limited logs)"), HOST-01 complexity ("M for lanes + stall policy; L full"), 08 ("Fix F-02 only by enlarging the queue … rejected").
- **Objection:**
  - **The three documents disagree** on what the stopgap is, and 07 understates its effort.
  - **A byte cap already exists.** It is 8 MiB per connection (`transport.rs:18`, enforced at `:211-214`). The binding constraint is the **message count** of 128 (`:17`, `sync_channel` at `:273`, `try_send` at `:215`), turned into a disconnect by `Host::send` (`host/mod.rs:442-447`).
  - **The cap absorbs realistic bursts.** At the measured frame sizes (360–600 B), 8 MiB holds about 14k–23k events, so R3's 10,000-event burst fits. A reading backend drains at about 35–45k frames/s (BIND-19). A backend that stops reading is still cut off by the 10 s write deadline or by the byte cap.
  - **Nothing is dropped by this change,** so lanes, reserved capacity, `seq` and `events_lost` are not needed yet.
- **Proposed 1.0.2 scope (replace 1.2 "Do"):**
  - (a) Bound the outbound queue by bytes only. Use an unbounded channel with the existing byte counter, or a count of e.g. 32k. `sync_channel` preallocates its slots, so a very large count costs memory up front.
  - (b) Write header and body in one syscall (HOST-09).
  - (c) Log the first failure once per connection (HOST-08).
  - (d) Add the binding drop policy (M6).
- **Effort:** S + S + S + M. Move lanes, coalescing, `seq` and `events_lost` to 3.1, and justify them there by measured need. Reword the 08 row to: "Enlarging only the message count while keeping the 8 MiB byte cap and write deadline is the 1.0.x fix. Subscription follows in 1.1."
- **Confidence:** High for the code facts and arithmetic. Medium that no hidden GUI-thread cost appears at 10k serialised events (not measured).

### M8. PR #3 is rejected by HOST but recommended by BIND, and F-05 is not a defect in the product
- **Target:** BIND-17 remediation ("The fix must be host-side: PR #3 drop/coalesce"), BIND-19 remediation 1 ("Drop or coalesce notifications (PR #3)"); 02 F-05 ("High (if merged) / Likelihood High"); README Q2.
- **Objection:**
  - BIND contradicts HOST-02, F-05 and 08 ("Merge PR #3 as is: Rejected").
  - A draft PR has no severity or likelihood in a register of shipped defects. "Likelihood High" that a draft is merged has no basis.
  - Listing F-05 among the "most important actual or potential defects" (README Q2) inflates the count.
- **Proposed change:**
  - In BIND, replace "(PR #3)" with "(an allowlisted drop of non-semantic high-frequency types only, as in HOST-02's quick-patch note; not PR #3 as written)".
  - Turn F-05 into a "Review of open PRs" note under F-02's remediation, rated "n/a (advisory)". Remove it from README Q2.
  - Keep "Do not merge PR #3 as written" in 07 1.2.
- **Confidence:** High.

### M9. The 1.0.2 test scope differs across three documents, and one listed test depends on Phase 2
- **Target:** README Q12 ("T1–T4, T7–T9"), 05 §5.4 ("T1–T4 in CI"), 07 5.1 ("T1, T2, T4, T7, T9 (P0)"), 04 §4.3 ("T1–T4 and T7–T9 … roughly two engineer-weeks").
- **Objection:** The three lists differ. T3 needs the headless virtual clock (04: "M (needs the clock)"), which is roadmap 2.1, Phase 2, P1. T8 (publish preflight refusal) appears in the README but not in 07 5.1. The "two engineer-weeks" estimate ignores the clock and the Windows release-profile CI job that T1 requires.
- **Proposed change:** Use one list everywhere: "1.0.2: T1 (without inline-block, see M5), T2, T4, T7, T8, T9, plus the binding drop test from M6. T3 ships with 2.1." Re-estimate as "about three engineer-weeks including fixes".
- **Confidence:** High.

### M10. R5 (no stall over 250 ms) cannot be met, and its cited evidence is wrong
- **Target:** 05 R5 ("No GUI-thread stall > 250 ms from any single command on documents within limits, release build").
- **Objection:**
  1. **Wrong build.** The evidence cites `request_submit` "≈ 12 s (debug)". HOST-06 itself rates this Low because the numbers are from a debug build.
  2. **Unattainable for documents within limits.** The limits are 65,536 nodes and 1 MiB of HTML. RENDER-06 measured the first render of 2,000 short rows at 1.1–1.4 s, at opt-2 in a harness. INTERACT measured a single `set_text` in a 5,000-item list at about 2.0 s of flush. A cold load of a document near the node limit cannot plausibly take under 250 ms without asynchronous or incremental layout, which the audit rightly postpones. The gate would stay FAILING for structural reasons, which defeats its purpose.
- **Proposed wording:** "R5: In a release build:
  - (a) Every pathological input class (SVG filters, nested shrink-to-fit, unclosed-block parse, `request_submit` on N fields) is bounded to under 250 ms or rejected with a clean error.
  - (b) On the reference documents (a 200-control form and a 2,000-row log), any single update stalls under 100 ms, and an append after 20 unrelated updates stalls under 100 ms.
  - (c) While backend commands are queued, the frame gap stays under 50 ms (time-budgeted pump)."

  Re-measure the `request_submit` case in release before citing it.
- **Confidence:** High.

### M11. R4's threshold is not calibrated against the audit's own control run
- **Target:** 05 R4 ("RSS slope < 0.5 MiB per 10 min after warm-up").
- **Objection:**
  - The coordinator's non-leaking control (`evidence/logs/none.log`) still grows 52,796 → 52,908 KiB between 94.5 s and 126.5 s. That is about 1.3 MiB per 10 minutes, well above the gate.
  - RSS includes touched pages of memory-mapped font files and allocator fragmentation, so it is noisy at this resolution.
  - The gate as written could fail on a fixed build, or pass a slow leak depending on where "warm-up" ends.
- **Proposed wording:** "R4:
  - (a) In-process counting-allocator soak (T4 style): live heap grows < 1 MiB over N = 20,000 mixed operations after the first 2,000 (deterministic, in CI).
  - (b) Nightly 30-min windowed soak: RSS over the last 20 min grows < 5 MiB or < 5 %, whichever is larger.
  - Define warm-up by operation count, not wall time."
- **Confidence:** High that the threshold is uncalibrated. Medium on the replacement numbers.

### M12. The repository-protection items cannot be done by a solo maintainer, and the plan facts are probably wrong
- **Target:**
  - 05 P3 ("publish environments need a second person").
  - 07 6.2 ("make the repository public or move to a plan …; require a reviewer on `pypi`/`psgallery` …"), rated P0.
  - RELEASE-01 remediation 3 ("or move to GitHub Team. Then … add a required reviewer").
  - README Q8.
- **Objection:**
  1. **There is only one author.** All 34 commits are from one account (`git log`). "Second person" and "require PRs and review" cannot be met.
  2. **Plan facts.** Required reviewers on deployment environments in **private** repositories require GitHub Enterprise, as far as I know. GitHub Team gives branch protection but not environment reviewers. Please verify against current GitHub docs before stating it.
  3. **Making the repository public** is an ownership, IP and licensing decision, not an engineering fix. The audit can lay out the trade-off but should not recommend it.
  4. **The control that matters does not depend on the plan.** The CI-status check in the publish preflight (6.1, RELEASE-01 item 1) works on any plan, and manual typed-phrase dispatch already exists.
- **Proposed change:**
  - P3 becomes: "Publish requires green required CI for the SHA (P1) and an explicit manual dispatch. Branch protection and reviewer gates are adopted when the plan or visibility allows, and a second maintainer exists."
  - 6.2 drops to "Decision for the owner (not P0)", with the three options (public / Team / Enterprise) and what each enables.
  - Remove 6.2 from README Q8.
- **Confidence:** High on items 1, 3 and 4. Medium on item 2.

### M13. The fuzzing claims contradict the code
- **Target:**
  - RENDER-10 ("dictionary has no deep-nesting or move-chain generators").
  - 08 Postponed "cargo-fuzz" row ("The existing seeded harnesses, run in CI with a 1 MiB stack, would have caught every content crash found here").
  - 04 §4.1 ("would catch nesting/resource bombs if run with a production-sized stack").
- **Objection:**
  - `fuzz_gate.rs:172-178` already has a nesting-bomb mutation. It inserts up to 600 levels of `div`, `span`, `table`, `ul`, `svg`, `a`, `p` or `i`, so RENDER-10 is wrong about HTML nesting.
  - Nobody ran the harnesses at 1 MiB, so "would have caught every" is unverified.
  - It is also unlikely for two of the crashes:
    - **F-03** needs `<g>` nesting inside an *image* SVG. The `svg` tag in the bomb is inline SVG, which is capped at depth 48 (`inline_svg.rs:33`).
    - **F-06** needs move chains past depth 256. The random `move_element` in the DOM-sequence harness is unlikely to build them.
- **Proposed wording (08):** "Run at a 1 MiB worker stack, the existing HTML harness would very likely have found F-01 (it already generates nesting bombs to depth 600). F-03 and F-06 need new generators (nested `<g>` and `clip-path` chains in image SVG, and deep move chains)." Fix RENDER-10 to match.
- **Confidence:** High.

### M14. The register index and R1 have gaps
- **Target:** 02 §2.2 (BIND row stops at 18), 02 F-02 area refs, 05 R1 evidence.
- **Objection:**
  - BIND-19 (High, the windowed measurement of issue #1) and all of BIND-PS-01…19 are missing from the index. BIND-PS-01, a `psbase` attribute plus `get_attributes` that kills the PowerShell app, is a Medium crash-class finding and appears nowhere in the register.
  - F-02's refs omit BIND-19 and BIND-PS-02.
  - R1 ("no open Critical or High") lists F-01…F-04, F-07 and F-08, but omits F-09 and F-10, which the register rates High.
- **Proposed change:**
  - Add BIND-19 and the PS rows to §2.2.
  - Add BIND-19 and BIND-PS-02 to F-02's refs, and either consolidate BIND-PS-01 or list it.
  - In R1, either say "runtime defects (process findings are covered by P-gates)" or list F-09 and F-10.
- **Confidence:** High.

### M15. Claims are stronger than their retained evidence
- **Target:** README Q6; 01 E7, E9 and E4; BIND-19; 02 "Status" column.
- **Objection:**
  1. **README Q6 states native Windows behaviour as fact.** "Windows IME stays on in password fields" is INTERACT-09, "Plausible concern; needs native verification". "`pythonw` would open a console" is BIND-15, not run. Non-Latin shortcut key values are inferred on both OSes. The coordinator had an interactive winDEV session, and the `pythonw` check would have taken minutes.
  2. **The PowerShell evidence cannot be told apart from the Python evidence.** `evidence/logs/psb-powershell-10.out.txt` and `psb-powershell-20.err.txt` are byte-identical to the Python `thresh-10.out.txt` and `thresh-20.err.txt` (same MD5). That may be a deterministic coincidence, but no PS-specific output (`PROBE_FAIL` from `burst.ps1`) or job script is retained. As it stands, E4's evidence does not distinguish PowerShell.
  3. **Most of E7 and E9 is not retained.** E7 claims 20/60/120/250, but only 30 and 40 are retained. E9 (Windows SVG 500/700) has no retained Windows log. The PE-header script (`pe_stack.py`) and `depth-events.txt` (Windows Application Error events) were produced but not retained.
  4. **BIND-19 is stale.** It still says "Medium on Windows (arithmetic only)", although E3 and E4 reproduced the failure on Windows.
- **Proposed change:**
  - Tag the README Q6 items "(from code, not run)", or run them on winDEV.
  - Retain the missing logs and scripts, or change the Status cells to "Reproduced (coordinator, log not retained)".
  - Re-check the PS log copy.
  - Update BIND-19's confidence to cite E3 and E4.
- **Confidence:** High for the facts. Low on whether the identical logs are a copy error.

### M16. The severity rubric mixes likelihood into severity, and F-03's rationale is not supported by the docs
- **Target:** 02 header ("Severity means consequence … Critical = process crash … under realistic, documented usage"); F-03 (High, Likelihood Medium); INPUTSEC-02 ("a user-chosen image file … (an explicitly untrusted source per the docs)", Likelihood Medium-High).
- **Objection:**
  - By pure consequence, F-03 is a process abort like F-01 and would be Critical. The rubric only separates them by sneaking in realism, while also saying likelihood is "stated separately".
  - The docs never call images untrusted or trusted. They speak only of markup and text: `architecture.md:81` ("Insert only trusted markup … use the text operations for user-provided content") and `html-css-scope.md:157`.
  - The real exposure is narrower, though genuine. An app that shows a user-chosen image (through a resource bundle, or a `data:` URL set with `set_attribute`, which is not an HTML-inserting operation) crashes on a crafted roughly 10 KB file.
  - No benign SVG nests 500 or more groups, so the likelihood is Low, not Medium.
- **Proposed change:**
  - Rubric: "Critical = process crash or application failure triggered by benign content or ordinary use within documented limits. High = the same consequence but requiring crafted input, or serious functional, accessibility or release-integrity defects."
  - F-03: Severity High, Likelihood Low ("needs a crafted file; reachable only in apps that display user-chosen SVG"). Keep it in Phase 1 because the fix is a depth counter in `svg/prescan.rs`.
  - INPUTSEC-02: replace "an explicitly untrusted source per the docs" with "a source the docs do not require to be trusted".
- **Confidence:** High.

---

## B. Should consider

### S1. F-02 is more likely than the draft says: an OS theme change triggers it with no app involvement
- **Target:** 02 F-02 Likelihood; README "Production readiness"; 05 R3.
- **Evidence:**
  - Documents follow the desktop colour scheme by default: `dark = self.env.app_dark.or(self.env.system_dark)` (`document.rs:821`).
  - The host pushes every preference change into **every window's** document in one GUI turn (`host/document_native.rs:155-197`, `set_system_preferences` per window). On Windows the trigger is `WM_SETTINGCHANGE`; on Linux it is the appearance portal (`docs/guides/platforms.md:48`).
  - So a user toggling OS dark mode, or a scheduled night theme, can kill every running UI4ME app that styles about 15 or more controls with `prefers-color-scheme` and transitions. The burst multiplies with the window count.
  - The burst arithmetic matches common CSS. `burst2.py`'s 18 events per button are 6 longhands × 3 events, because `border-color` expands to 4 longhands. This also answers BIND's open question: there are no duplicates. Bootstrap's `.btn` (`color, background-color, border-color, box-shadow`) gives about 21 events per button.
- **Proposed change:**
  - Add to the F-02 Likelihood cell: "including OS theme switches, which documents follow by default".
  - Change README guidance to "avoid transitions on colour properties of many elements, or pin `set_environment(color_scheme=…)`".
  - Close BIND's open question with the longhand explanation.
- **Confidence:** High for the code path. Medium for the end-to-end trigger, which was not run.

### S2. Pull the cheap keyboard fixes into 1.0.2, and make their severities consistent
- **Target:** 07 3.3 (P1); 02 F-08 (High) vs INTERACT-05 within F-17 (Medium).
- **Objection:**
  - F-08 is High/High and S–M effort. Enter on `<a href>` (INTERACT-05) is a WCAG 2.1.1 Level A failure with S effort, yet rated Medium, below the AA-level F-08. Both are cheap, independent and user-visible.
  - F-13 (`sys.exit()` in a callback silently kills the worker) is S effort and a classic beginner pattern.
- **Proposed change:**
  - Add F-08 and INTERACT-05 to 1.0.2 as "if capacity allows", ahead of anything in 6.2.
  - Raise INTERACT-05 to High.
  - Add F-13 (BIND-01) to 1.0.2.
- **Confidence:** Medium. This is a priority judgement.

### S3. The source-manifest change: correct direction, but use the minimal option for 1.0.2
- **Target:** 07 6.1, 03 §3.5, RELEASE-02.
- **Assessment:**
  - Removing the per-commit `test_checked_manifest_matches_entire_source` is right. Nothing legitimate is lost, because the publish workflows already run `tools/verify_package.py` against the committed manifest (`publish.yaml:63`, `publish-powershell.yaml:40`).
  - The manifest's one real job is to prove that a build done *outside Git* (cibuildwheel and the Windows wheel from the extracted sdist) used the reviewed file set. The full redesign keeps that by injecting a generated manifest into the sdist.
  - `native_acceptance.py:89-115` computes its own `source_identity`, so it does not depend on the file.
  - The full redesign touches `verify_source.py`, `verify_linux_artifact.py`, `verify_package.py`, the publish workflow and the docs, on the release path, right before a release.
- **Proposed change:** For 1.0.2, use RELEASE-02's own "minimal alternative": skip the test outside publish workflows, and make clippy run regardless. Schedule "generate at preflight, stop committing" for 6.3 or 6.4 with build-once-promote.
- **Confidence:** High.

### S4. 1.0.2's main fix is Windows-only, yet the Windows wheel is still rebuilt after qualification
- **Target:** 07 Phase 1 vs F-11 (Phase 4).
- **Objection:** F-01's fix lives in the PE header and stack behaviour. Under the current process the published Windows wheel is a different build from the qualified one (RELEASE-03).
- **Proposed change:** Add to the 1.0.2 gate:
  - `verify_windows_artifact.py` asserts `SizeOfStackReserve` (07 1.1(a) hints at this; make it explicit).
  - A post-publish (or pre-promote) run of `depth.py 250`, `svgdepth.py 700` and `burst2.py 100` on the **exact published wheel**, on winDEV.
- **Confidence:** High.

### S5. The 1.1 effort estimate is low
- **Target:** README Q12 ("1.1: roughly one to two engineer-months").
- **Objection:**
  - Adding up 07's own ratings for 1.1 content gives about 7–9 items rated M (≤ 2 weeks each): 2.1, 3.1, 3.2 (INTERACT-03 says M–L), 6.3, 6.4, 5.1 (part), BIND-02 (M), and 5.2 (M–L). Add about ten S items (2.3).
  - That is closer to 3–5 engineer-months.
  - With a single maintainer (34 of 34 commits from one account) the calendar time is longer.
- **Proposed wording:** "roughly three to five engineer-months of work; for one maintainer, plan 1.1 as two minor releases".
- **Confidence:** Medium.

### S6. Phase labels and P-levels contradict each other
- **Target:** 07 Phase 1 header ("target: 1.0.2") vs 1.4 (P1); Phase 8.1 ("P1 for the two cheap fixes") inside "Longer-term, optional".
- **Proposed change:**
  - Move 1.4 to Phase 2.
  - Move the RENDER-05 cache-touch and RENDER-06 family memo into Phase 3 as named P1 items.
  - Consider splitting RENDER-05 out of F-16 as High for the documented long-log use case. A 1.0–1.8 s freeze per appended row after 8 unrelated updates makes that use case unusable.
- **Confidence:** Medium.

### S7. Making `host.hello` strict (HOST-17) would break the capability negotiation in 3.1
- **Target:** HOST-17 remediation ("allow-list `token`, `protocol_version`, `client` in hello") vs 03 §3.3 A and 07 3.1 (client capability in `host.hello`).
- **Objection:** A strict allow-list in a 1.0.x host would reject a hello from a 1.1 binding that carries the new capability field. Paired through `UI4ME_HOST_BINARY` or the PATH fallback (BIND-04), the result would be a failed connection instead of v1 fallback.
- **Proposed change:** In HOST-17, keep `host.hello` open to unknown keys, or explicitly accept a `capabilities` object, and document forward compatibility. Make every other zero-parameter method strict.
- **Confidence:** High.

### S8. Make R3 and R8 testable
- **R3:** "any volume of … events" is unbounded and contradicts any byte cap. Proposed wording: "bursts up to 10,000 events / 8 MiB in one GUI turn, and sustained 2,000 events/s for 60 s, with no disconnect and no dropped control notification."
- **R8:** Orca plus NVDA/Narrator journeys "with retained evidence" is heavy for a solo project. Proposed wording: "Automated AT-API contract checks (AT-SPI tree and UIA tree: roles, names, states, focus-changed and live-region events) on the release artifacts, plus a documented 15-minute manual NVDA and Orca checklist per minor release with notes attached."
- **Confidence:** Medium.

### S9. Keep release-engineering hygiene out of the gates for the "Production/Stable" claim
- **Target:** 05 §5.4 ("all R-gates … and P1–P13").
- **Objection:** P9 (SHA pins), P10 (SBOM and build provenance) and P11 (single-sourced version) are good practice. They do not change what a user experiences as stability.
- **Proposed change:** Gate the stable claim on P1, P2, P4, P5, P6, P7, P12 and P13. Recommend P8–P11.
- **Confidence:** Medium.

### S10. Recommend the classifier change directly
- **Target:** 07 7.1 ("reconsider 'Production/Stable'"); RELEASE-11.5 ("Consider `4 - Beta`").
- **Proposed wording:** "1.0.2 changes the PyPI classifier to `Development Status :: 4 - Beta` (and the Gallery tags/description accordingly) until the stable gates pass." The verdict in 05 already supports it, and hedging here undercuts it.
- **Confidence:** High.

### S11. Dimensions the audit does not cover

Ranked by likely seriousness.

1. **Windows High Contrast / forced colours.** `html-css-scope.md:57`: "`prefers-contrast` and `forced-colors` are not recognised; high contrast is observed and reported to the backend but no forced-colours palette is applied." Low-vision users on Windows contrast themes get author colours. This is a real accessibility gap that EN 301 549 §11.7 (user preferences) would flag, and it is missing from the accessibility rating and from R6–R8. **Medium.**
2. **Bidi, RTL and complex-script editing.** Scope claims "complex scripts, bidi, vertical text" (`html-css-scope.md:83`) and RTL controls. No area probed caret movement, selection or deletion in mixed-direction text, which is a classic weak spot of custom editors. Recommend one INTERACT probe. **Unknown, plausibly Medium.**
3. **Locked-down Windows environments**, which matter because the README says the library is suitable for "internal tools and controlled deployments":
   - AppLocker or WDAC default rules block unsigned executables in user-writable paths: per-user venvs under `%LOCALAPPDATA%` and CurrentUser module paths.
   - PowerShell Constrained Language Mode blocks `Add-Type`, which the module uses (`UI4ME.Core.psm1:31`), so the module cannot load.

   Worth one sentence in 08 next to Authenticode, and a platforms-table row. **Medium, reasoned.**
4. **Windows coverage is narrower than "Windows".** It is one Windows Server 2022 VM over RDP (typically 100% DPI, RDP clipboard and IME redirection). Say so in 01's "Not exercised" list next to Windows 10/11. **Low effort.**
5. **Non-ASCII user profiles and paths.** Not tested. The code looks careful (the PS module writes the manifest as UTF-8 without a BOM, `psm1:1511, 1647`). The audit already created a winDEV user, so a one-off test with a non-ASCII user name is cheap. **Low.**
6. **No telemetry: verified, worth stating.** `Cargo.lock` has no HTTP client or telemetry crates, and the Python and PowerShell bindings contain no network calls (grep). This is a positive privacy statement the audit can make.
7. **Portal-missing dialogs.** `dialog.*` behaviour without xdg-desktop-portal (non-KDE or minimal sessions) is untested. BIND-14 only covers the startup message.
8. **Bus factor.** A single maintainer with mostly AI-written code is a risk to the roadmap itself (see S5). 06 hints at it ("nobody else has read most of the code") but does not name it.
9. **Already handled, so no action:** font licensing (OFL notice verified), PerMonitorV2 DPI and `longPathAware` in `host.manifest:6-8`, and SmartScreen, which is mostly moot for pip/PSGallery installs because no Mark-of-the-Web is set. One trivial finding: `host.manifest:3` still declares `assemblyIdentity version="0.1.0.0"`; add it to RELEASE-07's literal list.

### S12. Tone and accuracy in the executive text
- **README, executive paragraph 1.** "Much of the implementation is better than its age (three days of public history) would suggest." The history is private, not public. The code was imported as "supplied UI4ME Core 0.2 source" (`05b8f8f`, +480k lines), so its age is unknown. The phrase is flattering and unsupported. **Delete it.**
- **README.** "1.0.0 was published while its Windows CI was failing." In fact the publish was dispatched 21 s after CI started, CI failed about 20 minutes later, and the failures were test-only (RELEASE-01 says the artifacts were "probably not defective"). Reword: "1.0.0 was published 21 s after CI started, without waiting for it; Windows CI later failed (test-only failures)."
- **README.** "every non-release pull request fails CI by design." It is a side effect, not a design. Reword: "as a side effect of a committed source-manifest check".
- **README.** "the published Windows binary is not the one that was qualified." Add "(built from identical source)" so the reader does not infer a code difference.
- **03 §3.3 D.** "cheaper and more valuable than any amount of additional lab qualification" is an absolute claim. A virtual clock cannot test IME, AT, DPI or input. Reword: "cheaper than lab qualification for frame-driven behaviour".
- **01 "Model routing" paragraph.** Unverifiable by the audit's own admission, and irrelevant to the reader's decisions. Move it to an appendix or delete it.
- **01 "Lab assets left".** "(see the final summary of the session)" is not durable. State the final state of Xvfb, xfreerdp and kwin in the document.
- **Confidence:** High.

### S13. Rate process findings on their own scale
- **Target:** 02 F-09 (High).
- **Objection:** Red-by-default CI is a serious process smell, but it has no user-facing consequence by itself. F-10 (publishing from untested commits) is the release-integrity risk.
- **Proposed change:** F-09 becomes Medium (process), or put process findings in their own table so the ranked list of runtime risks reads cleanly.
- **Confidence:** Medium.

---

## C. Disagreements to record, not necessarily change

### D1. F-01 stays Critical, but with lower likelihood and context
Under the stated rubric, an abort at depth 40 when the documented limit is 256 is Critical. But the project's own most complex page (`examples/python/showcase/ui/index.html`, and its PowerShell twin) reaches depth **14**. Other examples reach 5 to 10 (measured with `<scratch>/review/maxdepth.py`). Typical form apps therefore have about 2× headroom for `<div>` and less for `fieldset` or tables. Recommend Likelihood "Low-Medium (complex or generated layouts; the bundled showcase reaches depth 14)", and a README note so readers do not assume all Windows apps are crashing. **Confidence:** High for the measurement.

### D2. F-02 stays Critical
It is Critical, and if anything understated (S1). No change to the rating.

### D3. F-03: I would rate it Medium
Under a model where the backend is trusted and only crafted user-chosen files trigger it, I would rate F-03 Medium. High is acceptable with the rubric fix in M16. Either way it belongs in 1.0.2 because the fix is trivial.

### D4. The full event-contract redesign may be more than a 1.x project needs
Type-level host-side subscription is worth doing in 1.1. Beyond saving GUI-thread time, it matters a lot for PowerShell, whose pump costs about 0.65 ms per *unsubscribed* event (BIND-PS-02), or about 1.2 s per theme switch at N = 100. Reliable and event lanes, per-element subscription, `seq` and `document.events_lost` should wait for a measured case where the host must drop discrete events. With a byte-capped queue and a never-drop rule, that case may never come. The audit presents all four properties as necessary "before growth".

### D5. F-07 and F-08 as High
Both are fair. F-07's "Likelihood High (centred modal pattern)" depends on hand-rolled `transform: translate(-50%, -50%)` modals. The `<dialog>` element is affected by the `position: fixed` half (INTERACT-02) after scrolling, not by the transform half. Consider "Medium-High".

### D6. `/STACK` mechanics
The mechanism is correct. A 16 or 32 MiB reserve on x64 is harmless: it reserves address space only, commit stays small, and threads created by system DLLs with a zero stack size inherit the larger reserve with no practical cost. It has no special interaction with winit beyond the fact that everything runs on that thread, which is the point. My objection is only sizing and the Linux gap (M3) and the factual error about alternatives (M4).

### D7. Stopping the committed manifest entirely is fine long-term
In Git, the tree hash is the manifest. The audit correctly keeps an sdist-embedded manifest for builds done outside Git. My only disagreement is timing (S3).

---

## D. What the draft got clearly right
- **Issue #2 root cause (F-04).** Font identity borrowed from parley's pruning cache, confirmed in code: `SourceCache::default()` at `environment.rs:269`, an unbounded `system_blobs` copy at `:323-328`. Discriminating variants (600 vs 700 weight) and heaptrack stacks make this a model diagnosis, and the fix is small and correct.
- **Issue #1 mechanism (F-02).** Every event type is registered as a root capture listener (`document.rs:347-405`). The 128-message `sync_channel` plus `try_send` → `disconnect` (`transport.rs:17, 215, 273`; `host/mod.rs:442-447`) and the two-write framing (`transport.rs:305-306`) are all confirmed. The correction of issue #1's "Linux survives" claim, and the observation that headless emits zero transition events, are valuable.
- **Self-correction.** The coordinator corrected specialists where they were wrong (INPUTSEC-02's Windows worker stack; Rust gives spawned threads 2 MiB on every OS) and retracted its own wrong early hypothesis. That is the right audit behaviour.
- **The asymmetry in the SVG caps.** Inline SVG is capped at depth 48 (`inline_svg.rs:33`); image SVG has no nesting cap.
- **The PR #3 analysis.** Lifecycle notifications on the drop path, and responses still disconnecting, are correctly identified.
- **Release-process facts.** The hash mismatch for the Windows rebuild-after-qualify, the PSGallery 1.0.0 commit mismatch, and the manifest change-detector that hides clippy are concrete, checkable and important.
- **The non-recommendations (08).** Keep the two processes, declarative default actions, pixel probes over goldens, no history rewrite, no MSRV job, no cp-ABI wheel matrix. All sound, and worth stating.
- **The "Not exercised" section and the honest labelling** of headless-only evidence (`native_desktop_qualified: false`).
