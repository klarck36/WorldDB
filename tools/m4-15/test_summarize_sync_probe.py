import errno
import unittest

from summarize_sync_probe import build_report, parse_rows, percentile


class SyncProbeSummaryTests(unittest.TestCase):
    def test_percentiles_use_nearest_rank(self):
        self.assertEqual(percentile([100, 10, 20], 0.50), 20)
        self.assertEqual(percentile([100, 10, 20], 0.95), 100)
        self.assertIsNone(percentile([], 0.95))

    def test_report_keeps_errors_separate_from_success_latency(self):
        rows = parse_rows(
            [
                {"operation": "fsync", "iteration": "0", "return": "0", "errno": "0", "elapsed_ns": "10"},
                {"operation": "fsync", "iteration": "1", "return": "0", "errno": "0", "elapsed_ns": "20"},
                {"operation": "fsync", "iteration": "2", "return": "-1", "errno": str(errno.EIO), "elapsed_ns": "3"},
                {"operation": "fullfsync", "iteration": "0", "return": "0", "errno": "0", "elapsed_ns": "50"},
                {"operation": "fullfsync", "iteration": "1", "return": "0", "errno": "0", "elapsed_ns": "150"},
                {"operation": "fault_fsync_ebadf", "iteration": "0", "return": "-1", "errno": str(errno.EBADF), "elapsed_ns": "1"},
                {"operation": "fault_fullfsync_ebadf", "iteration": "0", "return": "-1", "errno": str(errno.EBADF), "elapsed_ns": "1"},
            ]
        )

        report = build_report(rows)
        fsync = report["measurements"]["fsync"]
        self.assertEqual(fsync["successes"], 2)
        self.assertEqual(fsync["failures"], [{"return": -1, "errno": errno.EIO, "count": 1}])
        self.assertEqual(fsync["successful_latency_ns"]["median"], 15)
        self.assertEqual(fsync["successful_latency_ns"]["p95"], 20)
        self.assertTrue(report["fault_controls"]["fault_fsync_ebadf"]["expected_ebadf"])
        self.assertTrue(report["fault_controls"]["fault_fullfsync_ebadf"]["expected_ebadf"])

    def test_missing_measurement_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "missing measurement rows: fullfsync"):
            build_report(
                parse_rows(
                    [
                        {"operation": "fsync", "iteration": "0", "return": "0", "errno": "0", "elapsed_ns": "10"}
                    ]
                )
            )


if __name__ == "__main__":
    unittest.main()
