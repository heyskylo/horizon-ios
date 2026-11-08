"""The `entity_die` vibration of a mob killed by `/kill` (H251).

`LivingEntity.kill` is `hurtServer(genericKill, Float.MAX_VALUE)`: the hit
runs `LivingEntity.die` before the command returns, and `die` posts
`gameEvent(ENTITY_DIE)` at the mob's position. So a sculk sensor's listener
holds the event before any world tick, and the sensor activates when the
vibration has travelled.

A NoAI, NoGravity pig with a fixed UUID stands three blocks from sensor A
on an open stone floor. Before it is killed, a failed `/kill` (no target)
and a killed armor stand next to sensor B (the independent positive
control of the same event and listener kind) are measured. After the kill
the pig is killed again (no second hit), then the world is stepped one tick
at a time. A pig with one health point suffocates in stone next to sensor C
on the first step: a death in the mob's own tick (the ordinary death
control).

Label classes, declared before any native run: rows whose label starts with
`target-` are the H251 target; `diag-` rows are recorded diagnostics outside
the target (loot, experience, the body's death time, absolute clocks, the
ordinary death); every other row is a setup admission or a control that must
hold.
"""

Y = 100
Z = 8
SENSORS = {'a': 8, 'b': 24, 'c': 44}
# Fixed UUIDs: 00000251-0000-0000-0000-00000000000n.
PIG = '00000251-0000-0000-0000-000000000001'
STAND = '00000251-0000-0000-0000-000000000002'
SUFFOCATING = '00000251-0000-0000-0000-000000000003'
PIG_X, STAND_X, SUFFOCATING_X = 5.5, 21.5, 40.5
STEPS = 8
SETTLE = 12


def uuid_ints(n):
    return f'[I;593,0,0,{n}]'


def kill_die_vibration(full=False):
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
    cmd('/forceload add 0 0 47 15', 'force', 3)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0], [1, 0], [2, 0]]})
    for cx in range(3):
        cmd(f'/execute if loaded {cx * 16 + 8} {Y} {Z}', f'loaded-{cx}', 1)
    cmd(f'/fill 0 {Y - 1} 0 47 {Y - 1} 15 stone', 'floor', 768)
    cmd(f'/fill 0 {Y} 0 47 {Y + 3} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    for name, x in SENSORS.items():
        cmd(f'/setblock {x} {Y} {Z} sculk_sensor', f'sensor-{name}', 1)
        cmd(f'/execute if block {x} {Y} {Z} sculk_sensor[sculk_sensor_phase=inactive,power=0]',
            f'sensor-{name}-inactive', 1)
        cmd(f'/data get block {x} {Y} {Z} last_vibration_frequency', f'sensor-{name}-frequency', 0)
    # World ticks for the sensors before any actor: Horizon finds a new
    # sensor in its periodic chunk scan, within ten ticks (a separate
    # difference; vanilla's block entity registers its listener when it is
    # added).
    cmd(f'/tick step {SETTLE}', 'sensor-settle')
    ops.append({'op': 'tick', 'n': SETTLE})
    for name, x in SENSORS.items():
        cmd(f'/execute if block {x} {Y} {Z} sculk_sensor[sculk_sensor_phase=inactive,power=0]',
            f'sensor-{name}-settled', 1)
    # The vibration's path from the pig to sensor A is open.
    for x in (5, 6, 7):
        for y in (Y, Y + 1):
            cmd(f'/execute if block {x} {y} {Z} air', f'path-{x}-{y}', 1)
    cmd(f'/execute if block {SUFFOCATING_X - 0.5:.0f} {Y - 1} {Z} stone', 'suffocating-floor', 1)
    tags = '{NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Rotation:[0f,0f],Motion:[0d,0d,0d]'
    cmd(f'/summon pig {PIG_X} {Y} {Z + 0.5} {tags},UUID:{uuid_ints(1)}}}', 'pig', admission=False)
    cmd(f'/summon armor_stand {STAND_X} {Y} {Z + 0.5} {{NoGravity:1b,UUID:{uuid_ints(2)}}}', 'stand',
        admission=False)
    cmd(f'/summon pig {SUFFOCATING_X} {Y} {Z + 0.5} {tags},Health:1f,UUID:{uuid_ints(3)}}}', 'suffocating',
        admission=False)
    cmd(f'/setblock {SUFFOCATING_X - 0.5:.0f} {Y} {Z} stone', 'suffocating-stone', 1)
    cmd(f'/execute if entity {PIG}', 'pig-exists', 1)
    cmd(f'/execute if entity @e[type=pig,limit=1,x={PIG_X},y={Y},z={Z + 0.5},distance=..0.01]', 'pig-at', 1)
    cmd(f'/data get entity {PIG} Health', 'pig-health', 10)
    cmd(f'/execute if data entity {PIG} {{NoAI:1b,NoGravity:1b}}', 'pig-flags', 1)
    cmd(f'/data get entity {PIG} Pos[0]', 'pig-x', 5)
    cmd(f'/data get entity {PIG} Pos[1]', 'pig-y', Y)
    cmd(f'/data get entity {PIG} Pos[2]', 'pig-z', Z)
    cmd(f'/execute if entity @e[type=armor_stand,limit=1,x={STAND_X},y={Y},z={Z + 0.5},distance=..0.01]',
        'stand-at', 1)
    cmd(f'/data get entity {SUFFOCATING} Health', 'suffocating-health', 1)
    cmd(f'/execute if block {SUFFOCATING_X - 0.5:.0f} {Y} {Z} stone', 'suffocating-in-stone', 1)
    # Nothing has reached a sensor yet.
    for name, x in SENSORS.items():
        cmd(f'/execute if block {x} {Y} {Z} sculk_sensor[sculk_sensor_phase=inactive,power=0]',
            f'sensor-{name}-still-inactive', 1)
    cmd(f'/data get block {SENSORS["a"]} {Y} {Z} listener', 'diag-a-listener-before', admission=False,
        diagnostic=True)
    cmd('/scoreboard objectives add h251 dummy', 'objective')
    cmd('/execute store result score start h251 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    def sensor(name, key, admission=False):
        x = SENSORS[name]
        prefix = 'target-' if name == 'a' else ''
        cmd(f'/execute if block {x} {Y} {Z} sculk_sensor[sculk_sensor_phase=active]',
            f'{prefix}{key}-{name}-active', admission=admission)
        cmd(f'/execute if block {x} {Y} {Z} sculk_sensor[sculk_sensor_phase=cooldown]',
            f'{prefix}{key}-{name}-cooldown', admission=admission)
        cmd(f'/data get block {x} {Y} {Z} last_vibration_frequency', f'{prefix}{key}-{name}-frequency',
            admission=admission)
        for power in (0, 10):
            cmd(f'/execute if block {x} {Y} {Z} sculk_sensor[power={power}]', f'{prefix}{key}-{name}-power{power}',
                admission=admission)
        cmd(f'/data get block {x} {Y} {Z} listener', f'{prefix}{key}-{name}-listener', admission=False,
            diagnostic=True)
        pending(name, f'{prefix}{key}', admission=admission)

    def pending(name, key, relative=False, admission=False):
        # The listener without its absolute clock: the selected candidate and
        # the travelling vibration (VibrationSystem.Data), field by field. The
        # source of a sensor other than A's is a separate diagnostic (`diag-`).
        x = SENSORS[name]
        source = key if name == 'a' else f'diag-source-{key}'
        for part, path in (('selected', 'listener.selector.event'), ('travelling', 'listener.event')):
            for field in ('game_event', 'pos', 'distance'):
                cmd(f'/data get block {x} {Y} {Z} {path}.{field}', f'{key}-{name}-{part}-{field}',
                    admission=admission)
            cmd(f'/data get block {x} {Y} {Z} {path}.source', f'{source}-{name}-{part}-source', admission=False)
        cmd(f'/data get block {x} {Y} {Z} listener.event_delay', f'{key}-{name}-delay', admission=admission)
        if relative:
            # The tick the candidate was selected on, from the scene's start.
            cmd(f'/execute store result score sel h251 run data get block {x} {Y} {Z} listener.selector.tick',
                f'{key}-{name}-selected-tick-raw', admission=False, diagnostic=True)
            cmd('/scoreboard players operation sel h251 -= start h251', f'{key}-{name}-selected-tick',
                admission=admission)

    # A /kill that finds nothing fails and posts nothing.
    cmd('/kill @e[type=pig,tag=h251_none]', 'no-target-kill', admission=False)
    cmd(f'/data get block {SENSORS["a"]} {Y} {Z} listener', 'diag-a-listener-after-no-target', admission=False,
        diagnostic=True)
    pending('a', 'no-target')
    # The positive control: the stand's ENTITY_DIE reaches sensor B's listener.
    cmd(f'/kill {STAND}', 'stand-kill', 1)
    cmd(f'/execute unless entity {STAND}', 'stand-gone', 1)
    cmd(f'/data get block {SENSORS["b"]} {Y} {Z} listener', 'diag-b-listener-after-stand', admission=False,
        diagnostic=True)
    pending('b', 'control-stand', relative=True, admission=True)
    # The target: the pig is killed by the command.
    cmd(f'/kill {PIG}', 'target-pig-kill', admission=False)
    cmd(f'/data get entity {PIG} Health', 'target-pig-health-0', admission=False)
    cmd(f'/data get block {SENSORS["a"]} {Y} {Z} listener', 'target-a-listener-0', admission=False,
        diagnostic=True)
    pending('a', 'target-t0', relative=True)
    cmd(f'/data get entity {PIG} DeathTime', 'diag-pig-deathtime-0', admission=False)
    cmd(f'/execute if entity @e[type=item,x={PIG_X},y={Y},z={Z + 0.5},distance=..2]', 'diag-items-0',
        admission=False)
    cmd(f'/execute if entity @e[type=experience_orb,x={PIG_X},y={Y},z={Z + 0.5},distance=..2]', 'diag-orbs-0',
        admission=False)
    # Killed again: the dead pig takes no second hit, and no second event.
    cmd(f'/kill {PIG}', 'target-pig-kill-again', admission=False)
    cmd(f'/data get block {SENSORS["a"]} {Y} {Z} listener', 'target-a-listener-0-again', admission=False,
        diagnostic=True)
    pending('a', 'target-t0-again', relative=True)
    cmd('/execute store result score now h251 run time query gametime', 'diag-clock-0',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h251 -= start h251', 'clock-0', 0)
    for tick in range(1, STEPS + 1):
        cmd('/tick step 1', f'step-{tick}')
        ops.append({'op': 'tick', 'n': 1})
        cmd('/execute store result score now h251 run time query gametime', f'diag-clock-raw-{tick}',
        admission=False, diagnostic=True)
        cmd('/scoreboard players operation now h251 -= start h251', f'clock-{tick}', tick)
        sensor('a', f't{tick}')
        sensor('b', f'control-t{tick}', admission=True)
        sensor('c', f'diag-ordinary-t{tick}')
        cmd(f'/execute if entity {PIG}', f'diag-pig-body-t{tick}', admission=False)
        cmd(f'/data get entity {PIG} DeathTime', f'diag-pig-deathtime-t{tick}', admission=False)
        cmd(f'/execute if entity @e[type=item,x={PIG_X},y={Y},z={Z + 0.5},distance=..2]',
            f'diag-items-t{tick}', admission=False)
        cmd(f'/execute if entity @e[type=experience_orb,x={PIG_X},y={Y},z={Z + 0.5},distance=..2]',
            f'diag-orbs-t{tick}', admission=False)
        cmd(f'/execute if entity {SUFFOCATING}', f'diag-ordinary-body-t{tick}', admission=False)
        cmd(f'/data get entity {SUFFOCATING} Health', f'diag-ordinary-health-t{tick}', admission=False)
    # The body goes twenty ticks after the death (LivingEntity.tickDeath).
    elapsed = STEPS
    for tick in (19, 20, 21, 22):
        cmd(f'/tick step {tick - elapsed}', f'step-to-{tick}')
        ops.append({'op': 'tick', 'n': tick - elapsed})
        elapsed = tick
        cmd('/execute store result score now h251 run time query gametime', f'diag-clock-raw-{tick}',
        admission=False, diagnostic=True)
        cmd('/scoreboard players operation now h251 -= start h251', f'clock-{tick}', tick)
        cmd(f'/execute if entity {PIG}', f'diag-pig-body-t{tick}', admission=False)
        sensor('a', f'late-t{tick}')
    return ops
