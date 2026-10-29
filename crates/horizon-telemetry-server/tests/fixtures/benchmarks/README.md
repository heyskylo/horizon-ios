# Benchmark summary fixtures

Used by the telemetry server's `GET /v1/benchmarks/summary` test (`tests/summary.rs`) and, as a copy, by the
website's aggregator test (`scripts/benchmarks.test.mjs` in horizon-rust/horizon-site, which checks its copy against
this one on main): the same `benchmark` items in, the same groups out. Change both together.

- `items.jsonl`: one stored item per line (`install_id`, `day`, `version`, `data`: a benchmark report). Group
  RTX 4080 + i7-13700KF at render distance 12 has three installs (one with two runs); the same install at 32,
  and the RTX 3060 group, have fewer than three and are left out. The other runs are rejected: VSync on,
  offscreen, a frame cap, a missing scene, a one-scene scenario, a report of another version, no install ID.
- Breakdowns: the RTX 4080 group's three installs share a VRAM class (16 GB), the OS and a discrete GPU, so
  those cells are published; a3 has 16 GB of RAM and an SSD where a1 and a2 have 32 GB and NVMe, so neither the
  RAM nor the drive cells reach three installs and those dimensions are left out.
- `expected.json`: `min_installs` and `groups` of the summary (`generated` varies).
