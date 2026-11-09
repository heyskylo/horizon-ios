"""Two actual modern falling states admitted before exactly one world tick."""
import json
from pathlib import Path
import sys

import test_command_exec_parity as runner
from command_exec_corpus import cmd, setup


def cases():
    ops = setup('/forceload add 0 0 15 15',
                '/fill 0 199 0 15 199 15 stone',
                '/fill 0 200 0 15 210 15 air')
    ops += [{'op': 'settle'}]
    ops += [cmd('/scoreboard objectives add h097 dummy', expect_result=1),
            cmd('/execute if loaded 4 204 4', expect_result=1),
            cmd('/execute if loaded 8 204 4', expect_result=1),
            cmd('/execute if block 4 199 4 stone', expect_result=1),
            cmd('/execute if block 8 199 4 stone', expect_result=1)]
    ops += setup('/execute store result score start h097 run time query gametime')
    for i, (block, x) in enumerate((('air', 4), ('sand', 8)), 1):
        uuid = f'00000061-0000-0000-0000-{i:012x}'
        fields = (f'UUID:[I;97,0,0,{i}],Tags:["h097_{block}"],'
                  f'BlockState:"minecraft:{block}",Time:0,NoGravity:1b,Motion:[0d,0d,0d]')
        ops.append(cmd(f'/summon falling_block {x}.5 204 4.5 {{{fields}}}', expect_result=1))
        guard = (f'@s[type=minecraft:falling_block,tag=h097_{block},nbt={{'
                 f'UUID:[I;97,0,0,{i}],Pos:[{x}.5d,204d,4.5d],'
                 'Motion:[0d,0d,0d],NoGravity:1b,Time:0}]')
        ops += [cmd(f'/execute as {uuid} if entity {guard}', expect_result=1),
                cmd(f'/execute as {uuid} if entity @s[nbt={{BlockState:"minecraft:{block}"}}]', expect_result=1)]
        for field in ('UUID', 'Pos', 'Motion', 'BlockState', 'NoGravity', 'Time'):
            ops.append(cmd(f'/data get entity {uuid} {field}'))
        row = cmd(f'/data get entity {uuid}');row['diagnostic'] = 'complete initial saved body';ops.append(row)
    ops += setup('/execute store result score elapsed h097 run time query gametime')
    ops += [cmd('/scoreboard players operation elapsed h097 -= start h097', expect_result=0),
            {'op': 'tick', 'n': 1}]
    ops += setup('/execute store result score elapsed h097 run time query gametime')
    ops += [cmd('/scoreboard players operation elapsed h097 -= start h097', expect_result=1)]
    for i, (block, expected) in enumerate((('air', 0), ('sand', 1)), 1):
        uuid = f'00000061-0000-0000-0000-{i:012x}'
        probe = f'/execute if entity {uuid}'
        op = cmd(probe);op['expect_callback'] = [bool(expected), expected];ops.append(op)
        ops += setup('/scoreboard players set result h097 31', '/scoreboard players set success h097 17')
        op = cmd(f'/execute store result score result h097 store success score success h097 run execute if entity {uuid}')
        op['expect_callback'] = [bool(expected), expected];ops.append(op)
        ops += [cmd('/scoreboard players get result h097', expect_result=expected),
                cmd('/scoreboard players get success h097', expect_result=expected)]
        if expected:
            ops += [cmd(f'/execute as {uuid} if entity @s[type=falling_block,nbt={{UUID:[I;97,0,0,{i}],BlockState:"minecraft:sand",Pos:[8.5d,204d,4.5d],Motion:[0d,0d,0d],NoGravity:1b,Time:1}}]', expect_result=1)]
            for field in ('BlockState', 'Time', 'Pos', 'Motion'):
                ops.append(cmd(f'/data get entity {uuid} {field}'))
        else:
            ops.append(cmd(f'/execute unless entity {uuid}', expect_result=1))
        row = cmd(f'/data get entity {uuid}');row['diagnostic'] = 'complete post-tick body or absence';ops.append(row)
    ops += setup('/kill @e[tag=h097_air]', '/kill @e[tag=h097_sand]', '/scoreboard objectives remove h097')
    return ops


def main():
    runner.corpus_module.groups = lambda: [
        ('setup', runner.corpus_module.group_setup()), ('falling air first tick', cases())]
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir', str(runner.lab.LOCAL / 'falling-air-tick-parity')]
    if '--allow-gaps' not in sys.argv:
        sys.argv.append('--allow-gaps')
    runner.main()
    if '--reference-only' in sys.argv:
        return 0
    folder = Path(sys.argv[sys.argv.index('--work-dir') + 1])
    script = [json.loads(line) for line in (folder / 'script.jsonl').read_text().splitlines()]
    ops = [op for op in script if op['op'] == 'cmd' and op.get('record', True)]
    java = [json.loads(line) for line in (folder / 'vanilla.jsonl').read_text().splitlines()]
    native = [json.loads(line) for line in (folder / 'rust.jsonl').read_text().splitlines()]
    assert len(ops) == len(java) == len(native)
    report = {'strict': 0, 'strict_failures': [], 'diagnostics': 0, 'diagnostic_failures': []}
    for i, (op, left, right) in enumerate(zip(ops, java, native)):
        diagnostic = bool(op.get('diagnostic'))
        report['diagnostics' if diagnostic else 'strict'] += 1
        if left != right:
            report['diagnostic_failures' if diagnostic else 'strict_failures'].append(i)
    (folder / 'air-report.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])


if __name__ == '__main__':
    sys.exit(main())
