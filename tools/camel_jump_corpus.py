"""A ridden camel's START_RIDING_JUMP and its entity_action game event.

`ServerGamePacketListenerImpl.handlePlayerCommand` START_RIDING_JUMP calls
the controlled vehicle's `handleStartJump` when `canJump()` and the jump's
data is above zero. `Camel.canJump` is `!refuseToMove() && isSaddled()`, and
`Camel.handleStartJump` plays the dash sound, posts `GameEvent.ENTITY_ACTION`
from the camel and sets `DASH`.

The world is frozen; ticks are stepped. Each scene has its own camel (NoAI,
fixed UUID) on a stone floor in its own chunk, and a sculk sensor three
blocks from it, with a NoAI pig two blocks from the sensor. The connected
player mounts the camel (`/ride`), then `/damage` hurts the pig: the
sensor's own `entity_damage` shows that it hears the scene, and 80 ticks
later it is inactive with an empty listener. Then the player's real
ServerboundPlayerCommandPacket START_RIDING_JUMP arrives, and the sensor's
listener is read before the next tick and after it. Scenes: saddled with
power 100, saddled with power 0, unsaddled with power 100.

Label classes, declared before any native run: `target-` rows read the
sensor after the saddled camel's jump with power; `ctl-` rows read it after
the two jumps that must not act; `index-` rows read the pig's damage event (the
sensor hears the scene); `diag-` rows are recorded diagnostics (among them the
mount's own event and the rider's `RootVehicle`); every other
row is a setup admission or a control.
"""

Y = 100
Z = 8
# name, UUID tail, camel x, saddled, power
SCENES = [
    ('saddled', 1, 8, True, 100),
    ('zero', 2, 40, True, 0),
    ('unsaddled', 3, 72, False, 100),
]


def uuid(n):
    return f'0000ca3e-0000-0000-0000-00000000000{n}'


def camel_jump(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    def step(n, label):
        cmd(f'/tick step {n}', label)
        ops.append({'op': 'tick', 'n': n})

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0 79 15', 'force', 5)
    ops.append({'op': 'settle', 'required_chunks': [[c, 0] for c in range(5)]})
    for c in range(5):
        cmd(f'/execute if loaded {c * 16 + 8} {Y} {Z}', f'loaded-{c}', 1)
    cmd(f'/fill 0 {Y - 1} 0 79 {Y - 1} 15 stone', 'floor', 1280)
    cmd(f'/fill 0 {Y} 0 79 {Y + 4} 15 air', 'clear', admission=False)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/kill @e[type=item]', 'clear-items', admission=False, diagnostic=True)
    cmd('/gamemode creative @p', 'mode', admission=False)
    cmd('/execute if entity @p[gamemode=creative]', 'mode-is', 1)
    for name, n, x, saddled, _ in SCENES:
        cmd(f'/setblock {x + 3} {Y} {Z} sculk_sensor', f'{name}-sensor', 1)
        gear = ',equipment:{saddle:{id:"minecraft:saddle",count:1}}' if saddled else ''
        cmd(f'/summon camel {x + 0.5} {Y} {Z + 0.5} {{NoAI:1b,PersistenceRequired:1b,Silent:1b,Rotation:[0f,0f],'
            f'UUID:[I;51774,0,0,{n}]{gear}}}', f'{name}-summon', admission=False)
        cmd(f'/summon pig {x + 3.5} {Y} {Z + 2.5} {{NoAI:1b,PersistenceRequired:1b,Silent:1b,'
            f'UUID:[I;51774,0,1,{n}]}}', f'{name}-pig', admission=False)
    # A new sensor joins the listeners within its first ticks (a separate difference).
    step(12, 'settle-sensors')
    cmd('/scoreboard objectives add hc dummy', 'objective')
    cmd('/execute store result score start hc run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    for name, n, x, saddled, power in SCENES:
        sensor = f'{x + 3} {Y} {Z}'
        cmd(f'/execute as {uuid(n)} if entity @s[type=camel,x={x + 0.5},y={Y},z={Z + 0.5},distance=..0.01]',
            f'{name}-at', 1)
        cmd(f'/execute if block {x} {Y - 1} {Z} stone', f'{name}-floor', 1)
        for field in ('Health', 'Age', 'equipment', 'NoAI', 'Silent'):
            cmd(f'/data get entity {uuid(n)} {field}', f'{name}-{field.lower()}', admission=False)
        cmd(f'/data get entity {uuid(n)} LastPoseTick', f'diag-{name}-pose-tick', admission=False)
        cmd(f'/execute if block {sensor} sculk_sensor[sculk_sensor_phase=inactive]', f'{name}-sensor-inactive', 1)
        cmd(f'/tp @p {x + 0.5} {Y} {Z + 2.5}', f'{name}-tp', admission=False)
        cmd(f'/ride @p mount {uuid(n)}', f'{name}-mount', admission=False)
        cmd(f'/execute as @p on vehicle if entity @s[type=camel]', f'{name}-riding', admission=False)
        cmd(f'/execute as @p on vehicle if entity {uuid(n)}', f'{name}-riding-it', admission=False)
        cmd('/data get entity @p RootVehicle.Attach', f'diag-{name}-root-vehicle', admission=False,
            diagnostic=True)
        cmd('/data get entity @p Health', f'{name}-player-health', admission=False)
        cmd(f'/data get block {sensor} listener.selector.event.game_event', f'diag-{name}-mount-selected',
            admission=False, diagnostic=True)
        # The pig's damage: the sensor hears this scene.
        cmd(f'/damage 0000ca3e-0000-0000-0000-00010000000{n} 1', f'{name}-pig-damage', 1)
        cmd(f'/data get block {sensor} listener.selector.event.game_event', f'index-{name}-damage-selected',
            admission=False)
        step(1, f'{name}-index-step')
        cmd(f'/data get block {sensor} listener.event.game_event', f'index-{name}-damage-travelling',
            admission=False)
        # The vibration ends and the sensor rests before the jump.
        step(80, f'{name}-rest')
        cmd(f'/execute if block {sensor} sculk_sensor[sculk_sensor_phase=inactive]', f'{name}-rested',
            admission=False)
        cmd(f'/data get block {sensor} listener.selector.event', f'{name}-quiet-selected', admission=False)
        cmd(f'/data get block {sensor} listener.event', f'{name}-quiet-travelling', admission=False)
        cmd(f'/execute as @p on vehicle if entity {uuid(n)}', f'{name}-still-riding', admission=False)
        cmd(f'/data get entity {uuid(n)} Pos', f'diag-{name}-camel-pos', admission=False)
        cmd('/execute store result score now hc run time query gametime', f'diag-{name}-clock-raw',
            admission=False, diagnostic=True)
        cmd('/scoreboard players operation now hc -= start hc', f'{name}-clock', admission=False)
        ops.append({'op': 'player_command', 'action': 'start_riding_jump', 'data': power})
        prefix = 'target-' if name == 'saddled' else 'ctl-'
        for field in ('game_event', 'pos', 'distance'):
            cmd(f'/data get block {sensor} listener.selector.event.{field}', f'{prefix}{name}-jump-selected-{field}',
                admission=False)
        step(1, f'{name}-jump-step')
        for field in ('game_event', 'pos', 'distance'):
            cmd(f'/data get block {sensor} listener.event.{field}', f'{prefix}{name}-jump-travelling-{field}',
                admission=False)
        cmd(f'/data get block {sensor} listener', f'diag-{name}-listener', admission=False, diagnostic=True)
        cmd(f'/data get entity {uuid(n)} Pos', f'diag-{name}-camel-pos-after', admission=False)
        cmd('/ride @p dismount', f'{name}-dismount', admission=False)
        step(1, f'{name}-dismount-step')
    return ops
