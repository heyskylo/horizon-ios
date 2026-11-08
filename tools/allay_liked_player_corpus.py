"""Allays saved with a `minecraft:liked_player` memory, across world ticks (H095).

`Brain.forgetOutdatedMemories` ticks every memory in `Brain.tick`, which an
Allay runs in `customServerAiStep`: a memory with a time to live counts
down and is cleared on the tick after it reaches 0 (`MemorySlot.tick`); a
memory without one never expires. `NoAI` skips the brain, so its memory
keeps its time to live. Each Allay is bound to its own UUID and tag and is
read at 0, 1, 2 and 3 world ticks; positions are diagnostics.
"""

OWNER = '[I;95,0,0,99]'
Y = 200
# name, UUID suffix, NoAI, memory (None: absent; 'permanent'; or a ttl), x
FORMS = [('absent', None), ('permanent', 'permanent'), ('finite2', 2), ('zero', 0)]
ACTORS = [(f'{form}_{"noai" if noai else "ai"}', 1 + i * 2 + noai, noai, memory, 2.5 + i * 3, 4.5 + noai * 6)
          for i, (form, memory) in enumerate(FORMS) for noai in (0, 1)]
TICKS = [0, 1, 2, 3]


def uuid_of(n):
    return f'0000005f-0000-0000-0000-{n:012}'


def allay_liked_player(full=False):
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
    for rule, value in [('spawn_mobs', 'false'), ('advance_time', 'false'), ('random_tick_speed', '0'),
                        ('advance_weather', 'false')]:
        cmd(f'/gamerule {rule} {value}', 'rule-' + rule, admission=False)
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/tp @s 8.5 {Y + 20} 8.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    cmd('/scoreboard objectives add h095 dummy', 'objective')
    for name, n, noai, memory, x, z in ACTORS:
        brain = ''
        if memory == 'permanent':
            brain = f',Brain:{{memories:{{"minecraft:liked_player":{{value:{OWNER}}}}}}}'
        elif memory is not None:
            brain = f',Brain:{{memories:{{"minecraft:liked_player":{{value:{OWNER},ttl:{memory}L}}}}}}'
        cmd(f'/summon allay {x} {Y} {z} {{UUID:[I;95,0,0,{n}],Tags:["h095_{name}"],NoAI:{noai}b,'
            f'PersistenceRequired:1b,NoGravity:1b,Motion:[0d,0d,0d]{brain}}}', name + '-summon', 1)
    cmd('/execute store result score start h095 run time query gametime', 'clock-start', diagnostic=True)
    elapsed = 0
    for at in TICKS:
        if at:
            n = at - elapsed
            cmd(f'/tick step {n}', f'{at}-step')
            ops.append({'op': 'tick', 'n': n})
        elapsed = at
        key = str(at)
        cmd('/execute if entity @s[type=player,name=Player]', key + '-owner', 1)
        cmd(f'/execute if block 4 {Y - 1} 4 stone', key + '-floor', 1)
        cmd(f'/execute if loaded 8 {Y} 8', key + '-loaded', 1)
        cmd('/execute store result score now h095 run time query gametime', key + '-clock-raw', diagnostic=True)
        cmd('/scoreboard players operation now h095 -= start h095', key + '-clock-op', at)
        cmd('/scoreboard players get now h095', key + '-clock', at)
        for name, n, noai, memory, x, z in ACTORS:
            uuid = uuid_of(n)
            label = f'{key}-{name}'
            cmd(f'/execute as {uuid} if entity @s[type=allay,tag=h095_{name}]', label + '-type-tag', 1)
            for field, value in [('UUID', 4), ('Health', 20), ('PersistenceRequired', 1)]:
                cmd(f'/data get entity {uuid} {field}', f'{label}-{field}', value)
            cmd(f'/data get entity {uuid} NoAI', label + '-NoAI',
                result=1 if noai else None, callback=None if noai else [False, 0])
            # The memory under test: present, its owner and its time to live.
            liked = 'Brain.memories."minecraft:liked_player"'
            cmd(f'/execute if data entity {uuid} {liked}', label + '-present', admission=False)
            cmd(f'/execute if data entity {uuid} {{Brain:{{memories:{{"minecraft:liked_player":{{value:{OWNER}}}}}}}}}',
                label + '-owner-value', admission=False)
            cmd(f'/data get entity {uuid} {liked}.ttl', label + '-ttl', admission=False)
            cmd(f'/execute store success score {name} h095 if data entity {uuid} {liked}',
                label + '-store-op', admission=False)
            cmd(f'/scoreboard players get {name} h095', label + '-store', admission=False)
            cmd(f'/data get entity {uuid} Pos', label + '-Pos', diagnostic=True)
            cmd(f'/data get entity {uuid} Brain', label + '-brain', diagnostic=True)
    return ops
