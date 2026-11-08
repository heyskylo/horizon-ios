import assert from "node:assert/strict";
import { test } from "node:test";
import { abMetrics, metricUnit, ordinals, phaseLabel, signed, spreads, stamp, verdictTone } from "./bench.ts";
import type { BenchAb, BenchFrames } from "./types.ts";

function block(phase: string, repeat: number, p50: number, p99: number): BenchFrames {
  return {
    phase,
    repeat,
    frames: 100,
    seconds: 1,
    avg_fps: 100,
    low_1pct_fps: 50,
    low_01pct_fps: 40,
    p50_ms: p50,
    p95_ms: p99,
    p99_ms: p99,
    p999_ms: p99,
    max_ms: p99,
    histogram: [],
  };
}

test("every bar is scaled by the slowest 99th percentile of the report", () => {
  const rows = spreads([block("a", 0, 5, 10), block("b", 0, 10, 20), block("c", 0, 4, 8)]);
  assert.deepEqual(
    rows.map((r) => [r.typical, r.tail]),
    [
      [0.25, 0.5],
      [0.5, 1],
      [0.2, 0.4],
    ],
  );
});

test("a report with no time at all has no bar wider than nothing", () => {
  assert.deepEqual(spreads([block("a", 0, 0, 0)]).map((r) => [r.typical, r.tail]), [[0, 0]]);
  assert.deepEqual(spreads([]), []);
});

test("a metric's unit is the suffix of its name, and per-second beats seconds", () => {
  assert.equal(metricUnit("startup.title_ms"), "ms");
  assert.equal(metricUnit("memory.rss_peak_mb"), "MiB");
  assert.equal(metricUnit("fps.hold.avg_fps"), "FPS");
  assert.equal(metricUnit("chunks.village.generated_per_s"), "/s");
  assert.equal(metricUnit("run_s"), "s");
  assert.equal(metricUnit("chunks.village.generated_count"), "");
});

const ab: BenchAb = {
  switch: "shadows",
  a: "off",
  b: "on",
  pairs: 6,
  block_s: 10,
  warmup_s: 2,
  first_block_outside_range: false,
  result: {},
};

test("an A/B block is named by the setting it ran with, any other phase by itself", () => {
  assert.equal(phaseLabel("ab.a", ab), "shadows off");
  assert.equal(phaseLabel("ab.b", ab), "shadows on");
  assert.equal(phaseLabel("hold", ab), "hold");
  assert.equal(phaseLabel("ab.a", null), "ab.a");
});

test("blocks are numbered within their phase, not by the run's block index", () => {
  // An A/B run numbers its blocks 0..5 across both arms: the third ab.b block has repeat 5.
  const blocks = [block("ab.b", 0, 1, 1), block("ab.a", 1, 1, 1), block("ab.a", 2, 1, 1), block("ab.b", 3, 1, 1), block("ab.b", 4, 1, 1), block("ab.a", 5, 1, 1)];
  assert.deepEqual(ordinals(blocks), [
    { n: 1, total: 3 },
    { n: 1, total: 3 },
    { n: 2, total: 3 },
    { n: 2, total: 3 },
    { n: 3, total: 3 },
    { n: 3, total: 3 },
  ]);
  assert.deepEqual(ordinals([block("travel", 0, 1, 1)]), [{ n: 1, total: 1 }]);
});

test("a report's creation time is shown to the minute in UTC", () => {
  assert.equal(stamp("2026-11-08T12:34:56Z"), "2026-11-08 12:34 UTC");
  assert.equal(stamp("yesterday"), "yesterday");
});

test("a change carries its sign, and a change that rounds to nothing carries none", () => {
  assert.equal(signed(1.234, 2), "+1.23");
  assert.equal(signed(-0.5, 1), "-0.5");
  assert.equal(signed(0.001, 2), "0.00");
  assert.equal(signed(-0.001, 2), "0.00");
  assert.equal(signed(0, 1), "0.0");
});

test("A/B results list the frame time first, then CPU, then GPU", () => {
  const names = abMetrics({ gpu_p50_ms: 1, zzz: 4, cpu_p50_ms: 2, aaa: 5, frame_p50_ms: 3 }).map(([name]) => name);
  assert.deepEqual(names, ["frame_p50_ms", "cpu_p50_ms", "gpu_p50_ms", "aaa", "zzz"]);
});

test("only a faster or slower verdict has a colour", () => {
  assert.equal(verdictTone("faster"), "better");
  assert.equal(verdictTone("slower"), "worse");
  assert.equal(verdictTone("within_noise"), "same");
  assert.equal(verdictTone("negligible"), "same");
  assert.equal(verdictTone("too_few_pairs"), "same");
});
