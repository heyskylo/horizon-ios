# Decompilation and Rust research lab

Horizon started as a decompilation and optimization lab. This page describes the local game inspection workspace and the first Rust research crates. For the playable client, see [the native client](native-client.md); for the rewrite as a whole, see [Rust rewrite status](rust-parity.md).

## Local game inspection

Game JARs, decompiled Java, libraries, inventories, worlds and logs live **inside this checkout** under gitignored **`.local/`**. No game code is in Git history or release downloads. Fresh clones rebuild the inspection workspace from your own copy of the game (see [Build from source](../README.md#build-from-source)); nothing of the game is downloaded.

```sh
git clone https://github.com/horizon-rust/horizon.git
cd horizon
git config core.hooksPath .githooks
python3 tools/lab.py prepare --server-jar ~/Downloads/server.jar
python3 tools/lab.py decompile
python3 tools/lab.py status
```

`research-lock.json` pins the official manifest, the client and server jar checksums, and Vineflower. The inner server JAR is extracted and checked; client/server libraries provide decompiler context. Output: `.local/minecraft/26.3/sources/{client,server}/` and coverage/inventory reports. Current inventories cover 11,383 client classes and 7,762 server classes, producing 7,301/5,037 Java files. See [decompilation status](decompilation-status.md).

Class-file SourceFile metadata covers inner classes, which often share a source file. Coverage does not prove semantic equivalence or recompilability; original bytecode remains the reference. Completed source directories are not overwritten automatically. Older obfuscated releases require a separate remapping stage.

## Original Rust research

The dependency-free `horizon-core` crate starts the Rust simulation foundation:
integer block/chunk/section positions, legacy and xoroshiro seeded random streams and positional factories,
padded palette-index storage, scheduled block/fluid tick queues, and double-precision
bounding-box and voxel-grid collision geometry. Noise samplers, octave stacks and
bulk density kernels provide the next world-generation foundation. Compiled
density samplers add gradients, shifted and blended terrain noise, distance
metrics and arithmetic evaluation.
Differential runners compare selected
operations with the checksum-pinned vanilla client and server bytecode.
[Implemented scope, validation, and remaining rewrite work](rust-parity.md).

```sh
python3 tools/test_rust_parity.py --prepare
python3 tools/test_world_parity.py
python3 tools/test_tick_parity.py
python3 tools/test_geometry_parity.py
python3 tools/test_collision_parity.py
python3 tools/test_noise_parity.py
python3 tools/test_density_parity.py
```

The core corpus checks 1,634,520 output lines per reference JAR, including
signed boundaries, RNG rejection/state consumption and raw storage words.
Passing it does not establish full gameplay parity or an FPS improvement.
The separate [noise suite](noise-generation.md) checks 26.3 Perlin/simplex,
derivatives, density-volume accumulation, stack composition, normal-noise
normalization and modern/legacy initialization against both reference JARs.
The [density suite](density-functions.md) checks compiled scalar and bulk
evaluation, including nested samplers, their different arithmetic paths, and
selected interval operations used for graph range analysis.

`horizon-world` adds NBT, four region compression codecs, `.mca`/`.mcc`
reading and export, saved palettes, a version-checked block-state registry,
and packed resident block sections. The world-data suite checks Rust output
with the actual vanilla NBT, region and block-state decoders on both JARs.
It covers all 35,723 exported state IDs and a 100,000-edit section trace.
[World-data commands and validation limits](world-data.md).

Scheduled ticks now support vanilla ordering, duplicate suppression, save/reload,
chunk eligibility and callback mutations. The tick suite compares 97,776 output
lines per reference JAR and checks saved block/fluid lists through vanilla codecs
and region files. [Scheduler behavior and remaining integration](scheduled-ticks.md).

Voxel shapes preserve vanilla collision tolerances, greedy box ordering, ray faces,
inside hits, and cube-versus-array cell lookup. A cached collision registry connects
imported shapes to resident block sections while checking exact state identities.
Movement helpers resolve supplied colliders and step-up candidates, including
landing adjustment and the second collider query. Movement-path block visits
preserve vanilla ordering, step indices and packed-position deduplication.
Uncached states require block behavior; the entity-context overload also needs
separate implementation. [Geometry and collision scope](collision-geometry.md).

```sh
cargo test --workspace
```

Public branding and new mod downloads use **Horizon**. The existing Fabric mod
ID and `dev.horizonlab` namespace remain stable for installed instances and
measurement compatibility. The installer backs up both old `horizon-lab-*.jar`
and new `horizon-*.jar` versions before installing an update.

