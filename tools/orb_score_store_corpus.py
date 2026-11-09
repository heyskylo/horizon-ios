"""`execute as <experience orb> store … score @s` stores into the orb's score.

`ScoreHolderArgument` resolves `@s` to the source's entity, and any entity is a score holder named by
its UUID (`Entity.getScoreboardName`). An experience orb is one too.

The world is frozen. A fixed-UUID experience orb (`Value:7s`, `Count:2`, no gravity) and a fixed-UUID
armor stand (a supported non-player holder) stand on stone in a loaded chunk. Each subject stores the
result, then the success, of a direct getter into a score set to 31 first, through three holders: the
literal `#lit`, `@s` of the orb and `@s` of the armor stand. Every store is verified by a direct
`scoreboard players get` of the holder by its name (the UUID text for an entity).

Label classes, declared before any native run: `target-` rows are the orb's `@s` stores and their
direct reads; `diag-` rows are recorded diagnostics (clock); every other row is a setup admission or
a control (the literal holder, the armor stand, the direct getters).
"""

Y = 100
ORB = '00000000-0000-0000-0000-0000000000a7'
STAND = '00000000-0000-0000-0000-0000000000a8'
GETTER = {'orb': 'Value', 'stand': 'Air'}


def orb_score_store(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/forceload add 0 0 0 0', 'force', admission=False)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/scoreboard objectives add os dummy', 'objective', 1)
    cmd('/scoreboard objectives list', 'objective-listed', admission=False)
    cmd(f'/summon experience_orb 4.5 {Y} 8.5 {{UUID:[I;0,0,0,167],Value:7s,Count:2,Age:0s,Health:5s,'
        'NoGravity:1b,Motion:[0d,0d,0d]}', 'orb-summon', admission=False)
    cmd(f'/summon armor_stand 10.5 {Y} 8.5 {{UUID:[I;0,0,0,168],NoGravity:1b,Invisible:0b}}',
        'stand-summon', admission=False)
    cmd(f'/execute as {ORB} if entity @s[type=experience_orb]', 'orb-identity', 1)
    cmd(f'/execute as {STAND} if entity @s[type=armor_stand]', 'stand-identity', 1)
    cmd(f'/data get entity {ORB} UUID', 'orb-uuid', admission=False)
    cmd(f'/data get entity {ORB} Value', 'orb-value', 7)
    cmd(f'/data get entity {ORB} Count', 'orb-count', 2)
    cmd(f'/data get entity {STAND} Air', 'stand-air', admission=False)
    # The executor's source binding: `@s` is the subject, and its direct getter succeeds.
    for name, uuid in (('orb', ORB), ('stand', STAND)):
        cmd(f'/execute as {uuid} if entity @s[nbt={{UUID:[I;0,0,0,{167 if name == "orb" else 168}]}}]',
            f'{name}-source-binding', 1)
        cmd(f'/execute as {uuid} run data get entity @s {GETTER[name]}', f'{name}-direct-getter',
            admission=False)
    cmd('/execute store result score #clock os run time query gametime', 'diag-clock', admission=False,
        diagnostic=True)
    # subject, holder text in the store, holder name read back, label prefix
    cases = [
        ('orb', '#lit', '#lit', 'literal'),
        ('orb', '@s', ORB, 'target-orb'),
        ('stand', '@s', STAND, 'stand'),
    ]
    for subject, holder, read, prefix in cases:
        uuid = ORB if subject == 'orb' else STAND
        for kind in ('result', 'success'):
            cmd(f'/scoreboard players set {read} os 31', f'{prefix}-{kind}-fresh', 31)
            cmd(f'/execute as {uuid} store {kind} score {holder} os run data get entity @s {GETTER[subject]}',
                f'{prefix}-{kind}-store', admission=False)
            cmd(f'/scoreboard players get {read} os', f'{prefix}-{kind}-read', admission=False)
    cmd('/scoreboard players list', 'holders-listed', admission=False)
    cmd(f'/execute store result score #now os run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation #now os -= #clock os', 'clock', 0)
    return ops
