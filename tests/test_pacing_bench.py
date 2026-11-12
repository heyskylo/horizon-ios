import os
import unittest
from pathlib import Path

from tools import lab
from tools.pacing_bench import capture_frames, frame_line, slowest_frames, stats

FIXTURE = lab.ROOT / "tests/fixtures/pacing_slow_frames.hzperf"
# The five slow frames of the fixture, written by hand in crates/horizon-perf/tests/pacing_fixture.rs.
SLOWEST = [
    "slow frame 130.0 ms, cpu 18.0 ms: submit 110.0/1.0, tick 12.0",
    "slow frame 62.5 ms, cpu 60.0 ms: tick 50.0, update_world 8.0",
    "slow frame 45.0 ms, cpu 12.0 ms: render 40.0/10.0, chunk_upload 25.0/6.0",
]


def perf_binary():
    """horizon-perf as built by `cargo build -p horizon-perf`, or None."""
    named = os.environ.get("HORIZON_PERF_BIN")
    if named:
        return Path(named)
    for profile in ("fast", "debug", "release"):
        path = lab.cargo_target() / profile / "horizon-perf"
        if path.exists():
            return path
    return None


class StatsTests(unittest.TestCase):
    def test_lows_percentiles_and_hitch_counts(self):
        # 998 frames of 5 ms, one of 40 ms and one of 100 ms.
        frames = [5.0] * 998 + [40.0, 100.0]
        s = stats(frames)
        self.assertEqual(s["frames"], 1000)
        self.assertEqual(s["p50_ms"], 5.0)
        self.assertEqual(s["p99_ms"], 5.0)
        self.assertEqual(s["p999_ms"], 40.0)
        self.assertEqual(s["max_ms"], 100.0)
        # The slowest 1% (10 frames): 8 x 5 + 40 + 100 = 180 ms, 18 ms each.
        self.assertAlmostEqual(s["low_1pct_fps"], round(1000 / 18.0, 1))
        # The slowest 0.1% is the single 100 ms frame.
        self.assertEqual(s["low_01pct_fps"], 10.0)
        self.assertEqual(s["over_33ms"], 2)
        self.assertEqual(s["over_2x_median"], 2)

    def test_no_frames(self):
        self.assertEqual(stats([]), {"frames": 0})


class SlowFrameTests(unittest.TestCase):
    def test_a_frame_line_shows_cpu_only_for_spans_that_waited_and_drops_short_spans(self):
        row = {"wall_ms": 130.0, "cpu_ms": 18.0,
               "spans": [["submit", 0, 110.0, 1.0], ["tick", 0, 12.0, 11.0], ["hud", 0, 0.4, 0.4]]}
        self.assertEqual(frame_line(row), "slow frame 130.0 ms, cpu 18.0 ms: submit 110.0/1.0, tick 12.0")

    def test_the_slowest_frames_over_the_threshold_come_first_and_are_counted(self):
        rows = [{"wall_ms": w, "cpu_ms": 1.0, "before_capture": False, "spans": [["render", 0, w, w]]}
                for w in (30.0, 90.0, 60.0, 45.0, 20.0, 5.0)]
        lines, count = slowest_frames({"rows": rows}, 2, 20.0)
        self.assertEqual(count, 4)
        self.assertEqual([line.split(" ms")[0] for line in lines], ["slow frame 90.0", "slow frame 60.0"])

    def test_frames_from_before_the_capture_are_left_out(self):
        def row(wall, before):
            return {"wall_ms": wall, "cpu_ms": 1.0, "before_capture": before, "spans": [["load", 0, wall, wall]]}
        lines, count = slowest_frames({"rows": [row(900.0, True), row(40.0, False)]}, 5, 20.0)
        self.assertEqual(count, 1)
        self.assertEqual(lines, ["slow frame 40.0 ms, cpu 1.0 ms: load 40.0"])

    @unittest.skipUnless(perf_binary(), "build horizon-perf (cargo build -p horizon-perf) or set HORIZON_PERF_BIN")
    def test_a_fixture_capture_gives_its_slowest_frames_with_their_spans(self):
        reply = capture_frames(perf_binary(), FIXTURE)
        lines, count = slowest_frames(reply, 3, 20.0)
        self.assertEqual(count, 4)
        self.assertEqual(lines, SLOWEST)


if __name__ == "__main__":
    unittest.main()
