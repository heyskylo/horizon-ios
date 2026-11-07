import unittest

from tools.ci import parity_issues as issues


class ParityIssuesTests(unittest.TestCase):
    def test_results_merge_shards_and_skip_earlier_runs(self):
        state = {"harnesses": {
            "food": {"status": "pass", "finished": "2026-10-15T03:10:00"},
            "datafix#1": {"status": "pass", "finished": "2026-10-15T03:20:00"},
            "datafix#2": {"status": "fail", "finished": "2026-10-15T03:21:00"},
            "light": {"status": "fail", "finished": "2026-10-14T03:30:00"},
            "copied": {"status": "pass", "finished": "2026-10-15T03:30:00", "shared": True},
        }}
        self.assertEqual(issues.results(state, "2026-10-15T03:00:00"), {"food": "pass", "datafix": "fail"})

    def test_plan_opens_comments_and_closes(self):
        now = {"a": "fail", "b": "fail", "c": "pass", "d": "fail", "e": "pass"}
        previous = {"a": "pass", "b": "fail", "c": "fail", "e": "pass"}
        self.assertEqual(issues.plan(now, previous, {"b": 7, "c": 8}),
                         [("open", "a"), ("comment", "b"), ("close", "c")])
        # A harness that never passed in an audit (d) gets no issue; one that keeps passing (e) nothing.
        self.assertEqual(issues.plan(now, {}, {}), [])

    def test_many_new_failures_at_once_are_one_report(self):
        many = [("open", f"h{i}") for i in range(issues.MASS_FAILURE + 1)]
        self.assertEqual(issues.mass_failure(many + [("comment", "x"), ("close", "y")]),
                         [f"h{i}" for i in range(issues.MASS_FAILURE + 1)])
        # A few real regressions still get one issue each.
        self.assertEqual(issues.mass_failure([("open", "a"), ("open", "b"), ("comment", "c")]), [])


if __name__ == "__main__":
    unittest.main()
