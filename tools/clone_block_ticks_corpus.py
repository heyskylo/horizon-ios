"""`/clone` and the block ticks pending in its source box (H237).

`CloneCommands.clone` ends with `copyAreaFrom` on the level's block ticks:
every block tick pending in the source box goes to the same place in the
destination, with its trigger time, before the command reports. A placed
sand schedules its own fall 2 ticks later; a `strict` clone places without
that tick, so the destination sand falls only if the source's pending tick
was copied. The scenes run in a frozen level after one measured warm-up
tick (the chunk's tick queues are registered), all at Y 101 over air:

- strict: fresh sand cloned with `strict`;
- filtered: sand over dirt, cloned `strict filtered minecraft:sand`;
- move: fresh sand moved with `strict replace move`;
- early: sand placed one tick before a non-strict clone, so the copied tick
  (due at +2) comes before the placement's own tick (due at +3);
- queued: the destination already holds its own pending sand tick when the
  strict copy arrives.

Each source sand keeps its own schedule and falls too.
"""

Y = 101
SCENES = [
    # name, source x, destination x, z
    ('strict', 1, 3, 1),
    ('filtered', 5, 7, 1),
    ('move', 9, 11, 1),
    ('early', 1, 3, 5),
    ('queued', 5, 7, 5),
]


def clone_block_ticks(full=False):
    ops = []

    def cmd(text, label, result=None, callback=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if callback is not None:
            op['expect_callback'] = callback
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    for rule, value in [('random_tick_speed', '0'), ('spawn_mobs', 'false'), ('advance_time', 'false'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd('/execute if loaded 8 100 8', 'loaded', 1)
    cmd('/tp @s 8.5 120 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h237 dummy', 'objective')
    cmd('/execute store result score start h237 run time query gametime', 'clock-start', diagnostic=True)

    def clock(label, ticks):
        cmd('/execute store result score now h237 run time query gametime', label + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h237 -= start h237', label + '-clock-op', ticks)
        cmd('/scoreboard players get now h237', label + '-clock', ticks)

    def step(n, at):
        cmd(f'/tick step {n}', f'step-to-{at}')
        ops.append({'op': 'tick', 'n': n})
        clock(f'at{at}', at)

    # The warm-up: one world tick registers the chunk's tick queues, the same
    # on both servers.
    step(1, 1)
    cmd('/fill 0 99 0 15 99 15 stone', 'floor', 256)
    cmd('/fill 0 100 0 15 103 15 air', 'clear', admission=False)
    for name, sx, dx, z in SCENES:
        for x in (sx, dx):
            cmd(f'/execute if block {x} 99 {z} stone', f'{name}-floor-{x}', 1)
            cmd(f'/execute if block {x} 100 {z} air', f'{name}-below-{x}', 1)
    # early: its source sand is placed one tick before its clone.
    cmd(f'/setblock 1 {Y} 5 sand', 'early-source', 1)
    step(1, 2)
    # The others are placed and cloned in the same tick.
    cmd(f'/setblock 1 {Y} 1 sand', 'strict-source', 1)
    cmd(f'/clone 1 {Y} 1 1 {Y} 1 3 {Y} 1 strict replace', 'strict-clone', 1)
    cmd(f'/setblock 5 100 1 dirt', 'filtered-dirt', 1)
    cmd(f'/setblock 5 {Y} 1 sand', 'filtered-source', 1)
    cmd(f'/clone 5 100 1 5 {Y} 1 7 100 1 strict filtered minecraft:sand', 'filtered-clone', 1)
    cmd(f'/execute if block 7 100 1 air', 'filtered-dirt-not-copied', 1)
    cmd(f'/setblock 9 {Y} 1 sand', 'move-source', 1)
    cmd(f'/clone 9 {Y} 1 9 {Y} 1 11 {Y} 1 strict replace move', 'move-clone', 1)
    cmd(f'/execute if block 9 {Y} 1 air', 'move-source-emptied', 1)
    cmd(f'/clone 1 {Y} 5 1 {Y} 5 3 {Y} 5 replace', 'early-clone', 1)
    cmd(f'/setblock 7 {Y} 5 sand', 'queued-destination', 1)
    cmd(f'/setblock 5 {Y} 5 sand', 'queued-source', 1)
    cmd(f'/clone 5 {Y} 5 5 {Y} 5 7 {Y} 5 strict replace', 'queued-clone', 1)
    for name, sx, dx, z in SCENES:
        if name != 'move':
            cmd(f'/execute if block {sx} {Y} {z} sand', f'{name}-source-placed', 1)
        cmd(f'/execute if block {dx} {Y} {z} sand', f'{name}-destination-placed', 1)
    for at in (3, 4, 5):
        step(1, at)
        for name, sx, dx, z in SCENES:
            for role, x in (('source', sx), ('destination', dx)):
                if name == 'move' and role == 'source':
                    continue
                label = f'at{at}-{name}-{role}'
                cmd(f'/execute if block {x} {Y} {z} sand', label + '-sand', admission=False)
                cmd(f'/execute store success score {name}_{role} h237 if block {x} {Y} {z} sand',
                    label + '-store-op', admission=False)
                cmd(f'/scoreboard players get {name}_{role} h237', label + '-store', admission=False)
                cmd(f'/execute if entity @e[type=falling_block,x={x}.5,y={Y - 1},z={z}.5,distance=..1.5]',
                    label + '-falling', admission=False)
                cmd(f'/data get block {x} {Y} {z}', label + '-block', diagnostic=True)
    return ops
