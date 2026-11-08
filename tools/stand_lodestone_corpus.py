"""Loaded lodestone removal and actual holder inventory ticks; all raws retained."""
import uuid

CASES = [('tracked', 'minecraft:overworld', True, True),
         ('untracked', 'minecraft:overworld', False, True),
         ('other_dimension', 'minecraft:the_nether', True, True),
         ('targetless', 'minecraft:overworld', True, False)]

def program():
    ops = []
    def cmd(text, label, result=None, admission=False, category='control', results=None):
        row = dict(op='cmd', cmd=text, label=label, admission=admission, category=category)
        if result is not None: row['expect_result'] = result
        if results is not None: row['expect_results'] = results
        ops.append(row)
    def guard(text, label, result=1): cmd(text,label,result,True)
    cmd('/tick freeze','freeze',admission=True)
    for k,v in [('spawn_mobs','false'),('advance_time','false'),('random_tick_speed','0')]:
        cmd(f'/gamerule {k} {v}',k,admission=True)
    guard('/forceload add 0 0','load')
    ops.append(dict(op='settle',required_chunks=[[0,0]]))
    guard('/fill 0 199 0 15 199 15 stone','floor',256)
    guard('/setblock 12 200 12 lodestone','target-create')
    guard('/execute if loaded 12 200 12','target-loaded')
    guard('/execute if block 12 200 12 lodestone','target-positive')
    cmd('/gamemode creative @s','player-mode',admission=True)
    guard('/tp @s 14.5 200.0 14.5 0 0','player-position')
    guard('/execute if entity @s[type=player,name=Player,nbt={Health:20f,playerGameType:1,Pos:[14.5d,200d,14.5d]}]','player-admit')
    cmd('/data get entity @s UUID','player-uuid',4,True)
    guard('/scoreboard objectives add lodetick dummy','objective')
    cmd('/execute store result score start lodetick run time query gametime','clock-start',category='raw')
    holders=[]
    for n,(name,dim,tracked,target) in enumerate(CASES,1):
        who=str(uuid.UUID(int=(0xA551<<96)|n));x=2*n+.5
        guard(f'/summon armor_stand {x} 200.0 4.5 {{UUID:[I;42321,0,0,{n}],Health:20f,NoGravity:1b}}',name+'-summon')
        guard(f'/execute as {who} if entity @s[type=armor_stand,nbt={{UUID:[I;42321,0,0,{n}],Health:20f,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]',name+'-holder')
        guard(f'/execute if loaded {int(x)} 200 4',name+'-loaded')
        guard(f'/execute if block {int(x)} 199 4 stone',name+'-floor')
        value=(f'target:{{dimension:"{dim}",pos:[I;12,200,12]}},' if target else '')+'tracked:'+str(tracked).lower()
        component=f'minecraft:compass[minecraft:lodestone_tracker={{{value}}},minecraft:custom_data={{marker:{n}}}]'
        for hand in ['mainhand','offhand']:
            guard(f'/item replace entity {who} weapon.{hand} with {component} 1',name+'-'+hand+'-equip')
            holders.append((name+'-'+hand,who,f'equipment.{hand}',target,tracked,n))
    component='minecraft:compass[minecraft:lodestone_tracker={target:{dimension:"minecraft:overworld",pos:[I;12,200,12]},tracked:true},minecraft:custom_data={marker:9}]'
    for hand,slot in [('mainhand',0),('offhand',-106)]:
        guard(f'/item replace entity @s weapon.{hand} with {component} 1','player-'+hand+'-equip')
        holders.append(('player-'+hand,'@s',(f'Inventory[{{Slot:{slot}b}}]' if hand=='mainhand' else 'equipment.offhand'),True,True,9))
    for phase in ['before','after']:
        if phase=='after':
            guard('/setblock 12 200 12 air','target-remove')
            guard('/execute if block 12 200 12 air','target-gone')
            guard('/execute if loaded 12 200 12','target-still-loaded')
            cmd('/tick step 1','step-request',admission=True)
            ops.append(dict(op='player_loop',n=1,label='actual-one-tick',expect_result=1,admission=True,category='control'))
        cmd('/execute store result score now lodetick run time query gametime',phase+'-raw-clock',category='raw')
        guard('/scoreboard players operation now lodetick -= start lodetick',phase+'-clock-op',int(phase=='after'))
        guard('/scoreboard players get now lodetick',phase+'-clock',int(phase=='after'))
        for name,who,path,target,tracked,marker in holders:
            label=phase+'-'+name
            for field,result in [('id',17),('count',1),('components."minecraft:custom_data".marker',marker)]:
                cmd(f'/data get entity {who} {path}.{field}',label+'-'+field,result,True)
            tracker=path+'.components."minecraft:lodestone_tracker"'
            present=target and not(phase=='after' and (name.startswith('tracked-') or name.startswith('player-')))
            for score in ['result','success']:guard(f'/scoreboard players set {score} lodetick -99',label+'-reset-'+score,-99)
            cmd(f'/execute store result score result lodetick store success score success lodetick run data get entity {who} {tracker}.target',label+'-target',admission=(phase=='before' or not name.startswith('tracked-')),category='tracker',results=[[True,2]] if present else [[False,0]])
            cmd('/scoreboard players get result lodetick',label+'-result',2 if present else 0,admission=(phase=='before' or not name.startswith('tracked-')),category='tracker')
            cmd('/scoreboard players get success lodetick',label+'-success',int(present),admission=(phase=='before' or not name.startswith('tracked-')),category='tracker')
            cmd(f'/data get entity {who} {tracker}.tracked',label+'-saved-tracked',category='canonical')
            cmd(f'/data get entity {who} {path}',label+'-full-stack',category='raw')
        for n,(name,_,_,_) in enumerate(CASES,1):
            who=str(uuid.UUID(int=(0xA551<<96)|n));x=2*n+.5
            guard(f'/execute as {who} if entity @s[type=armor_stand,nbt={{UUID:[I;42321,0,0,{n}],Health:20f,NoGravity:1b,Pos:[{x}d,200d,4.5d]}}]',phase+'-'+name+'-holder')
            cmd(f'/data get entity {who}',phase+'-'+name+'-body',category='raw')
    return ops
