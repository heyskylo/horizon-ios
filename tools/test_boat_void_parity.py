#!/usr/bin/env python3
"""Real server Entity.load and boat void removal in the Overworld and End.

Compare complete command records with pinned vanilla; no masks or tolerances.
The data-merge route invokes Entity.load, as saved entity loading does. Scratch
save replays separately exercise the supplied region file through EntityStorage.
"""
import json
from pathlib import Path
import sys
import tempfile
import uuid
import test_command_exec_parity as driver


def program():
    ops = []
    def cmd(text):
        op = dict(op='cmd', cmd=text)
        if text.startswith(('/summon ', '/execute if entity ', '/execute unless entity ', '/data merge ')):
            op['expect_result'] = 1
        if text.startswith('/data get '):
            op['expect_result'] = 3
        ops.append(op)
    def tick(n):
        cmd('/tick unfreeze')
        ops.append(dict(op='tick', n=n))
        cmd('/tick freeze')
    cmd('/tick freeze')
    cmd('/gamerule spawn_mobs false')
    cmd('/gamerule show_advancement_messages false')
    for dimension, line in [('overworld', -128), ('the_end', -64)]:
        cmd(f'/execute in minecraft:{dimension} run tp @s 8.5 200 8.5')
        cmd('/forceload add 0 0')
        cmd('/forceload add -78 -56')
        ops.append(dict(op='settle', required_chunks=[[0, 0], [-5, -4]]))
        boat = str(uuid.UUID(int=(801 << 96) | 1))
        cmd(f'/summon oak_boat 0.5 {line} 0.5 {{UUID:[I;801,0,0,1],Motion:[0d,-0.5d,0d]}}')
        cmd(f'/execute if entity {boat}')
        tick(1)
        cmd(f'/execute if entity {boat}')
        tick(1)
        cmd(f'/execute unless entity {boat}')
        cmd('/execute unless entity @e[type=item]')
        # Load the incident's exact position and motion while frozen. Clamping
        # must happen at load, and removal on the very next server tick.
        cmd(f'/summon oak_boat 0.5 200 0.5 {{UUID:[I;801,0,0,1]}}')
        cmd(f'/data merge entity {boat} {{Pos:[0.5d,30000000d,0.5d],Motion:[10d,-10d,10.00001d]}}')
        cmd(f'/data get entity {boat} Pos')
        cmd(f'/data get entity {boat} Motion')
        cmd(f'/data merge entity {boat} {{Pos:[-77.75688596047263d,-2146769274.8915758d,-55.6734678353711d],Motion:[-2e-323d,-13104.999899947774d,-2e-323d]}}')
        cmd(f'/data get entity {boat} Pos')
        cmd(f'/data get entity {boat} Motion')
        tick(1)
        cmd(f'/execute unless entity {boat}')
        # Boundary equality survives indefinitely without gravity.
        cmd(f'/summon oak_boat 0.5 {line} 0.5 {{UUID:[I;801,0,0,1],NoGravity:1b}}')
        tick(3)
        cmd(f'/execute if entity {boat}')
        cmd(f'/kill {boat}')
        cmd('/kill @e[type=item]')
    return ops


def main():
    if '--work-dir' not in sys.argv:
        parent = Path(tempfile.mkdtemp(prefix='boat-void-', dir=driver.lab.LOCAL))
        sys.argv += ['--work-dir', str(parent / 'run')]
    work = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
    driver.corpus_module.groups = lambda: [('Boat load and void removal', program())]
    sys.argv.append('--allow-gaps')
    driver.main()
    if '--reference-only' in sys.argv:
        return
    vanilla = [json.loads(s) for s in (work / 'vanilla.jsonl').read_text().splitlines()]
    rust = [json.loads(s) for s in (work / 'rust.jsonl').read_text().splitlines()]
    assert len(vanilla) == len(rust)
    errors = [(a, b) for a, b in zip(vanilla, rust) if a != b]
    assert not errors, errors[:3]
    print(f'Boat load and void removal: {len(vanilla)} complete records match vanilla')


if __name__ == '__main__':
    main()
