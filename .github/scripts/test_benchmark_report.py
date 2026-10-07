import json
from pathlib import Path
import tempfile
import unittest
from textwrap import dedent

from benchmark_report import read_revision, report


def estimate(mean):
    return {
        "point_estimate": mean,
        "confidence_interval": {"lower_bound": mean - 1, "upper_bound": mean + 1},
    }


class BenchmarkReportTests(unittest.TestCase):
    def test_compatibility_is_per_workload(self):
        base = (
            {"same": estimate(10), "changed": estimate(20), "removed": estimate(5)},
            {"same": ("v1", "a"), "changed": ("v1", "b"), "removed": ("v1", "c")},
        )
        head = (
            {"same": estimate(12), "changed": estimate(10), "new": estimate(8)},
            {"same": ("v1", "a"), "changed": ("v2", "b"), "new": ("v1", "d")},
        )
        output = report(base, head)
        self.assertIn(
            "| `same` | 10.0 (9.0–11.0) | 12.0 (11.0–13.0) | +20.00% |", output
        )
        self.assertIn("Inputs or measurement contract changed", output)
        self.assertIn("New workload", output)
        self.assertIn("Removed workload", output)
        self.assertNotIn("-50.00%", output)

    def test_changed_inputs_are_not_compared(self):
        output = report(
            ({"case": estimate(10)}, {"case": ("v1", "a")}),
            ({"case": estimate(20)}, {"case": ("v1", "b")}),
        )
        self.assertIn("Inputs or measurement contract changed", output)
        self.assertNotIn("+100.00%", output)

    def test_legacy_results_remain_visible_without_comparison(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            self.write_result(root)
            estimates, workloads = read_revision(root)
            self.assertEqual(estimates, {"case": estimate(10)})
            self.assertEqual(workloads, {})
            output = report((estimates, workloads), (estimates, {"case": ("v1", "a")}))
            self.assertIn("Compatibility manifest unavailable", output)
            self.assertNotIn("+0.00%", output)

    def test_missing_or_unrecorded_measurements_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(ValueError, "No benchmark measurements"):
                read_revision(root)
            self.write_result(root)
            (root / "workloads.toml").write_text(
                dedent("""\
                    [[workloads]]
                    id = "different"
                    contract = "v1"
                    inputs = "a"
                    """)
            )
            with self.assertRaisesRegex(ValueError, "differs from measured cases"):
                read_revision(root)

    @staticmethod
    def write_result(root):
        path = root / "criterion" / "case" / "measured"
        path.mkdir(parents=True)
        (path / "benchmark.json").write_text(json.dumps({"full_id": "case"}))
        (path / "estimates.json").write_text(json.dumps({"mean": estimate(10)}))


if __name__ == "__main__":
    unittest.main()
