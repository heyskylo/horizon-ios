#!/usr/bin/env python3
"""Saved Interaction and Display compounds through the real /data and selector paths."""
import json
import sys

from command_exec_corpus import cmd
import harness_args
from parity_fuzz import read_data
import test_command_exec_parity as driver


CHUNK = 256
EDGE = 16 * CHUNK
IDENTITY = ('{translation:[0f,0f,0f],left_rotation:[0f,0f,0f,1f],'
            'scale:[1f,1f,1f],right_rotation:[0f,0f,0f,1f]}')
TRANSFORM = ('{translation:[1,2,3],left_rotation:[0,2,0,0],'
             'scale:[2,3,4],right_rotation:[0,0,0,1]}')
DISPLAY_FIELDS = (
    f'transformation:{TRANSFORM},billboard:"center",interpolation_duration:7s,'
    'start_interpolation:-3,teleport_duration:100,view_range:2,shadow_radius:0.25d,'
    'shadow_strength:0.5f,width:4,height:3s,glow_color_override:42s,brightness:{block:7b,sky:12}'
)
COMMON_FIELDS = ('Motion:[0.25d,11d,-0.5d],fall_distance:2,Fire:-1,Air:270,'
                 'OnGround:1,Invulnerable:257,PortalCooldown:17s')


def program():
    ops = [
        cmd('/tick freeze'),
        cmd('/gamerule spawn_mobs false'),
        cmd(f'/forceload add {EDGE} {EDGE}', expect_result=1),
        {'op': 'settle', 'required_chunks': [[CHUNK, CHUNK]]},
        cmd(f'/execute if loaded {EDGE} 200 {EDGE}', expect_result=1),
    ]

    def summon(kind, fields=''):
        index = len(actors) + 1
        uid = f'0000310c-0000-0000-0000-{index:012x}'
        suffix = f',{fields}' if fields else ''
        ops.append(cmd(f'/summon {kind} {EDGE + 2.5} 200 {EDGE + 2.5} {{UUID:[I;12556,0,0,{index}]{suffix}}}',
                       expect_result=1))
        actors.append((uid, kind))
        return uid

    def body(uid, kind):
        ops.append(cmd(f'/execute as {uid} if entity @s[type={kind}]', expect_result=1))
        ops.append(cmd(f'/data get entity {uid}', expect_result=1))

    actors = []
    for kind in ('interaction', 'text_display', 'block_display', 'item_display'):
        summon(kind)
    interaction = summon('interaction', COMMON_FIELDS + ',width:3d,height:2,response:257,'
                         'attack:{player:[I;1,2,3,4],timestamp:5}')
    text = summon('text_display', DISPLAY_FIELDS + ',text:{text:"Hello"},'
                  'line_width:-2.5f,text_opacity:130s,background:17s,'
                  'alignment:"right",shadow:257,see_through:256,default_background:1b')
    summon('block_display', 'block_state:{id:"minecraft:oak_log",properties:{axis:"x"}},'
           'transformation:[1f,0f,0f,2f,0f,1f,0f,3f,0f,0f,1f,4f,0f,0f,0f,1f]')
    summon('item_display', 'Rotation:[12.5f,-20f],item:{id:"diamond"},item_display:"on_shelf"')
    summon('text_display', 'text:123,alignment:"top",line_width:"wide",billboard:"top",'
           'transformation:{},teleport_duration:-7,brightness:{block:16,sky:0}')
    summon('block_display', 'block_state:"minecraft:not_a_block"')
    summon('item_display', 'Rotation:[12.5f,-20f],item:{id:"minecraft:air"},item_display:7')
    summon('interaction', 'Motion:[1d,2d],fall_distance:"far",Air:"air",Fire:"fire",'
           'Invulnerable:"yes",OnGround:"yes",PortalCooldown:"soon",'
           'width:"wide",height:"tall",response:"yes",interaction:{player:[I;1,2,3,4]}')

    for uid, kind in actors:
        body(uid, kind)
    ops += [
        cmd('/execute if entity @e[type=interaction,nbt={width:1f,height:1f,response:0b,Air:300s}]',
            expect_result=2),
        cmd(f'/execute if entity @e[type=text_display,nbt={{transformation:{IDENTITY},'
            'billboard:"fixed",line_width:200,text:""}]', expect_result=2),
        cmd(f'/execute unless data entity {text} start_interpolation', expect_result=1),
        cmd(f'/data remove entity {interaction} width', expect_result=1),
        cmd(f'/data merge entity {interaction} {{height:3s,response:256}}', expect_result=1),
        cmd(f'/data get entity {interaction}', expect_result=1),
        cmd(f'/forceload remove {EDGE} {EDGE}', expect_result=1),
        # Expire the temporary chunk tickets as well as draining the unload queue.
        cmd('/tick unfreeze'),
        {'op': 'tick', 'n': 120, 'pace_ms': 50, 'allow_chunk_io': True},
        cmd('/tick freeze'),
        cmd(f'/execute unless loaded {EDGE} 200 {EDGE}', expect_result=1),
        cmd(f'/execute unless entity @e[x={EDGE},y=200,z={EDGE},distance=..10,type=!player]',
            expect_result=1),
        cmd(f'/forceload add {EDGE} {EDGE}', expect_result=1),
        {'op': 'settle', 'required_chunks': [[CHUNK, CHUNK]]},
        cmd(f'/execute if loaded {EDGE} 200 {EDGE}', expect_result=1),
    ]
    for uid, kind in actors:
        body(uid, kind)
    return ops


def main():
    build = harness_args.ensure_work_dir(driver.lab.LOCAL / 'display-save-parity')
    ops = program()
    driver.corpus_module.groups = lambda: [('display and interaction saves', ops)]
    sys.argv += ['--allow-gaps', '--limit', '0']
    driver.main()
    if '--reference-only' in sys.argv:
        return

    vanilla = [json.loads(line) for line in (build / 'vanilla.jsonl').read_text(encoding='utf-8').splitlines()]
    native = [json.loads(line) for line in (build / 'rust.jsonl').read_text(encoding='utf-8').splitlines()]
    differences = []
    for index, (want, got) in enumerate(zip(vanilla, native, strict=True), 1):
        if want['cmd'].startswith('/data get entity '):
            # Compound ordering in the pretty printer is separate from saved NBT.
            # Parse the complete compounds, keeping numeric tag types and list order.
            expected = read_data(want)
            actual = read_data(got)
            matches = (expected is not None and isinstance(expected[1], dict)
                       and expected == actual and want['results'] == got['results'])
        else:
            matches = want == got
        if not matches:
            differences.append({'row': index, 'vanilla': want, 'horizon': got})
    report = {'records': len(vanilla), 'passed': len(vanilla) - len(differences),
              'comparison': 'typed SNBT compounds and exact command records',
              'differences': differences}
    (build / 'save-report.json').write_text(json.dumps(report, indent=2) + '\n', encoding='utf-8')
    print(f'Saved NBT and commands: {len(vanilla) - len(differences)}/{len(vanilla)} match')
    if differences:
        sys.exit(f'{len(differences)} saved NBT or command mismatches; see {build / "save-report.json"}')


if __name__ == '__main__':
    main()
