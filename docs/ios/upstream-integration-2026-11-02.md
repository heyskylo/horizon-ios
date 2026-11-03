# Mobile upstream integration: 2 November 2026

This update merges `horizon-rust/horizon` through `3a264943c`, following the
previous `dea6dea98` snapshot. It preserves the mobile fork's launcher and
platform behavior while adopting upstream's gameplay-library split, custom
entity rendering, connected textures and renderer allocation/pipeline reuse.

## Conflict and compatibility decisions

- Move reusable mobile modules into the new `horizon-client-lib` exports and
  keep mobile initialization in shared `entry::main`. Android's NativeActivity
  entry and allocator now live with its gameplay library. Preserve Apple-only
  backend selection and the local winit, Naga and Java-regex patches.
- Preserve native launcher startup, account/import handling, iOS orientation,
  AirPlay, true engine shutdown, drained workers and suspended memory monitoring.
- Keep shader state IDs and interpolated lighting attached to subdivided CTM
  geometry, overlays, sorting and arena moves. Keep deep-stack shader compilation
  on pipeline-cache misses and retain camera-origin rebasing.
- Combine emissive armor layers and GPU/custom entity rendering with the mobile
  item/trim metadata and NPC skin fixes. Retain Java-correct TNT float drag.
- Bound decoded texture prefetch and PNG allocations. Inflate pack entries
  serially on iOS/Android to avoid multiplying per-entry memory use.
- Notify asynchronous frame completion for center-depth readbacks, as already
  done on the synchronous path; otherwise VSync-off shader depth could stall.
- Keep manual workflow inputs in quoted environment variables. Correct the
  readiness tool's GPUI runtime-shader feature to the new platform dependency.

## Validation interpretation

The focused renderer tests exercise actual Metal readback, including metadata
movement and asynchronous center depth. GPU skinning positions allow a tight
four-ULP FP32 rounding bound while other vertex data remains byte-exact; the
observed maximum was two ULP. Camera-history tests retain exact prior matrices
and verify displacement in the rebased coordinate system.

Private Complementary Reimagined 5.9.3 and Solas 3.8 inputs parse and build complete
Metal pipelines in all three dimensions. These are pipeline and focused GPU
checks, not a full-world visual comparison or physical older-iPad validation.
No private game files, shader archives, personal configuration or diagnostic
reports belong in this update. The startup report in mobile issue #14 still
requires the reporter's diagnostics and is not claimed fixed here.
