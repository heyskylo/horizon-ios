import datetime as dt
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools" / "ci"))
import runner_watch as rw  # noqa: E402

NOW = dt.datetime(2026, 11, 10, 12, 0, tzinfo=dt.timezone.utc)
LIGHT = ["self-hosted", "linux", "horizon-light"]
GAME = ["self-hosted", "linux", "horizon-game"]
MAC_NIGHTLY = ["self-hosted", "macOS", "horizon-nightly"]


def job(labels, hours_ago, status="queued", name="j"):
    created = (NOW - dt.timedelta(hours=hours_ago)).strftime("%Y-%m-%dT%H:%M:%SZ")
    return {"name": name, "labels": labels, "status": status, "created_at": created, "html_url": f"https://x/{name}"}


class WaitingJobs(unittest.TestCase):
    def test_a_job_waiting_past_the_limit_in_an_idle_pool_counts(self):
        waiting = rw.waiting_jobs([job(LIGHT, 3, name="triage")], NOW, 2)
        self.assertEqual([j["name"] for j in waiting], ["triage"])
        self.assertAlmostEqual(waiting[0]["waited_h"], 3)

    def test_recent_jobs_github_hosted_jobs_and_finished_jobs_do_not(self):
        jobs = [job(LIGHT, 1), job(["ubuntu-latest"], 9), job(GAME, 9, status="completed")]
        self.assertEqual(rw.waiting_jobs(jobs, NOW, 2), [])

    def test_a_busy_pool_is_not_offline_but_another_idle_pool_still_is(self):
        jobs = [job(GAME, 7, name="parity"), job(GAME, 0.5, status="in_progress"), job(MAC_NIGHTLY, 5, name="mac")]
        self.assertEqual([j["name"] for j in rw.waiting_jobs(jobs, NOW, 2)], ["mac"])


class Plan(unittest.TestCase):
    def test_opens_once_then_updates_then_closes(self):
        waiting = rw.waiting_jobs([job(LIGHT, 3), job(LIGHT, 5, name="claim"), job(MAC_NIGHTLY, 4)], NOW, 2)
        action, text = rw.plan(waiting, None, NOW)
        self.assertEqual(action, "open")
        self.assertIn("| `horizon-light` | 2 | 5 h | [claim](https://x/claim) |", text)
        self.assertIn("SELF_HOSTED_CI", text)  # the fix for the light pool is named
        self.assertIn("no Nightly is published", text)  # and what the Mac pool stops
        self.assertNotIn("parity job waits", text)  # pools with nothing waiting are left out
        self.assertEqual(rw.plan(waiting, 42, NOW)[0], "update")
        self.assertEqual(rw.plan([], 42, NOW)[0], "close")
        self.assertEqual(rw.plan([], None, NOW), (None, ""))

    def test_an_unknown_label_set_is_still_reported_by_its_labels(self):
        waiting = rw.waiting_jobs([job(["self-hosted", "gpu-box"], 3)], NOW, 2)
        self.assertIn("| `gpu-box` | 1 |", rw.plan(waiting, None, NOW)[1])


if __name__ == "__main__":
    unittest.main()
