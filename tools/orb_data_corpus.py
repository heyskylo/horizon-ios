"""`/data` on a live experience orb's Age (H009, the orb half).

`EntityDataAccessor.setData` keeps the entity's UUID around `Entity.load`
of the edited compound, and `ExperienceOrb.readAdditionalSaveData` reads
`Age` (a short). So `data merge entity <orb> {Age:42s}` changes the live
orb, which saves `Age:42s` again, with the same UUID. A falling block's
`Time` is the other half of H009 and is not measured here.

The world is frozen and no tick runs. A NoGravity orb with a fixed UUID is
edited; a second orb far away and a NoAI pig (`Age`, an int) are controls.

Label classes, declared before any native run: `target-` rows read the orb
after an Age write; `diag-` rows are recorded diagnostics; every other row
is a setup admission or a control that must hold.
"""

ORB = '00000009-0000-0000-0000-000000000001'
WITNESS = '00000009-0000-0000-0000-000000000002'
PIG = '00000009-0000-0000-0000-000000000003'
Y = 100


def orb_data(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/fill 0 {Y} 0 15 {Y + 3} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    still = 'NoGravity:1b,Motion:[0d,0d,0d]'
    cmd(f'/summon experience_orb 4.5 {Y} 4.5 {{{still},Value:3s,UUID:[I;9,0,0,1]}}', 'orb', admission=False)
    cmd(f'/summon experience_orb 12.5 {Y} 12.5 {{{still},Value:5s,UUID:[I;9,0,0,2]}}', 'witness',
        admission=False)
    cmd(f'/summon pig 8.5 {Y} 2.5 {{NoAI:1b,{still},UUID:[I;9,0,0,3]}}', 'pig', admission=False)
    cmd('/scoreboard objectives add h009 dummy', 'objective')
    cmd('/execute store result score start h009 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    for name, uuid, kind, x, z in (('orb', ORB, 'experience_orb', 4.5, 4.5),
                                   ('witness', WITNESS, 'experience_orb', 12.5, 12.5),
                                   ('pig', PIG, 'pig', 8.5, 2.5)):
        cmd(f'/execute as {uuid} if entity @s[type={kind},x={x},y={Y},z={z},distance=..0.01]', f'{name}-at', 1)
        cmd(f'/data get entity {uuid} Motion', f'{name}-motion', admission=False)
        cmd(f'/data get entity {uuid} Age', f'{name}-age', admission=False)
    for field in ('Value', 'Count', 'Health'):
        cmd(f'/data get entity {ORB} {field}', f'orb-{field.lower()}', admission=False)
    cmd('/execute store result score orbs h009 if entity @e[type=experience_orb]', 'orbs', 2)

    def orb_reads(key):
        cmd(f'/data get entity {ORB} Age', f'{key}-age', admission=False)
        cmd(f'/execute if data entity {ORB} {{Age:42s}}', f'{key}-age-42s', admission=False)
        cmd(f'/data get entity {ORB} UUID', f'{key}-uuid', admission=False)
        cmd(f'/data get entity {ORB} Value', f'{key}-value', admission=False)
        cmd(f'/data get entity {ORB} Pos', f'{key}-pos', admission=False)
        cmd('/execute if entity @e[type=experience_orb]', f'{key}-orb-count', admission=False)
        cmd(f'/data get entity {WITNESS} Age', f'{key}-witness-age', admission=False)

    # The target: the merge, then the same again (nothing to change).
    cmd(f'/data merge entity {ORB} {{Age:42s}}', 'target-merge', admission=False)
    orb_reads('target-merged')
    cmd(f'/data merge entity {ORB} {{Age:42s}}', 'target-merge-again', admission=False)
    orb_reads('target-again')
    # Fresh result and success stores.
    cmd('/scoreboard players set r h009 -7', 'r-reset', admission=False)
    cmd('/scoreboard players set s h009 -7', 's-reset', admission=False)
    cmd(f'/execute store result score r h009 store success score s h009 run data merge entity {ORB} '
        '{Age:43s}', 'target-stored-merge', admission=False)
    cmd('/scoreboard players get r h009', 'target-stored-result', admission=False)
    cmd('/scoreboard players get s h009', 'target-stored-success', admission=False)
    cmd(f'/data get entity {ORB} Age', 'target-stored-age', admission=False)
    cmd(f'/data modify entity {ORB} Age set value 50s', 'target-modify', admission=False)
    cmd(f'/data get entity {ORB} Age', 'target-modified-age', admission=False)
    # The UUID stays the entity's own (EntityDataAccessor.setData).
    cmd(f'/data merge entity {ORB} {{UUID:[I;1,2,3,4]}}', 'diag-uuid-merge', admission=False)
    cmd(f'/data get entity {ORB} UUID', 'diag-uuid-after', admission=False)
    cmd('/execute if entity 00000001-0000-0002-0000-000300000004', 'diag-uuid-other', admission=False)
    # The pig, an int Age, already adopted.
    cmd(f'/data merge entity {PIG} {{Age:42}}', 'pig-merge', admission=False)
    cmd(f'/data get entity {PIG} Age', 'pig-merged-age', admission=False)
    cmd('/execute store result score now h009 run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h009 -= start h009', 'clock', 0)
    return ops
