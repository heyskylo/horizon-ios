"""Real command-loaded arrow permission scenes; all records remain in the report."""
import uuid


def identity(words):
    return str(uuid.UUID(hex=''.join(f'{n & 0xffffffff:08x}' for n in words)))


def program():
    out = []

    def add(command, label, result=None, *, admission=False, category='control'):
        row = dict(op='cmd', cmd=command, label=label, admission=admission, category=category)
        if result is not None:
            row['expect_result'] = result
        out.append(row)

    def guard(command, label, result=1):
        add(command, label, result, admission=True)

    def observed(command, label, category, expected):
        guard('/scoreboard players set result h197 -99', label+'-reset-result', -99)
        guard('/scoreboard players set success h197 -99', label+'-reset-success', -99)
        add('/execute store result score result h197 store success score success h197 run '+command,
            label, expected, category=category)
        add('/scoreboard players get result h197', label+'-result', expected, category=category)
        add('/scoreboard players get success h197', label+'-success', 1, category=category)

    add('/tick freeze', 'freeze', admission=True)
    for rule, value in [('spawn_mobs','false'), ('random_tick_speed','0'),
                        ('projectiles_can_break_blocks','true'), ('block_drops','true')]:
        add(f'/gamerule {rule} {value}', rule+'-set', admission=True)
        guard(f'/gamerule {rule}', rule+'-read', 1 if value=='true' else 0)
    add('/forceload add 0 0 31 15', 'forceload', admission=True)
    out.append(dict(op='settle', required_chunks=[[0,0],[1,0]]))
    guard('/fill 0 199 0 31 199 15 stone', 'floor', 512)
    guard('/execute if loaded 3 200 1', 'arrow-chunk-loaded')
    guard('/execute if loaded 20 200 14', 'owner-chunk-loaded')
    add('/gamemode survival @s', 'player-mode-set', admission=True)
    guard('/tp @s 29.5 200 14.5 0 0', 'player-position-set')
    guard('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:0,Pos:[29.5d,200d,14.5d]}]', 'player-state')
    add('/data get entity @s UUID', 'player-uuid', admission=True)
    guard('/execute if data entity @s {UUID:[I;-1608632253,-450807400,-1786055591,-2146546415]}', 'player-uuid-admission')
    add('/data get entity @s', 'player-body', category='raw')
    guard('/scoreboard objectives add h197 dummy', 'objective')
    pig = identity([197,0,0,1])
    guard('/summon pig 20.5 200 14.5 {UUID:[I;197,0,0,1],NoAI:1b,NoGravity:1b,Health:10f}', 'owner-summon')
    guard('/execute if entity @e[type=pig,nbt={UUID:[I;197,0,0,1],Health:10f,NoAI:1b,NoGravity:1b,Pos:[20.5d,200d,14.5d]}]', 'owner-state')
    add(f'/data get entity {pig} UUID', 'owner-uuid', 4, admission=True)
    add(f'/data get entity {pig}', 'owner-body', category='raw')
    missing = identity([197,0,0,99])
    guard(f'/execute unless entity {missing}', 'missing-owner-actor')
    cases = [('mob_match_true','mob','chorus_flower',True,True),
             ('mob_deny_true','mob','stone',True,False),
             ('mob_ordinary_true','mob',None,True,False),
             ('mob_match_false','mob','chorus_flower',False,False),
             ('mob_deny_false','mob','stone',False,False),
             ('mob_ordinary_false','mob',None,False,False),
             ('player_ordinary','player',None,False,True),
             ('no_owner_ordinary','none',None,False,True),
             ('missing_actor_owner_field','missing',None,False,True)]
    for i,(label,owner,predicate,griefing,breaks) in enumerate(cases):
        x,z=3+8*(i//4),1+4*(i%4)
        words=[197,0,i+1,2]; arrow=identity(words)
        add(f'/gamerule mob_griefing {str(griefing).lower()}', label+'-griefing-set', admission=True)
        guard('/gamerule mob_griefing', label+'-griefing-read', int(griefing))
        guard('/gamerule projectiles_can_break_blocks', label+'-projectile-rule')
        guard(f'/setblock {x} 199 {z} end_stone', label+'-support-set')
        guard(f'/setblock {x} 200 {z} chorus_flower[age=0]', label+'-flower-set')
        guard(f'/execute if block {x} 200 {z} chorus_flower[age=0]', label+'-initial-flower')
        guard(f'/execute if block {x} 199 {z} end_stone', label+'-support')
        guard(f'/execute if loaded {x} 200 {z}', label+'-loaded')
        region=f'x={x-1},y=199,z={z-1},dx=3,dy=3,dz=3'
        guard('/execute unless entity @e[type=item,'+region+']', label+'-initial-no-items')
        owner_tag = (',Owner:[I;197,0,0,1]' if owner=='mob' else
                     ',Owner:[I;197,0,0,99]' if owner=='missing' else
                     ',Owner:[I;-1608632253,-450807400,-1786055591,-2146546415]' if owner=='player' else '')
        permission='' if predicate is None else ',can_break:{blocks:"minecraft:'+predicate+'"}'
        guard(f'/summon arrow {x-1.5} 200.5 {z+.5} {{UUID:[I;197,0,{i+1},2],NoGravity:1b,HasBeenShot:1b,Motion:[2d,0d,0d],item:{{id:"minecraft:arrow",count:1}}'+owner_tag+permission+'}', label+'-arrow-summon')
        out.append(dict(op='projectile_saved_type',uuid=arrow,label=label+'-actual-arrow',admission=True,category='control',expect_observation=dict(uuid=words,kind='minecraft:arrow',pos=[x-1.5,200.5,z+.5],no_gravity=True)))
        guard(f'/execute if data entity {arrow} {{UUID:[I;197,0,{i+1},2],NoGravity:1b,Pos:[{x-1.5}d,200.5d,{z+.5}d],Motion:[2d,0d,0d],item:{{id:"minecraft:arrow",count:1}}}}', label+'-arrow-state')
        if owner_tag:
            guard(f'/execute if data entity {arrow} {{'+owner_tag[1:]+'}', label+'-saved-owner')
        else:
            guard(f'/execute unless data entity {arrow} Owner', label+'-absent-owner')
        if predicate:
            guard(f'/execute if data entity {arrow} {{can_break:{{blocks:"minecraft:{predicate}"}}}}', label+'-saved-predicate')
        else:
            guard(f'/execute unless data entity {arrow} can_break', label+'-absent-predicate')
        add(f'/data get entity {arrow}', label+'-initial-body', category='raw')
        add('/execute store result score before h197 run time query gametime', label+'-clock-start', category='raw')
        add('/tick step 1', label+'-step', admission=True)
        out.append(dict(op='tick',n=1,record=True,label=label+'-actual-tick',admission=True,category='control'))
        add('/execute store result score elapsed h197 run time query gametime', label+'-clock-end', category='raw')
        guard('/scoreboard players operation elapsed h197 -= before h197', label+'-clock-subtract', 1)
        guard('/scoreboard players get elapsed h197', label+'-elapsed', 1)
        guard('/execute if loaded 20 200 14', label+'-owner-loaded-after')
        guard(f'/execute if entity {pig}', label+'-owner-live-after')
        observed(f'execute if block {x} 200 {z} '+('air' if breaks else 'chorus_flower[age=0]'), label+'-block', 'permission', 1)
        observed('execute '+('if' if breaks else 'unless')+' entity @e[type=item,'+region+']', label+'-unfiltered-drop', 'drop', 1)
        if breaks:
            observed('execute if entity @e[type=item,'+region+',nbt={Item:{id:"minecraft:chorus_flower",count:1}}]', label+'-typed-drop', 'drop', 1)
        add('/execute as @e[type=item,'+region+'] run data get entity @s', label+'-full-drops', category='raw-drop')
        add(f'/data get entity {arrow}', label+'-final-body', category='raw')
    for n,block in enumerate(['chorus_flower','dirt']):
        x,z=27,3+5*n
        if block=='chorus_flower':
            guard(f'/setblock {x} 199 {z} end_stone','direct-flower-support')
        guard(f'/setblock {x} 200 {z} {block}', 'direct-'+block+'-set')
        guard(f'/execute if block {x} 200 {z} {block}', 'direct-'+block+'-initial')
        guard(f'/setblock {x} 200 {z} air destroy','direct-'+block+'-destroy')
        guard(f'/execute if block {x} 200 {z} air','direct-'+block+'-removed')
        add(f'/execute if entity @e[type=item,x={x-1},y=199,z={z-1},dx=3,dy=3,dz=3,nbt={{Item:{{id:"minecraft:{block}",count:1}}}}]', 'direct-'+block+'-drop', category='drop-control')
        add(f'/execute as @e[type=item,x={x-1},y=199,z={z-1},dx=3,dy=3,dz=3] run data get entity @s','direct-'+block+'-full-drop',category='raw-drop')
    return out
