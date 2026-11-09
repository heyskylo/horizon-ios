#!/usr/bin/env python3
"""Compare where a shulker's bullet starts and what turns it in flight against vanilla."""
import json
import struct

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def peeks():
    """Shulker.updatePeekAmount opening the lid: 0, then +0.05F a tick up to 1 (float sums)."""
    peek, out = 0.0, [0.0]
    for _ in range(20):
        peek = min(f32(peek + f32(0.05)), 1.0)
        out.append(peek)
    return out


# Full cubes, partial tops, hollow tops, context shapes (scaffolding, powder snow, liquids) and empty shapes.
BLOCKS = ['air', 'stone', 'glass', 'oak_leaves', 'barrier', 'mangrove_roots', 'soul_sand', 'mud',
          'honey_block', 'farmland', 'dirt_path', 'cactus', 'chest', 'end_portal_frame', 'hopper',
          'cauldron', 'composter', 'lectern', 'daylight_detector', 'oak_fence', 'cobblestone_wall',
          'iron_bars', 'bamboo', 'big_dripleaf', 'powder_snow', 'water', 'lava', 'light',
          'oak_slab[type=bottom]', 'oak_slab[type=top]', 'oak_slab[type=double]',
          'oak_stairs[half=bottom]', 'oak_stairs[half=top,facing=north]',
          'oak_trapdoor[half=bottom]', 'oak_trapdoor[half=top]', 'oak_trapdoor[half=top,open=true]',
          'snow[layers=1]', 'snow[layers=8]', 'scaffolding[distance=0,bottom=false]',
          'scaffolding[distance=7,bottom=true]']


def make_input(build):
    rows = []
    for position in ([0.5, 64.0, 0.5], [-7.5, -3.0, 12.5]):
        target = [position[0] + 6, position[1] + 2, position[2] - 4]
        for face in range(6):
            for peek in peeks():
                for scale in (1.0, 0.5, 2.5, 4.0):
                    rows.append(dict(mode='origin', seed=1, cells=[], position=position, target=target, face=face,
                                     peek=struct.unpack('<I', struct.pack('<f', peek))[0], scale=scale))
    for pos in ([0, 64, 0], [-17, -20, 31]):
        for block in BLOCKS:
            for above in (-0.5, 0.5, 1.0, 1.5):
                rows.append(dict(mode='stand', seed=1, cells=[], pos=pos, block='minecraft:' + block,
                                 y=pos[1] + above))
    path = build / 'inputs.jsonl'
    path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
    return path, len(rows)


def main():
    harness.differential(
        'rust-shulker-bullet-parity', 'VanillaShulkerBulletOracle', 'shulker_bullet_probe', make_input,
        'The real ShulkerBullet(level, shulker, target, axis) constructor: the bullet starts at the centre of '
        'Shulker.makeBoundingBox for all six attach faces, every peek of an opening lid and four scales (one over '
        'the cap of 3). Level.loadedAndEntityCanStandOn(pos, bullet), the test that turns a bullet in flight, over '
        'forty block states with full, partial, hollow, empty and context-dependent collision shapes, with the '
        'bullet below, level with, on and above the block. The bullet\'s own random flight path is not compared.',
        sources=[lab.ROOT / 'crates/horizon-client/src/mobs/shulker.rs',
                 lab.ROOT / 'crates/horizon-client/src/projectiles.rs',
                 lab.ROOT / 'crates/horizon-client-render/src/collisions.rs',
                 lab.ROOT / 'crates/horizon-world/src/block_collision.rs',
                 lab.ROOT / 'crates/horizon-world/src/collision_context.rs',
                 lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaBlockUpdateOracle.java',
                 lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java'])


if __name__ == '__main__':
    main()
