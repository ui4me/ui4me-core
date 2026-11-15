"""T3 (F-12, roadmap 2.1): the headless host runs the frame pump and a deterministic virtual clock.

Real host in headless test mode, real IPC, a raw-protocol backend (`headless_backend.py`).
Needs a built host (`cargo build -p ui4me-host`); skipped without one. `UI4ME_HOST_BINARY`
selects a specific host binary (for example a baseline build, to show these tests fail there).
"""
from __future__ import annotations

from collections import Counter
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))
from run_example import checkout_environment, manifest  # noqa: E402

BACKEND = ROOT / "tools" / "headless_backend.py"


def built_host() -> str | None:
    explicit = os.environ.get("UI4ME_HOST_BINARY")
    if explicit:
        return explicit if Path(explicit).is_file() else None
    name = "ui4me-host.exe" if os.name == "nt" else "ui4me-host"
    # The most recently built profile: a stale release build must not shadow a fresh
    # debug build (tools/verify.py builds debug before the Python stage).
    found = [ROOT / "target" / mode / name for mode in ("release", "debug")]
    found = [candidate for candidate in found if candidate.is_file()]
    return str(max(found, key=lambda candidate: candidate.stat().st_mtime)) if found else None


CLEAN = {key: value for key, value in os.environ.items() if not key.startswith("UI4ME_")}


def run_scenario(test, scenario, *args, app_id, timeout=240, extra_env=None):
    """Run the host headless with `headless_backend.py SCENARIO` and return (process, report)."""
    folder = tempfile.TemporaryDirectory(prefix="ui4me-headless-")
    test.addCleanup(folder.cleanup)
    base = Path(folder.name)
    report = base / "report.json"
    app = base / "app.toml"
    app.write_text(manifest([str(report), scenario, *map(str, args)], app_id=app_id,
                            backend_path=BACKEND), encoding="utf-8")
    env = checkout_environment(CLEAN)
    env["UI4ME_TEST_MODE"] = "1"
    if os.name != "nt":
        runtime = base / "runtime"
        runtime.mkdir(mode=0o700)
        env["XDG_RUNTIME_DIR"] = str(runtime)
    env.update(extra_env or {})
    process = subprocess.run([built_host(), "run", "--app", str(app), "--headless"], env=env,
                             capture_output=True, timeout=timeout, check=False)
    data = json.loads(report.read_text(encoding="utf-8")) if report.is_file() else {}
    return process, data


@unittest.skipUnless(built_host(), "build the host first: cargo build -p ui4me-host")
class HeadlessClockTests(unittest.TestCase):
    def test_t3_transition_burst_on_the_virtual_clock(self):
        controls, rounds = 100, 6
        process, report = run_scenario(self, "burst", controls, rounds,
                                       app_id="org.ui4me.test.HeadlessBurst")
        self.assertEqual(process.returncode, 0, (report.get("error"), process.stderr.decode(errors="replace")[-4000:]))
        self.assertTrue(report.get("ok"), report.get("error"))
        self.assertIs(report["capabilities_test_clock"], True)
        self.assertEqual(len(report["rounds"]), rounds, "the backend survived every round")
        per_round = []
        for index, round_ in enumerate(report["rounds"]):
            with self.subTest(round=index):
                self.assertEqual(round_["advanced"]["frames"], 13)
                self.assertEqual(round_["text"], {"text": "x"}, "the connection is alive")
                events = round_["events"]
                kinds = Counter(kind for kind, _, _ in events)
                # Every control transitions three properties on each scheme switch.
                for kind in ("transitionrun", "transitionstart", "transitionend"):
                    self.assertEqual(kinds[kind], 3 * controls, kinds)
                self.assertEqual(kinds["transitioncancel"], 0, kinds)
                # Ordering: per control and property, run < start < end.
                seen = {}
                for kind, element, prop in events:
                    seen.setdefault((element, prop), []).append(kind)
                self.assertEqual(len(seen), 3 * controls)
                for key, sequence in seen.items():
                    self.assertEqual(sequence, ["transitionrun", "transitionstart", "transitionend"],
                                     key)
                per_round.append([kind for kind, _, _ in events])
        # The virtual clock is deterministic: every round produced the same sequence of types.
        self.assertTrue(all(sequence == per_round[0] for sequence in per_round), per_round)

    @unittest.skipUnless(sys.platform.startswith("linux"), "RSS is sampled from /proc")
    def test_t3_ten_thousand_writes_without_reads_keep_memory_bounded(self):
        process, report = run_scenario(self, "soak", 10_000, app_id="org.ui4me.test.HeadlessSoak")
        self.assertEqual(process.returncode, 0, (report.get("error"), process.stderr.decode(errors="replace")[-4000:]))
        self.assertTrue(report.get("ok"), report.get("error"))
        samples = dict((ops, rss) for ops, rss in report["rss_kib"])
        self.assertEqual(report["final_text"], {"text": "value 9999"})
        # Allocator warm-up settles within the first few thousand writes; after that the host
        # must stay flat. RENDER-04 baseline: ~0.4 KiB per unflushed write, i.e. ~2.4 MiB over
        # these 6,000 (measured 2,460 KiB before the frame pump; 136 KiB with it).
        growth = samples[10_000] - samples[4_000]
        self.assertLess(growth, 1024, f"RSS grew {growth} KiB over 6,000 writes: {samples}")


    def test_r5c_turns_stay_short_while_1000_queued_commands_drain(self):
        """Gate R5(c) (HOST-05/06): 1,000 pipelined commands (writes and forced-layout reads)
        never hold the GUI thread for 50 ms in one turn; the pump yields and re-wakes."""
        process, report = run_scenario(self, "drain", 1000, 20, app_id="org.ui4me.test.Drain")
        self.assertEqual(process.returncode, 0, (report.get("error"), process.stderr[-2000:]))
        self.assertTrue(report.get("ok"), report.get("error"))
        pump = report["pump"]
        # Measured (debug host): 230 ms with only the 64-message budget, 18 ms with 8 ms.
        self.assertLess(pump["longest_turn_ms"], 50, pump)
        self.assertGreater(pump["budget_yields"], 0, pump)


if __name__ == "__main__":
    unittest.main()
