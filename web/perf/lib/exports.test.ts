import assert from "node:assert/strict";
import { test } from "node:test";
import { exportsFor } from "./exports.ts";
import type { OptionsInfo } from "./types.ts";

const options = (timeline: boolean): OptionsInfo => ({
  hz: 99,
  threads: "main",
  ticks_over_ms: null,
  wall: false,
  timeline,
  gpu: "off",
  samples: true,
  window_s: null,
});

const kinds = (...sides: Parameters<typeof exportsFor>[0]) => exportsFor(sides).map((item) => item.kind);

test("a sampled capture without a timeline offers the sample formats only", () => {
  assert.deepEqual(kinds({ sections: { samples: 3 }, options: options(false) }), ["pprof", "folded"]);
});

test("a timeline adds Perfetto, and the other side may be the one that has it", () => {
  assert.deepEqual(kinds({ sections: { samples: 1 }, options: options(false) }, { sections: { samples: 0 }, options: options(true) }), [
    "pprof",
    "folded",
    "perfetto",
  ]);
});

test("a capture with neither has nothing to export", () => {
  assert.deepEqual(kinds({ sections: { samples: 0 }, options: null }, { sections: {}, options: undefined }), []);
});
