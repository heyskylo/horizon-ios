# Horizon noise-generation scope

The target is the repository-pinned Minecraft Java Edition **26.3**. The Rust
samplers live in `crates/horizon-core/src/noise/` and use the checked legacy or
xoroshiro streams. They are a world-generation foundation; Horizon does not yet
generate complete vanilla chunks or run a playable Rust game.

## Implemented behavior

- Gradient offsets and all 256 permutation entries, including the three random
  draws consumed when simplex offsets are disabled.
- 26.3 Perlin 2D/3D sampling, single-precision interpolation and accumulation into
  caller-supplied derivative arrays. Smeared Perlin uses its adjusted Y component
  for gradient dots and the original component for interpolation. Its inherited
  derivative method samples the unsmeared field.
- Simplex 2D/3D sampling with both offset modes, double-precision corner arithmetic
  and single-precision return values. The second 2D coordinate uses the Y offset.
- Normal coordinate wrapping with the Far Lands debug flag disabled, including
  the fast-path boundary immediately below 2^24.
- Positive strided density volumes with Z/X/Y flat ordering, Java-wrapping block
  coordinates, bulk Perlin/smeared-Perlin gradient reuse, scalar simplex volumes,
  repeated scaled accumulation and untouched buffer tails.
- Ordered noise stacks, nested sampling, flattened stack composition, layer
  access and range arithmetic. Zero-amplitude layers are retained. Flattening
  rescales each layer before sampling and can differ from a nested stack.
- Normal-noise octave selection, frequencies, amplitude modifiers, compensated
  amplitude sums, enabled/disabled/legacy normalization and the parity factory.
  Modern creation forks two positional factories and derives each octave from
  its name, then interleaves the two sets of Perlin layers.
- Legacy Nether biome initialization, including reverse octave construction,
  exactly 262 integer draws for skipped octaves, rejected positive octaves and
  the RNG state left by initialization failures. Its two stacks are flattened in
  sequence, preserving their distinct accumulation order.

The bulk Perlin kernel adds X/Z gradient contributions before Y. Calling the
scalar sampler repeatedly would rearrange those operations and can change the
result. Rust preserves the bulk path and caches its eight gradients while the
Y cell remains unchanged. These are implementation choices checked for behavior;
no whole-game frame-rate or generation-speed improvement has been measured.

## Validation

After preparing the pinned artifacts with `python3 tools/lab.py prepare`, run:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 tools/test_noise_parity.py
```

The original Java adapter invokes the actual downloaded classes on both client
and server JARs. It exposes offsets, permutations, octave parameters and layer
weights for comparison, and calls the reference scalar and bulk methods.
Artifacts and dependencies are authenticated against the repository lock and
the official bundled checksums. Generated corpus/output and compiled adapters
remain private under `.local/rust-noise-parity/`.

The corpus covers both random streams; offset and lattice boundaries; very large
coordinates; signed zero, subnormals, infinities and NaNs; negative and zero
amplitudes; rejected ranges; nested and flattened stacks; octave gaps; all three
normalization modes; empty definitions; and modern/legacy RNG consumption.
Finite values and signed zeros are compared by bits. NaN payloads/signs are
canonicalized on both sides, matching Java's `floatToIntBits`/`doubleToLongBits`.

The local run matches **1,443,539 output lines per reference JAR**. This is a
count of output records, including repeated checks, not distinct gameplay
behaviors or a coverage percentage. The metadata-only
[scoped report](../benchmarks/rust-noise-2026-10-05/scoped-parity.json) records
source/input/output hashes and reference identities. The runner removes a prior
success report before running and rejects source changes during validation.
CI includes the suite on Linux, Windows and Apple Silicon macOS; the recorded
local result is Linux, not proof that remote jobs or physical gameplay passed.

## API limits and remaining integration

Rust returns typed errors and uses immutable shared samplers. Density dimensions
and strides must be positive, total volume must fit a Java signed array index,
and a short output buffer is rejected before any writes. Vanilla can overflow
its volume-size arithmetic or partially write before an array exception; those
invalid-buffer behaviors are outside this bounded API.

The normal-noise builder limits allocation to 4,096 octaves; the legacy
initializer also limits reverse construction to 4,096 octave steps. Requests
beyond these limits fail before RNG draws. Vanilla's serialized definition
codec limits octave count to 32. Rust's builder owns a snapshot of parameters;
Java object identity, mutable builder-list aliases, equality/hash methods and
the complete Java API surface are not reproduced.

The direct normal-noise JSON codec is now implemented and checked against every
bundled noise definition in the [world-generation codec suite](world-generation.md).
The density registry loader is also compared against all bundled definitions.
Remaining noise initializers, complete terrain-router integration,
aquifers, surface rules, biome/feature/structure placement and complete chunk
generation still require implementation and integration evidence. Gaussian
sampling remains absent from the RNG API pending exact logarithm behavior.
No generated-world or full gameplay parity claim follows from this suite.

## Related docs

- [World generation](world-generation.md)
- [Density functions](density-functions.md)
- [Terrain filling](terrain-filling.md)
