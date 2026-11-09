"""A mannequin's saved fields (H141).

`Mannequin` extends `Avatar` extends `LivingEntity`; it is not a `Mob`.
`Mannequin.addAdditionalSaveData` writes, after `LivingEntity`'s fields,
`profile` (the empty static profile by default, `{}`), `hidden_layers`
(the hidden `PlayerModelPart`s, `[]` by default), `main_hand` (`right`),
`pose` (`standing`), `immovable`, and `description` or `hide_description`.
`Mob.addAdditionalSaveData`'s fields (`NoAI`, `PersistenceRequired`,
`CanPickUpLoot`, `LeftHanded`, `drop_chances`, `leash`, home, death loot)
are not a mannequin's.

The world is frozen and no tick runs. Mannequins with fixed UUIDs are
summoned with no fields, with the explicit defaults, with other values, and
with Mob-only fields; a pig with the same Mob-only fields is the ordinary
Mob control, and a marker the non-living one. One mannequin is then edited
with `/data merge`.

Label classes, declared before any native run: `target-` rows read a
mannequin's own or a Mob-only field; `diag-` rows are recorded diagnostics
(full bodies, invalid values, absolute clocks); every other row is a setup
admission or a control.
"""

Y = 100
MANNEQUIN_FIELDS = ('profile', 'hidden_layers', 'main_hand', 'pose', 'immovable', 'hide_description',
                    'description')
MOB_FIELDS = ('NoAI', 'PersistenceRequired', 'CanPickUpLoot', 'LeftHanded', 'drop_chances', 'leash',
              'home_pos', 'home_radius', 'DeathLootTable')
MOB_INPUT = 'NoAI:1b,PersistenceRequired:1b,CanPickUpLoot:1b,LeftHanded:1b'
CASES = [
    ('absent', 1, ''),
    ('defaults', 2, 'profile:{},hidden_layers:[],main_hand:"right",pose:"standing"'),
    ('alternate', 3, 'hidden_layers:["hat","cape"],main_hand:"left",pose:"crouching"'),
    ('hidden', 4, 'immovable:1b,hide_description:1b'),
    ('custom', 5, 'description:{text:"Guard",color:"red"}'),
    ('mob-input', 6, MOB_INPUT),
]


def uuid(n):
    return f'00000141-0000-0000-0000-00000000000{n}'


def mannequin_save(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    for name, n, fields in CASES:
        extra = ',' + fields if fields else ''
        cmd(f'/summon mannequin {n * 2 + 0.5} {Y} 2.5 {{UUID:[I;321,0,0,{n}],NoGravity:1b{extra}}}',
            f'{name}-summon', admission=False)
    cmd(f'/summon pig 2.5 {Y} 8.5 {{UUID:[I;321,0,0,7],NoGravity:1b,{MOB_INPUT}}}', 'pig-summon',
        admission=False)
    cmd(f'/summon marker 6.5 {Y} 8.5 {{UUID:[I;321,0,0,8]}}', 'marker-summon', admission=False)
    for name, n, _ in CASES + [('pig', 7, ''), ('marker', 8, '')]:
        kind = {'pig': 'pig', 'marker': 'marker'}.get(name, 'mannequin')
        x = {'pig': 2.5, 'marker': 6.5}.get(name, n * 2 + 0.5)
        z = 8.5 if name in ('pig', 'marker') else 2.5
        cmd(f'/execute as {uuid(n)} if entity @s[type={kind},x={x},y={Y},z={z},distance=..0.01]',
            f'{name}-at', 1)
        cmd(f'/data get entity {uuid(n)} UUID', f'{name}-uuid', admission=False)
        cmd(f'/data get entity {uuid(n)} Rotation', f'{name}-rotation', admission=False)
    cmd('/scoreboard objectives add h141 dummy', 'objective')
    cmd('/execute store result score start h141 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    def reads(key, n, fields):
        for field in fields:
            cmd(f'/data get entity {uuid(n)} {field}', f'{key}-{field}', admission=False)
        cmd(f'/data get entity {uuid(n)} Health', f'{key}-health', admission=False)
        cmd(f'/data get entity {uuid(n)}', f'diag-{key}-body', admission=False)

    for name, n, _ in CASES:
        reads(f'target-{name}', n, MANNEQUIN_FIELDS + MOB_FIELDS)
    # The ordinary Mob keeps its Mob fields.
    reads('pig', 7, ('NoAI', 'PersistenceRequired', 'CanPickUpLoot', 'LeftHanded'))
    cmd(f'/data get entity {uuid(8)}', 'diag-marker-body', admission=False)
    # A live edit of a mannequin's own fields, then the same again.
    edit = 'pose:"swimming",main_hand:"left",hidden_layers:["jacket"]'
    cmd(f'/data merge entity {uuid(1)} {{{edit}}}', 'target-edit', admission=False)
    reads('target-edited', 1, MANNEQUIN_FIELDS + MOB_FIELDS)
    cmd(f'/data merge entity {uuid(1)} {{{edit}}}', 'target-edit-again', admission=False)
    cmd('/scoreboard players set r h141 -7', 'r-reset', admission=False)
    cmd('/scoreboard players set s h141 -7', 's-reset', admission=False)
    cmd(f'/execute store result score r h141 store success score s h141 run data merge entity {uuid(1)} '
        '{pose:"standing"}', 'target-stored-edit', admission=False)
    cmd('/scoreboard players get r h141', 'target-stored-result', admission=False)
    cmd('/scoreboard players get s h141', 'target-stored-success', admission=False)
    cmd(f'/data get entity {uuid(1)} pose', 'target-stored-pose', admission=False)
    # Invalid values (diagnostics).
    cmd(f'/data merge entity {uuid(2)} {{pose:"dying"}}', 'diag-bad-pose-edit', admission=False)
    cmd(f'/data get entity {uuid(2)} pose', 'diag-bad-pose', admission=False)
    cmd(f'/data merge entity {uuid(2)} {{main_hand:"middle"}}', 'diag-bad-hand-edit', admission=False)
    cmd(f'/data get entity {uuid(2)} main_hand', 'diag-bad-hand', admission=False)
    cmd('/execute if entity @e[type=mannequin]', 'mannequin-count', admission=False)
    cmd('/execute store result score now h141 run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h141 -= start h141', 'clock', 0)
    return ops
