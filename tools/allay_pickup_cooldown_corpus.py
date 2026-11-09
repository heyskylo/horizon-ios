"""An allay saves its item pickup cooldown memory.

`AllayAi` sets `ITEM_PICKUP_COOLDOWN_TICKS` to 60 when it throws an item to its liked player; the core
activity's `CountDownCooldownTicks` counts it down while it is above 0 and erases it at 0 (`stop`). The
Brain saves a memory as `{value: …}` (no `ttl` for a memory set without one) and reads it back. NoAI
skips the Brain tick.

The world is frozen; ticks are stepped. Allays with fixed UUIDs fly in a loaded glass room:
- `supplied` (AI) and `frozen` (NoAI) are summoned with `item_pickup_cooldown_ticks` `{value:30}`;
- `plain` (AI) has no memory;
- `natural` (AI) gets a diamond from the real player (`mob_interact`), then a matching diamond item
  appears beside it; it fetches it and throws it to the player, which starts the cooldown.

Label classes, declared before any native run: `target-` rows read the cooldown memory (`supplied`,
`frozen`, `plain`) and the full Brain; `rand-` rows are recorded and compared, but their tick depends
on the natural delivery's flight (the `natural` allay's memory and pocket per tick); `diag-` rows are
recorded diagnostics (clock, positions); every other row is a setup admission or a control.
"""

Y = 100
KEY = 'Brain.memories."minecraft:item_pickup_cooldown_ticks"'


def uuid(n):
    return f'00006096-0000-0000-0000-{n:012x}'


ALLAYS = [('supplied', 1, 4.5, False, 30), ('frozen', 2, 7.5, True, 30), ('plain', 3, 10.5, False, None),
          ('natural', 4, 13.5, False, None)]
DIAMOND = uuid(9)
TICKS = 80


def allay_pickup_cooldown(full=False):
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
                        ('advance_weather', 'false'), ('mob_griefing', 'true')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0 15 15', 'force', admission=False)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/fill 0 {Y} 0 15 {Y + 5} 15 air', 'clear', admission=False)
    cmd(f'/fill 0 {Y} 0 15 {Y + 5} 0 glass', 'wall-n', admission=False)
    cmd(f'/fill 0 {Y + 6} 0 15 {Y + 6} 15 glass', 'roof', 256)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd('/gamemode survival', 'survival', admission=False)
    cmd(f'/tp @s 13.5 {Y} 4.5 0 0', 'player-at-set', admission=False)
    cmd(f'/execute if entity @s[x=13.5,y={Y},z=4.5,distance=..0.001]', 'player-at', 1)
    cmd('/clear @s', 'player-clear', admission=False)
    cmd('/data get entity @s UUID', 'diag-player-uuid', admission=False, diagnostic=True)
    for name, n, x, no_ai, value in ALLAYS:
        brain = (f',Brain:{{memories:{{"minecraft:item_pickup_cooldown_ticks":{{value:{value}}}}}}}'
                 if value is not None else '')
        cmd(f'/summon allay {x} {Y + 1} 8.5 {{NoAI:{int(no_ai)}b,PersistenceRequired:1b,Silent:1b,Health:20f,'
            f'NoGravity:1b,Rotation:[0f,0f],Motion:[0d,0d,0d],UUID:[I;24726,0,0,{n}]{brain}}}', f'{name}-summon',
            admission=False)
        cmd(f'/execute as {uuid(n)} if entity @s[type=allay,x={x},y={Y + 1},z=8.5,distance=..0.001]',
            f'{name}-is', 1)
        cmd(f'/execute as {uuid(n)} if entity @s[nbt={{NoAI:{int(no_ai)}b}}]' if no_ai else
            f'/execute as {uuid(n)} if entity @s[nbt=!{{NoAI:1b}}]', f'{name}-ai-mode', 1)
        cmd(f'/data get entity {uuid(n)} Health', f'{name}-health', admission=False)
    cmd('/scoreboard objectives add ap dummy', 'objective', 1)
    cmd('/execute store result score #start ap run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)

    def read(label, n, cls='target'):
        cmd(f'/data get entity {uuid(n)} {KEY}', f'{cls}-{label}-cooldown', admission=False)
        cmd(f'/data get entity {uuid(n)} Brain', f'{cls}-{label}-brain', admission=False)

    for name, n, *_ in ALLAYS[:3]:
        read(f'{name}-loaded', n)
    # The natural delivery: the player gives the allay a diamond, then a matching diamond appears.
    cmd('/item replace entity @s weapon.mainhand with minecraft:diamond 1', 'player-diamond', admission=False)
    ops.append({'op': 'mob_interact', 'uuid': uuid(4)})
    cmd(f'/data get entity {uuid(4)} Brain.memories."minecraft:liked_player"', 'natural-liked', admission=False)
    cmd(f'/data get entity {uuid(4)} equipment.mainhand', 'natural-held', admission=False)
    cmd('/data get entity @s SelectedItem', 'player-held-after-give', admission=False)
    cmd(f'/summon item 13.5 {Y} 10.5 {{Item:{{id:"minecraft:diamond",count:1}},PickupDelay:0s,'
        f'Motion:[0d,0d,0d],UUID:[I;24726,0,0,9]}}', 'diamond-summon', admission=False)
    cmd(f'/execute if entity {DIAMOND}', 'diamond-is', 1)
    for tick in range(1, TICKS + 1):
        cmd('/tick step 1', f'step-{tick}', admission=False)
        ops.append({'op': 'tick', 'n': 1})
        cmd(f'/data get entity {uuid(4)} {KEY}', f'rand-natural-t{tick:03d}-cooldown', admission=False)
        cmd(f'/data get entity {uuid(4)} Inventory', f'rand-natural-t{tick:03d}-pocket', admission=False)
        if tick in (1, 10, 29, 30, 31, 40):
            for name, n, *_ in ALLAYS[:3]:
                read(f'{name}-t{tick:03d}', n)
    read('natural-end', 4, 'rand')
    cmd('/execute store result score #now ap run time query gametime', 'diag-clock-now', admission=False,
        diagnostic=True)
    cmd('/scoreboard players operation #now ap -= #start ap', 'clock', TICKS)
    return ops
