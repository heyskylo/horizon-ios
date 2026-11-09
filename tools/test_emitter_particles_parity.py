#!/usr/bin/env python3
"""Compare particle emitters with the real vanilla ones: ParticleUtils
spawnParticlesOnBlockFaces and spawnParticlesAlongAxis as LevelEventHandler
calls them for level events 3002 (electric sparks), 3003 (wax on), 3004 (wax
off) and 3005 (scrape), LivingEntity.spawnItemParticles (eating, drinking),
and TrackingEmitter (totem and critical hits): particle, count, positions,
motion and every random draw."""
import json
import random

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness


def cases(rng):
    for _ in range(600):
        pos = [rng.randrange(-30000, 30000), rng.randrange(-64, 320), rng.randrange(-30000, 30000)]
        yield dict(kind='block', pos=pos, axis=rng.choice([-1, -1, 0, 1, 2]), seed=rng.randrange(-2**63, 2**63))
    angles = [0.0, 90.0, -90.0, 45.0, -45.0, 180.0, -180.0, 12.5, 359.75, -721.5]
    for _ in range(900):
        # Dyadic values: serde_json's decimal parsing may round others a unit off.
        pos = [rng.randrange(-2**20, 2**20) / 64 for _ in range(3)]
        yield dict(kind='item_use', pos=pos, pitch=rng.choice(angles + [rng.randrange(-5760, 5760) / 64]),
                   yaw=rng.choice(angles + [rng.randrange(-23040, 23040) / 64]),
                   eye=rng.choice([1.62, 1.27, 0.4, 1.74]), count=rng.choice([5, 16, 1, 0]),
                   item=rng.choice(['minecraft:apple', 'minecraft:cooked_beef', 'minecraft:potion', 'minecraft:honey_bottle']),
                   seed=rng.randrange(-2**63, 2**63))
    for particle, lifetime in [('crit', 3), ('enchanted_hit', 3), ('totem_of_undying', 30)]:
        for sample in range(24):
            frames = [
                dict(pos=[rng.randrange(-2**20, 2**20) / 64 for _ in range(3)],
                     width=rng.choice([0.3, 0.6, 1.4]), height=rng.choice([0.6, 1.5, 1.8, 1.95]))
                for _ in range(lifetime + 2)
            ]
            # Losing the target preserves its final body. The next frame
            # can have a replacement at the same id.
            if sample % 2 == 0:
                frames[rng.randrange(1, lifetime)] = None
            yield dict(kind='tracking', particle=particle, frames=frames, seed=rng.randrange(-2**63, 2**63))


def make_cases(build):
    path = build / 'cases.jsonl'
    count = 0
    with path.open('w') as out:
        for case in cases(random.Random(0x454d4954)):
            out.write(json.dumps(case, separators=(',', ':')) + '\n')
            count += 1
    return path, count


def main():
    scope = ('The real ParticleUtils.spawnParticlesOnBlockFaces (UniformInt 3..5 per face) and spawnParticlesAlongAxis '
             '(radius 0.125, UniformInt 10..19) on a stand-in level with a seeded random, as LevelEventHandler runs '
             'them for level events 3002-3005, and the real LivingEntity.spawnItemParticles on an entity given a '
             'position, rotation, eye height and seeded random. TrackingEmitter constructors and ticks use a '
             'seeded particle random and follow movement, dimension changes, removal and lifetime.')
    harness.differential(
        'rust-emitter-particles-parity', 'VanillaEmitterParticlesOracle', 'emitter_particles_probe', make_cases, scope,
        sources=[lab.ROOT / 'crates/horizon-client-render/src/particles.rs'])


if __name__ == '__main__':
    main()
