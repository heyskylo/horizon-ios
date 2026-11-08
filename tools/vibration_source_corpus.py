"""A vibration's saved source and projectile owner UUIDs (H064).

`VibrationInfo(gameEvent, distance, pos, entity)` keeps the source entity's
UUID, and, for a projectile with an owner, the owner's UUID;
`VibrationInfo.CODEC` saves them as `source` and `projectile_owner` beside
`game_event`, `distance` and `pos`. A sculk sensor's listener (`listener`,
`VibrationSystem.Data`) holds the selected candidate, then the travelling
vibration, and saves both.

The world is frozen; ticks are stepped. Each sensor stands in its own chunk.
A NoAI pig is damaged next to sensor A (an entity source). A snowball owned
by a second pig lands next to sensor B (a projectile source with its owner),
and one with no owner next to sensor C. A note block powered next to sensor
D has no source. A listener holding the UUIDs of entities that do not exist
is merged into sensor E and read back (the codec).

Label classes, declared before any native run: `target-` rows read a saved
`source` or `projectile_owner`; `diag-` rows are recorded diagnostics (full
listeners, absolute clocks); every other row is a setup admission or a
control.
"""

Y = 100
SENSORS = {'a': 8, 'b': 24, 'c': 40, 'd': 56, 'e': 72}
PIG = '00000064-0000-0000-0000-000000000001'
OWNER = '00000064-0000-0000-0000-000000000002'
OWNED = '00000064-0000-0000-0000-000000000003'
ORPHAN = '00000064-0000-0000-0000-000000000004'


def vibration_source(full=False):
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
    cmd('/forceload add 0 0 79 15', 'force', 5)
    ops.append({'op': 'settle', 'required_chunks': [[c, 0] for c in range(5)]})
    for c in range(5):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} 8', f'loaded-{c}', 1)
    cmd(f'/fill 0 {Y - 1} 0 79 {Y - 1} 15 stone', 'floor', 1280)
    cmd(f'/fill 0 {Y} 0 79 {Y + 4} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    for name, x in SENSORS.items():
        cmd(f'/setblock {x} {Y} 8 sculk_sensor', f'sensor-{name}', 1)
    # A new sensor joins the native listeners within ten ticks (a separate
    # difference): every sensor is in place before anything moves.
    cmd('/tick step 12', 'settle-sensors')
    ops.append({'op': 'tick', 'n': 12})
    still = 'NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Silent:1b'
    cmd(f'/summon pig 5.5 {Y} 8.5 {{{still},Health:10f,UUID:[I;100,0,0,1]}}', 'pig', admission=False)
    cmd(f'/summon pig 20.5 {Y} 2.5 {{{still},UUID:[I;100,0,0,2]}}', 'owner', admission=False)
    cmd(f'/execute as {PIG} if entity @s[type=pig,x=5.5,y={Y},z=8.5,distance=..0.01]', 'pig-at', 1)
    cmd(f'/execute as {OWNER} if entity @s[type=pig,x=20.5,y={Y},z=2.5,distance=..0.01]', 'owner-at', 1)
    cmd(f'/setblock 53 {Y} 8 note_block', 'note-block', 1)
    for name, x in SENSORS.items():
        cmd(f'/execute if block {x} {Y} 8 sculk_sensor[sculk_sensor_phase=inactive]', f'sensor-{name}-inactive', 1)
    cmd('/scoreboard objectives add h064 dummy', 'objective')
    cmd('/execute store result score start h064 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    def reads(key, name):
        x = SENSORS[name]
        prefix = 'target-' if name != 'd' else ''
        for part, path in (('selected', 'listener.selector.event'), ('travelling', 'listener.event')):
            for field in ('game_event', 'pos', 'distance'):
                cmd(f'/data get block {x} {Y} 8 {path}.{field}', f'{key}-{name}-{part}-{field}', admission=False)
            for field in ('source', 'projectile_owner'):
                cmd(f'/data get block {x} {Y} 8 {path}.{field}', f'{prefix}{key}-{name}-{part}-{field}',
                    admission=False)
        cmd(f'/data get block {x} {Y} 8 listener', f'diag-{key}-{name}-listener', admission=False,
            diagnostic=True)

    # The events, before any tick.
    cmd(f'/damage {PIG} 1', 'pig-damage', 1)
    snowball = '{Motion:[0d,-0.5d,0d],UUID:[I;100,0,0,%d]%s}'
    cmd(f'/summon snowball 21.5 {Y + 1.2} 8.5 ' + snowball % (3, f',Owner:[I;100,0,0,2]'), 'owned-snowball',
        admission=False)
    cmd(f'/summon snowball 37.5 {Y + 1.2} 8.5 ' + snowball % (4, ''), 'orphan-snowball', admission=False)
    cmd(f'/setblock 53 {Y + 1} 8 redstone_block', 'note-power', 1)
    for name in 'ad':
        reads('t0', name)
    # The snowballs land on their first ticks; then every vibration travels.
    for tick in (1, 2, 3):
        cmd('/tick step 1', f'step-{tick}')
        ops.append({'op': 'tick', 'n': 1})
        cmd('/execute store result score now h064 run time query gametime', f'diag-clock-raw-{tick}',
            admission=False, diagnostic=True)
        cmd('/scoreboard players operation now h064 -= start h064', f'clock-{tick}', tick)
        cmd(f'/execute if entity {OWNED}', f'diag-owned-snowball-t{tick}', admission=False)
        cmd(f'/execute if entity {ORPHAN}', f'diag-orphan-snowball-t{tick}', admission=False)
        for name in 'abcd':
            reads(f't{tick}', name)
    # The codec: saved UUIDs of entities that do not exist.
    x = SENSORS['e']
    saved = ('{selector:{tick:-1L},event_delay:5,event:{game_event:"minecraft:step",distance:3.0f,'
             'pos:[70.5d,100d,8.5d],source:[I;9,9,9,9],projectile_owner:[I;8,8,8,8]}}')
    cmd(f'/data merge block {x} {Y} 8 {{listener:{saved}}}', 'codec-merge', admission=False)
    reads('codec', 'e')
    return ops
