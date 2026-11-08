"""TraderLlama timer through actual frozen command/tick execution.

Full expiry observations stay executable. Any narrower timer gate needs an
independently established H182 boundary and a declaration before fixed runs.
"""
import uuid


def llama_delay(cmd, diagnostic=False):
    cases = [
        ('free_ai5', False, 5, ''), ('free_noai5', True, 5, ''),
        ('zero_ai', False, 0, ''), ('zero_noai', True, 0, ''),
        ('default_ai', False, None, ''), ('default_noai', True, None, ''),
        ('tame_noai', True, 5, 'Tame:1b'),
        ('persistent_noai', True, 5, 'PersistenceRequired:1b'),
    ]
    out = [cmd('/tick freeze'), cmd('/scoreboard objectives add h181 dummy', expect_result=1),
           cmd('/forceload add 0 0 63 31', expect_result=8), {'op': 'settle'}]
    for x in (0, 16, 32, 48):
        for z in (0, 16):
            out.append(cmd(f'/execute if loaded {x} 200 {z}', expect_result=1))
    out += [cmd('/fill 0 199 0 63 207 15 barrier', expect_result=9216),
            cmd('/fill 0 200 0 63 207 15 air', expect_result=8192),
            cmd('/fill 0 199 0 63 199 15 stone', expect_result=1024),
            cmd('/tp @s 28.5 204 12.5', expect_result=1),
            cmd('/data get entity @s Pos', expect_result=3),
            cmd('/execute store result score #start h181 store success score #clock h181 run time query gametime', False),
            cmd('/scoreboard players get #clock h181', expect_result=1)]
    actors = []
    for i, (label, noai, delay, keep) in enumerate(cases, 1):
        actor = str(uuid.UUID(int=(181 << 96) + i));x = 4.5 + 6 * (i - 1)
        actors.append((actor, x, label, noai, delay, keep))
        fields = f'UUID:[I;181,0,0,{i}],Tags:["h181","{label}"],NoGravity:1b,Silent:1b,Health:30f,attributes:[{{id:"minecraft:max_health",base:30d}}],Motion:[0d,0d,0d]'
        if noai: fields += ',NoAI:1b'
        if delay is not None: fields += f',DespawnDelay:{delay}'
        if keep: fields += ',' + keep
        out += [cmd(f'/summon minecraft:trader_llama {x} 200 4.5 ' + '{' + fields + '}', expect_result=1),
                cmd(f'/execute as {actor} if entity @s[type=minecraft:trader_llama,tag={label},nbt={{UUID:[I;181,0,0,{i}],Pos:[{x}d,200d,4.5d],Health:30f,NoGravity:1b,Silent:1b}}]', expect_result=1),
                cmd(f'/execute if block {int(x)} 199 4 stone', expect_result=1),
                cmd(f'/execute if loaded {int(x)} 200 4', expect_result=1)]
        out.append(cmd(f'/execute if data entity {actor} NoAI', expect_result=1) if noai else cmd(f'/execute unless data entity {actor} NoAI', expect_result=1))
        if keep:out.append(cmd(f'/execute as {actor} if entity @s[nbt={{{keep}}}]', expect_result=1))
        if diagnostic:out.append(cmd(f'/data get entity {actor}'))
    for elapsed, advance in [(0, 0), (1, 1), (2, 1), (4, 2), (5, 1)]:
        if advance:
            out += [cmd(f'/tick step {advance}', expect_result=1), {'op': 'tick', 'n': advance}]
        out += [cmd('/execute store result score #elapsed h181 store success score #clock h181 run time query gametime', False),
                cmd('/scoreboard players operation #elapsed h181 -= #start h181', False),
                cmd('/scoreboard players get #clock h181', expect_result=1),
                cmd('/scoreboard players get #elapsed h181', expect_result=elapsed)]
        for actor, x, label, noai, delay, keep in actors:
            initial = 47999 if delay is None else delay
            alive = bool(keep) or elapsed == 0 or initial > elapsed
            if alive:
                value = initial if keep else initial - elapsed
                out += [cmd(f'/execute as {actor} if entity @s[type=minecraft:trader_llama,tag={label}]', expect_result=1),
                        cmd(f'/data get entity {actor} Health', expect_result=30),
                        cmd(f'/data get entity {actor} DespawnDelay', expect_result=value)]
                if noai:out.append(cmd(f'/execute as {actor} if entity @s[nbt={{Pos:[{x}d,200d,4.5d],NoAI:1b,NoGravity:1b}}]', expect_result=1))
                if diagnostic:out.append(cmd(f'/data get entity {actor} Pos', expect_result=3))
            else:
                out += [cmd(f'/execute unless entity {actor}', expect_result=1),
                        cmd(f'/execute unless entity @e[type=minecraft:trader_llama,tag={label}]', expect_result=1),
                        cmd('/scoreboard players set #r h181 31', expect_result=31),
                        cmd('/scoreboard players set #s h181 17', expect_result=17),
                        cmd(f'/execute store result score #r h181 store success score #s h181 run data get entity {actor} DespawnDelay'),
                        cmd('/scoreboard players get #r h181'), cmd('/scoreboard players get #s h181')]
    out += [cmd('/kill @e[tag=h181]'), cmd('/scoreboard objectives remove h181'),
            cmd('/forceload remove 0 0 63 31', expect_result=8), cmd('/tick unfreeze')]
    return out
