"""Frozen-world contact via real player ticks and read-only player progress."""
from command_exec_corpus import cmd

IDS = ['h311:any', 'h311:fish_source', 'h311:wrong_source']


def advancements():
    def criterion(source=None):
        conditions={'effects':{'minecraft:poison':{}}}
        if source is not None:
            predicate={'minecraft:entity_type':source}
            if source=='minecraft:pufferfish':
                predicate['minecraft:distance']={'absolute':{'max':0.01}}
            conditions['source']={'type':'minecraft:entity_properties','entity':'this','predicate':predicate}
        return {'criteria':{'poison':{'trigger':'minecraft:effects_changed','conditions':conditions}}}
    return dict(zip(IDS,[criterion(),criterion('minecraft:pufferfish'),criterion('minecraft:axolotl')]))


def contact_fixture(raw_bodies=False):
    out=[]
    def run(command,expected=None):
        out.append(cmd(command,expect_result=expected))
    def state(label,health=20,poison=0,done=()):
        out.append({'op':'player_state','label':label,'expect_observation':{
            'health':health,'poison_ticks':poison,'loaded':IDS,
            'done':{i:i in done for i in IDS}}})
    def loop(label,n):
        out.append({'op':'player_loop','label':label,'n':n,'expect_result':n})
    def body(command,label):
        op=cmd(command);op["label"]=label;return op
    def clock_start():
        out.append(cmd('/execute store result score start contact_clock run time query gametime',record=False))
    def clock_check():
        out.extend([cmd('/execute store result score elapsed contact_clock run time query gametime',record=False),
                    cmd('/scoreboard players operation elapsed contact_clock -= start contact_clock',record=False),
                    cmd('/scoreboard players get elapsed contact_clock',expect_result=0)])
    run('/tick rate 20',20);run('/tick freeze',1)
    run('/difficulty normal');run('/difficulty',2)
    run('/gamerule spawn_mobs false');run('/gamerule advance_time false')
    run('/gamerule show_advancement_messages false')
    run('/scoreboard objectives add contact_clock dummy',1)
    run('/forceload add -16 -16 -1 -1',1);out.append({'op':'settle'})
    run('/execute if loaded -11 100 -11',1)
    run('/fill -16 99 -16 -1 99 -1 stone')
    run('/fill -16 100 -16 -1 112 -1 air')
    run('/gamemode survival');run('/tp @s -10.5 100 -10.5 0 0')
    run('/execute if entity @s[type=player,gamemode=survival]',1)
    run('/execute if data entity @s UUID',1)
    run('/data get entity @s UUID',4)
    run('/data get entity @s Pos');run('/data get entity @s Health',20)
    state('initial')
    for i in IDS:
        run('/advancement grant @s only '+i,1)
        state('grant-'+i,done=(i,))
        run('/advancement revoke @s only '+i,1)
        state('reset-'+i)
    cases=[('noai',2,True,False,False),('ai',2,False,False,False),
           ('deflated',0,True,False,False),('separated',2,True,True,False),
           ('creative',2,True,False,True)]
    for number,(name,puff,noai,separated,creative) in enumerate(cases,1):
        # Old fish remain in the frozen world, away from the player contact query.
        run('/tp @e[type=pufferfish] -14.5 100 -14.5')
        run('/gamemode creative');run('/effect clear @s')
        run('/effect give @s instant_health 1 10 true')
        clock_start();loop(name+'-warmup',24);clock_check()
        run('/effect clear @s');run('/tp @s -10.5 100 -10.5 0 0')
        run('/gamemode '+('creative' if creative else 'survival'))
        for i in IDS: run('/advancement revoke @s only '+i)
        state(name+'-reset')
        run('/execute if entity @s[gamemode='+('creative' if creative else 'survival')+']',1)
        run('/data get entity @s Pos');run('/data get entity @s Health',20)
        run('/execute unless entity @e[type=pufferfish,distance=..2]',1)
        x=-6.5 if separated else -10.5
        fields=f'UUID:[I;311,0,0,{number}],PuffState:{puff},PersistenceRequired:1b,Tags:["contact_{number}"]'
        if noai: fields+=',NoAI:1b'
        run(f'/summon pufferfish {x} 100 -10.5 {{{fields}}}',1)
        fish=f'00000137-0000-0000-0000-{number:012x}'
        run(f'/execute if entity @e[type=pufferfish,tag=contact_{number},nbt={{UUID:[I;311,0,0,{number}],Health:3f,PuffState:{puff}}}]',1)
        run(f'/data get entity {fish} Pos')
        run(f'/execute '+('if' if noai else 'unless')+f' data entity {fish} NoAI',1)
        run('/execute if block -11 99 -11 stone',1)
        if raw_bodies:
            out += [body('/data get entity @s',label=name+'-body-before-player'),body(f'/data get entity {fish}',label=name+'-body-before-fish')]
        clock_start();loop(name+'-contact',1);clock_check()
        positive=name in ('noai','ai')
        state(name+'-after',health=17 if positive else 20,poison=120 if positive else 0,
              done=('h311:any','h311:fish_source') if positive else ())
        run(f'/execute if entity @e[type=pufferfish,tag=contact_{number},nbt={{Health:3f,PuffState:{puff}}}]',1)
        if raw_bodies:
            out += [body('/data get entity @s',label=name+'-body-after-player'),body(f'/data get entity {fish}',label=name+'-body-after-fish')]
        if name=='noai':
            run('/effect clear @s')
            for i in IDS: run('/advancement revoke @s only '+i)
            state('cooldown-reset',health=17)
            clock_start();loop('cooldown-rejected',1);clock_check()
            state('cooldown-after',health=17)
            if raw_bodies:
                out += [body('/data get entity @s',label='cooldown-body-after-player'),body(f'/data get entity {fish}',label='cooldown-body-after-fish')]
    return out
