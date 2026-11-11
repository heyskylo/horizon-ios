#!/usr/bin/env python3
"""A painting's saved pitch (Rotation[1]) survives save and load.

`Entity.load` sets both angles (`setXRot` ignores a non-finite pitch and otherwise clamps
`xRot % 360` to +-90); `Painting.readAdditionalSaveData`'s `setDirection` only writes the yaw, so
`Entity.saveWithoutId` writes the loaded pitch back. Paintings are summoned with several pitches
against a wall, the world is saved and reloaded (tools/entity_save_reload.py), and each one's
`Rotation`, `facing` and `variant` are read with `/data get` on vanilla's server and Horizon's.
"""
try:
    from . import entity_save_reload as harness
except ImportError:
    import entity_save_reload as harness

# (tag, saved Rotation or None for the field absent)
CASES = [('absent', None), ('zero', '[0.0f,0.0f]'), ('pitched', '[45.0f,10.0f]'), ('fraction', '[0.0f,-33.5f]'),
         ('wrapped', '[0.0f,-400.0f]'), ('over', '[0.0f,135.0f]'), ('under', '[90.0f,-90.0f]')]

# Each painting faces east (facing 3) and hangs on a stone block to its west.
SETUP = []
for i, (tag, rotation) in enumerate(CASES):
    x = 3 + 3 * i
    SETUP += [f'setblock {x - 1} -60 2 minecraft:stone',
              f'summon minecraft:painting {x}.5 -60 2.5 {{Tags:["{tag}"],facing:3b,variant:"minecraft:kebab",'
              f'block_pos:[I;{x},-60,2],Pos:[{x}.5d,-60.0d,2.5d],Invulnerable:1b' + (f',Rotation:{rotation}' if rotation else '') + '}']
PROBES = [(f'{tag} {field}', f'data get entity @e[tag={tag},limit=1] {field}')
          for tag, _ in CASES for field in ('Rotation', 'facing', 'variant')]


def main():
    return harness.main('painting', SETUP, PROBES, __doc__)


if __name__ == '__main__':
    raise SystemExit(main())
