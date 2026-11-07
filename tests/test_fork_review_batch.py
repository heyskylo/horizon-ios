import sys
import unittest
from datetime import datetime, timedelta, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "fork"))
import review_batch as rb  # noqa: E402

NOW = datetime(2026, 11, 7, 12, 0, tzinfo=timezone.utc)


def run(conclusion="SUCCESS", status="COMPLETED", at="2026-11-07T10:00:00Z"):
    return {"__typename": "CheckRun", "status": status, "conclusion": conclusion, "completedAt": at}


def pr(n, checks, draft=False, conflict=False, labels=(), head="h"):
    return rb.Pr(n, f"PR {n}", "dev", f"{head}{n}", draft, conflict, set(labels), checks)


class Classify(unittest.TestCase):
    def test_green_checks_make_it_ready_since_the_last_one(self):
        why, since = rb.classify(pr(1, [run(at="2026-11-07T09:00:00Z"), run("SKIPPED", at="2026-11-07T10:30:00Z")]))
        self.assertEqual((why, since), ("ready", datetime(2026, 11, 7, 10, 30, tzinfo=timezone.utc)))

    def test_each_reason_to_wait(self):
        self.assertEqual(rb.classify(pr(1, [run()], draft=True))[0], "draft")
        self.assertEqual(rb.classify(pr(1, [run()], conflict=True))[0], "conflict")
        self.assertEqual(rb.classify(pr(1, [run()], labels=["needs-changes"]))[0], "needs-changes")
        self.assertEqual(rb.classify(pr(1, [run(), run(status="IN_PROGRESS", conclusion=None)]))[0], "checks pending")
        self.assertEqual(rb.classify(pr(1, [run(), run("FAILURE")]))[0], "checks failing")
        self.assertEqual(rb.classify(pr(1, []))[0], "no checks")

    def test_a_failing_status_context_blocks(self):
        ctx = {"__typename": "StatusContext", "state": "ERROR", "startedAt": "2026-11-07T10:00:00Z"}
        self.assertEqual(rb.classify(pr(1, [run(), ctx]))[0], "checks failing")


class Due(unittest.TestCase):
    def ready(self, ages_h):
        return [(pr(i + 1, []), NOW - timedelta(hours=a)) for i, a in enumerate(ages_h)]

    def test_five_fresh_ready_pull_requests_make_a_batch_oldest_first(self):
        batch = rb.due(self.ready([1, 3, 2, 0.5, 1.5]), {}, NOW)
        self.assertEqual([p.number for p in batch], [2, 3, 5, 1, 4])

    def test_four_young_ones_wait_but_one_six_hours_old_is_due(self):
        self.assertEqual(rb.due(self.ready([1, 2, 3, 4]), {}, NOW), [])
        self.assertEqual([p.number for p in rb.due(self.ready([1, 6]), {}, NOW)], [2, 1])

    def test_signalled_heads_are_not_signalled_again_but_a_new_head_is(self):
        ready = self.ready([7])
        self.assertEqual(rb.due(ready, {"1": "h1"}, NOW), [])
        self.assertEqual([p.number for p in rb.due(ready, {"1": "old"}, NOW)], [1])


if __name__ == "__main__":
    unittest.main()
