"""Actual world ticks, with contact and rejected-hit controls for Pufferfish.aiStep."""
from command_exec_corpus import cmd


def mob_contacts(durations=False):
    out = []

    def run(command, expected=None, label=None, record=True):
        op = cmd(command, expect_result=expected, record=record)
        if label:
            op['label'] = label
        out.append(op)

    run('/tick freeze', 1)
    for command in ['/gamerule spawn_mobs false', '/gamerule advance_time false',
                    '/gamerule advance_weather false', '/difficulty normal',
                    '/gamemode creative', '/tp @s 10.5 200 20.5 0 0']:
        run(command)
    run('/scoreboard objectives add sting_clock dummy', 1)
    run('/forceload add 0 0 31 31', 4)
    out.append({'op': 'settle', 'required_chunks': [[0, 0], [0, 1], [1, 0], [1, 1]]})
    for x in [0, 16]:
        for z in [0, 16]:
            run(f'/execute if loaded {x} 200 {z}', 1)
    run('/fill 0 198 0 31 199 31 stone')
    run('/fill 0 200 0 31 212 31 air')
    for a, b in [('6 200 6', '6 204 14'), ('14 200 6', '14 204 14'),
                 ('6 200 6', '14 204 6'), ('6 200 14', '14 204 14')]:
        run(f'/fill {a} {b} glass')
    run('/fill 7 200 7 13 203 13 water')
    run('/tp @s 10.5 200 20.5 0 0')
    run('/data get entity @s UUID', 4)
    cases = [('noai', True, 2, 'pig', False, False, False),
             ('ai', False, 2, 'pig', False, False, False),
             ('deflated', True, 0, 'pig', False, False, False),
             ('separated', True, 2, 'pig', True, False, False),
             ('invulnerable', True, 2, 'pig', False, True, False),
             ('cooldown', True, 2, 'pig', False, False, True),
             ('cod', True, 2, 'cod', False, False, False),
             ('undead', True, 2, 'zombie', False, False, False)]
    for i, (name, noai, puff, species, far, immune, cooldown) in enumerate(cases, 1):
        # Frozen old actors are kept alive away from this contact query.
        run('/tp @e[tag=h313_subject] 30.5 201 30.5')
        fish_id, target_id = i * 2, i * 2 + 1
        fish = f'00000139-0000-0000-0000-{fish_id:012x}'
        target = f'00000139-0000-0000-0000-{target_id:012x}'
        fish_flags = ',NoAI:1b' if noai else ''
        run(f'/summon pufferfish 10.5 201 10.5 {{UUID:[I;313,0,0,{fish_id}],'
            f'Tags:["h313_subject","h313_{name}_fish"],PuffState:{puff},'
            f'NoGravity:1b,Silent:1b,PersistenceRequired:1b{fish_flags}}}', 1)
        x = 20.5 if far else 10.5
        health = {'pig': 10, 'cod': 3, 'zombie': 20}[species]
        invulnerable = ',Invulnerable:1b' if immune else ''
        run(f'/summon {species} {x} 200 10.5 {{UUID:[I;313,0,0,{target_id}],'
            f'Tags:["h313_subject","h313_{name}_target"],NoAI:1b,NoGravity:1b,'
            f'Silent:1b,PersistenceRequired:1b{invulnerable}}}', 1)
        if cooldown:
            run(f'/damage {target} 3 generic', 1)
            health = 7
            run(f'/data get entity {target} HurtTime', 10)
        for actor, kind, ident, hp, pos in [
                (fish, 'pufferfish', fish_id, 3, '10.5d,201d,10.5d'),
                (target, species, target_id, health, f'{x}d,200d,10.5d')]:
            run(f'/execute if entity @e[type={kind},nbt={{UUID:[I;313,0,0,{ident}],'
                f'Health:{hp}f,Pos:[{pos}],NoGravity:1b,Silent:1b,PersistenceRequired:1b}}]', 1)
            run(f'/data get entity {actor} Pos')
            run(f'/data get entity {actor} Health', hp)
        run(f'/execute {"if" if noai else "unless"} data entity {fish} NoAI', 1)
        run(f'/execute if data entity {target} NoAI', 1)
        run(f'/data get entity {fish} PuffState', puff)
        if immune:
            run(f'/execute if data entity {target} Invulnerable', 1)
        run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[10.5d,200d,20.5d]}]', 1)
        run('/execute if block 10 199 10 stone', 1)
        run('/execute if block 10 201 10 water[level=0]', 1)
        run(f'/execute unless data entity {target} active_effects[{{id:"minecraft:poison"}}]', 1)
        run('/execute store result score start sting_clock run time query gametime', record=False)
        run('/tick step 2')
        out.append({'op': 'tick', 'n': 2})
        run('/execute store result score elapsed sting_clock run time query gametime', record=False)
        run('/scoreboard players operation elapsed sting_clock -= start sting_clock', record=False)
        run('/scoreboard players get elapsed sting_clock', 2)
        run(f'/execute if entity @e[type={species},nbt={{UUID:[I;313,0,0,{target_id}]}}]', 1)
        run(f'/execute if entity @e[type=pufferfish,nbt={{UUID:[I;313,0,0,{fish_id}],Health:3f}}]', 1)
        expected_health = {'noai': 7, 'ai': 7, 'deflated': 10, 'separated': 10,
                           'invulnerable': 10, 'cooldown': 7, 'cod': 3, 'undead': 17}[name]
        run(f'/data get entity {target} Health', expected_health, label=name + '-health-after')
        run(f'/data get entity {target} Pos', label=name + '-target-pos-after')
        run(f'/data get entity {fish} Pos', label=name + '-fish-pos-after')
        poisoned = name in ('noai', 'ai')
        run(f'/execute {"if" if poisoned else "unless"} data entity {target} '
            'active_effects[{id:"minecraft:poison"}]', 1, label=name + '-poison-presence')
        if poisoned and durations:
            run(f'/data get entity {target} active_effects[{{id:"minecraft:poison"}}].duration',
                118, label=name + '-poison-duration')
        if cooldown:
            run(f'/data get entity {target} HurtTime', 8)
    return out


def overlapping_cod_control():
    """An overlapping pig proves contact is active while the cod is left alone."""
    ops = []

    def run(command, expected=None, record=True):
        ops.append(cmd(command, expect_result=expected, record=record))

    run('/tick freeze', 1)
    for command in ['/gamerule spawn_mobs false', '/gamerule advance_time false',
                    '/gamemode creative', '/forceload add 0 0 15 15']:
        run(command)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    run('/fill 7 199 7 13 199 13 stone')
    run('/fill 7 200 7 13 203 13 water')
    run('/setblock 10 199 5 stone')
    run('/tp @s 10.5 200 5.5 0 0')
    run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[10.5d,200d,5.5d]}]', 1)
    run('/execute if loaded 10 201 10', 1)
    run('/execute if block 10 201 10 water[level=0]', 1)
    run('/scoreboard objectives add overlap_clock dummy', 1)
    for species, ident, health in [('pufferfish', 101, 3), ('pig', 102, 10), ('cod', 103, 3)]:
        puff = ',PuffState:2' if species == 'pufferfish' else ''
        run(f'/summon {species} 10.5 201 10.5 {{UUID:[I;313,0,0,{ident}],'
            f'NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b{puff}}}', 1)
        run(f'/execute if entity @e[type={species},nbt={{UUID:[I;313,0,0,{ident}],'
            f'Health:{health}f,Pos:[10.5d,201d,10.5d],NoAI:1b,NoGravity:1b}}]', 1)
    run('/execute store result score start overlap_clock run time query gametime', record=False)
    run('/tick step 2')
    ops.append({'op': 'tick', 'n': 2})
    run('/execute store result score elapsed overlap_clock run time query gametime', record=False)
    run('/scoreboard players operation elapsed overlap_clock -= start overlap_clock', record=False)
    run('/scoreboard players get elapsed overlap_clock', 2)
    for species, ident, health in [('pufferfish', 101, 3), ('pig', 102, 7), ('cod', 103, 3)]:
        uuid = f'00000139-0000-0000-0000-{ident:012x}'
        run(f'/execute if entity @e[type={species},nbt={{UUID:[I;313,0,0,{ident}]}}]', 1)
        run(f'/data get entity {uuid} Pos')
        run(f'/data get entity {uuid} Health', health)
    run('/execute if data entity 00000139-0000-0000-0000-000000000066 active_effects[{id:"minecraft:poison"}]', 1)
    run('/execute unless data entity 00000139-0000-0000-0000-000000000067 active_effects[{id:"minecraft:poison"}]', 1)
    return ops
