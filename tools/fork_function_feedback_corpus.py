"""Chronological function replies; counters and callbacks are never sorted or masked."""
GROUP = 'fork function feedback'


def functions():
    counter = ('scoreboard players add #calls h155 1\n'
               'scoreboard players add @s h155 1\n'
               'scoreboard players operation #trace h155 *= #ten h155\n'
               'scoreboard players operation #trace h155 += @s h155_id\n')
    out = {f'h155:{name}': counter + tail for name, tail in [
        ('zero', 'return 0\n'), ('five', 'return 5\n'),
        ('fail', 'return fail\n'), ('fallthrough', '')]}
    out['h155:admit'] = 'scoreboard players add #pack h155 1\nreturn 1\n'
    out['h155:chat'] = 'say H155 chat diagnostic\n' + counter + 'return 0\n'
    return out


def cases(cmd, chat=False):
    out = []
    def add(s, n=None, guard=False, diagnostic=None, **kw):
        op = cmd(s) if n is None else cmd(s, expect_result=n)
        if guard: op['h155_admission'] = True
        if diagnostic: op['diagnostic'] = diagnostic
        op.update(kw); out.append(op)
    def actor(n): return f'@e[nbt={{UUID:[I;155,1,0,{n}]}},limit=1]'
    add('/tick freeze', guard=True)
    add('/datapack list enabled', diagnostic='full pack listing; positive named-pack admission', h155_pack=True)
    add('/scoreboard objectives add h155 dummy', 1, True)
    add('/scoreboard objectives add h155_id dummy', 2, True)
    add('/forceload add 0 0 15 15', 1, True); out.append({'op': 'settle'})
    add('/execute if loaded 4 200 4', 1, True)
    add('/fill 0 199 0 15 204 15 barrier', 1536, True)
    add('/fill 0 200 0 15 204 15 air', 1280, True)
    add('/fill 0 199 0 15 199 15 stone', 256, True)
    add('/gamemode spectator', guard=True); add('/tp @s 4.5 202 4.5', 1, True)
    add('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:3,Pos:[4.5d,202d,4.5d]}]', 1, True)
    add('/data get entity @s UUID', 4, True)
    for n, x in [(1, 4.5), (2, 8.5)]:
        add(f'/summon pig {x} 200 4.5 {{UUID:[I;155,1,0,{n}],Tags:["h155"],NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f}}', 1, True)
        add(f'/execute if entity @e[type=pig,nbt={{UUID:[I;155,1,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Pos:[{x}d,200d,4.5d]}}]', 1, True)
        add(f'/data get entity {actor(n)} UUID', 4, True)
        add(f'/execute if block {int(x)} 199 4 stone', 1, True)
        add(f'/scoreboard players set {actor(n)} h155_id {n}', n, True)
        add(f'/data get entity {actor(n)}', diagnostic='complete actual actor body')
    add('/scoreboard players set @s h155_id 3', 3, True)
    add('/scoreboard players set #ten h155 10', 10, True)
    add('/scoreboard players set #pack h155 0', 0, True)
    add('/function h155:admit', 1, True)
    add('/scoreboard players get #pack h155', 1, True)
    add('/execute store result score #before h155 run time query gametime', diagnostic='absolute epoch')
    tests = [(name, route) for name in ['zero', 'five', 'fail', 'fallthrough'] for route in ['direct', 'two']]
    tests += [('five', 'nonfork'), ('zero', 'one')]
    if chat: tests += [('chat', 'two')]
    for name, route in tests:
        for h in ['#calls', '#trace', '@s', actor(1), actor(2)]:
            add(f'/scoreboard players set {h} h155 0', 0, True)
        add('/scoreboard players set #r h155 91', 91, True)
        add('/scoreboard players set #s h155 17', 17, True)
        middle = {'direct': '', 'nonfork': 'positioned 4.5 202 4.5 ',
                  'one': 'as @e[type=pig,tag=h155,sort=nearest,limit=1] ',
                  'two': 'as @e[type=pig,tag=h155,sort=nearest,limit=2] '}[route]
        add(f'/execute store result score #r h155 store success score #s h155 {middle}run function h155:{name}', h155_action=f'{name}/{route}')
        count = 2 if route == 'two' else 1
        add('/scoreboard players get #calls h155', count, True)
        add('/scoreboard players get #trace h155', {'direct': 3, 'nonfork': 3, 'one': 1, 'two': 12}[route], True)
        for h, expected in [('@s', int(route in ['direct', 'nonfork'])),
                            (actor(1), int(route in ['one', 'two'])), (actor(2), int(route == 'two'))]:
            add(f'/scoreboard players get {h} h155', expected, True)
        # Fresh Java admits these controls before any native defect claim.
        result, success = {'zero': (0, 1), 'chat': (0, 1), 'five': (5, 1),
                           'fail': (0, 0), 'fallthrough': (91, 17)}[name]
        add('/scoreboard players get #r h155', result, True)
        add('/scoreboard players get #s h155', success, True)
    add('/scoreboard players set #plain h155 0', 0, True)
    add('/execute as @e[type=pig,tag=h155,sort=nearest,limit=2] run scoreboard players add #plain h155 1', guard=True)
    add('/scoreboard players get #plain h155', 2, True)
    for n, x in [(1, 4.5), (2, 8.5)]:
        add(f'/execute if entity @e[type=pig,nbt={{UUID:[I;155,1,0,{n}],Health:10f,NoAI:1b,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]', 1, True)
        add(f'/data get entity {actor(n)}', diagnostic='complete unchanged actor body')
    add('/execute store result score #after h155 run time query gametime', diagnostic='absolute epoch')
    add('/scoreboard players operation #after h155 -= #before h155')
    add('/scoreboard players get #after h155', 0, True)
    add('/forceload remove 0 0 15 15', 1)
    add('/scoreboard objectives remove h155_id', 1)
    add('/scoreboard objectives remove h155', 0)
    add('/tick unfreeze')
    return out
