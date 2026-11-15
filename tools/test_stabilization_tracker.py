import copy
import unittest

import stabilization_tracker as tracker


class TrackerTests(unittest.TestCase):
    def setUp(self):
        self.doc = tracker.load()

    def test_tracker_is_valid_and_matrix_current(self):
        self.assertEqual(tracker.problems(self.doc), [])
        self.assertEqual(tracker.MATRIX.read_text(encoding="utf-8"), tracker.render(self.doc))

    def test_all_thirty_consolidated_findings(self):
        self.assertEqual(len(self.doc["findings"]), 30)
        self.assertEqual(len(self.doc["gates"]), 28)
        self.assertEqual([t["id"] for t in self.doc["tests"]], [f"T{n}" for n in range(1, 17)])

    def test_fixed_requires_evidence(self):
        doc = copy.deepcopy(self.doc)
        row = doc["findings"][0]
        row.update(status="FIXED_VERIFIED", commits=[], tests=[])
        row["verification"].update(linux="pending", windows="pending", required="both")
        found = "\n".join(tracker.problems(doc))
        for needle in ("without a commit", "regression tests", "Linux verification", "Windows verification"):
            self.assertIn(needle, found)

    def test_deferral_requires_justification(self):
        doc = copy.deepcopy(self.doc)
        doc["specialist"][0].update(status="DEFERRED", notes="")
        self.assertTrue(any("justification" in p for p in tracker.problems(doc)))


if __name__ == "__main__":
    unittest.main()
