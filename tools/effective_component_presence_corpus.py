"""Effective component presence on actual prototype/patch stacks."""
def cases(cmd):
    out=[]
    def add(s,n=None,guard=False,diag=None,**kw):
        o=cmd(s) if n is None else cmd(s,expect_result=n)
        if guard:o['h193_admission']=True
        if diag:o['diagnostic']=diag
        o.update(kw);out.append(o)
    def clock():
        add('/execute store result score #now h193 run time query gametime',diag='absolute epoch')
        add('/scoreboard players operation #now h193 -= #start h193')
        add('/scoreboard players get #now h193',0,True)
    add('/tick freeze',guard=True);add('/gamemode spectator',guard=True)
    add('/scoreboard objectives add h193 dummy',1,True);add('/forceload add 0 0 15 15',1,True);out.append({'op':'settle'})
    add('/execute if loaded 4 200 4',1,True)
    add('/fill 0 199 0 15 203 15 barrier',1280,True);add('/fill 0 200 0 15 203 15 air',1024,True);add('/fill 0 199 0 15 199 15 stone',256,True)
    add('/tp @s 8.5 202 8.5',1,True);add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,202d,8.5d],playerGameType:3}]',1,True);add('/data get entity @s UUID',4,True)
    add('/execute store result score #start h193 run time query gametime',diag='absolute epoch')
    stacks=['{Slot:0b,id:"minecraft:music_disc_cat",count:1}','{Slot:1b,id:"minecraft:stone",count:1}','{Slot:2b,id:"minecraft:stone",count:1,components:{"minecraft:jukebox_playable":"minecraft:cat","minecraft:custom_data":{h193:1}}}','{Slot:3b,id:"minecraft:music_disc_cat",count:1,components:{"!minecraft:jukebox_playable":{},"minecraft:custom_data":{h193:1}}}']
    add('/setblock 4 200 4 chest{Items:['+','.join(stacks)+']}',1,True)
    add('/execute if block 4 199 4 stone',1,True);add('/execute if block 4 200 4 chest',1,True);add('/execute if data block 4 200 4 {id:"minecraft:chest",x:4,y:200,z:4}',1,True)
    for slot,item in enumerate(['music_disc_cat','stone','stone','music_disc_cat']):
        add(f'/execute if data block 4 200 4 Items[{{Slot:{slot}b,id:"minecraft:{item}",count:1}}]',1,True)
        add(f'/data get block 4 200 4 Items[{{Slot:{slot}b}}].count',1,True)
        add(f'/execute if items block 4 200 4 container.{slot} {item}',1,True)
    add('/execute unless data block 4 200 4 Items[{Slot:0b}].components',1,True)
    add('/execute unless data block 4 200 4 Items[{Slot:1b}].components',1,True)
    add('/execute if data block 4 200 4 Items[{Slot:2b}].components."minecraft:jukebox_playable"',1,True)
    add('/execute if data block 4 200 4 Items[{Slot:3b}].components."!minecraft:jukebox_playable"',1,True)
    for slot in [2,3]:add(f'/data get block 4 200 4 Items[{{Slot:{slot}b}}].components."minecraft:custom_data".h193',1,True)
    add('/data get block 4 200 4 Items',diag='whole saved inventory',h193_snapshot='before');clock()
    def test(slot,neg,present,sign='if',diag=None):
        accepted=(not present if neg else present);accepted=accepted if sign=='if' else not accepted
        for name,v in [('r',91),('s',17),('flow',99)]:add(f'/scoreboard players set #{name} h193 {v}',v,True)
        pred='*['+('!' if neg else '')+'minecraft:jukebox_playable]'
        s=f'/execute store result score #r h193 store success score #s h193 {sign} items block 4 200 4 container.{slot} {pred} run scoreboard players set #flow h193 7'
        diag=diag or 'complete action-flow callback/store diagnostic'
        if accepted:add(s,7,diag=diag)
        else:add(s,diag=diag,expect_results=[])
        for name,v in [('r',7 if accepted else 91),('s',1 if accepted else 17),('flow',7 if accepted else 99)]:add(f'/scoreboard players get #{name} h193',v,diag=diag)
    def terminal(slot,neg,present,sign):
        yes=(present != neg) if sign=='if' else (present == neg)
        add('/scoreboard players set #r h193 91',91,True)
        add('/scoreboard players set #s h193 17',17,True)
        pred='*['+('!' if neg else '')+'minecraft:jukebox_playable]'
        command=f'/execute store result score #r h193 store success score #s h193 {sign} items block 4 200 4 container.{slot} {pred}'
        if yes:add(command,1)
        else:add(command,expect_callback=[False,0])
        add('/scoreboard players get #r h193',int(yes))
        add('/scoreboard players get #s h193',int(yes))
    for slot,p in enumerate([True,False,True,False]):
        for neg in [False,True]:
            for sign in ['if','unless']:terminal(slot,neg,p,sign)
    for slot,p in enumerate([True,False,True,False]):
        for neg in [False,True]:
            for sign in ['if','unless']:test(slot,neg,p,sign)
    add('/execute if items block 4 200 4 container.2 stone[custom_data={h193:1}]',1,True)
    add('/execute if items block 4 200 4 container.3 music_disc_cat[custom_data={h193:1}]',1,True)
    add('/execute unless items block 4 200 4 container.4 *',1,True)
    add('/data get block 4 200 4 Items',diag='whole saved inventory',h193_snapshot='after');clock()
    # Independent command-materialization bridge: Java expectations remain explicit.
    add('/item replace block 4 200 4 container.5 with music_disc_cat[!minecraft:jukebox_playable]',1,True)
    add('/data get block 4 200 4 Items[{Slot:5b}]',diag='item-command removal bridge')
    add('/execute if data block 4 200 4 Items[{Slot:5b}].components."!minecraft:jukebox_playable"',1,diag='item-command removal bridge')
    test(5,False,False,diag='item-command removal bridge');test(5,True,False,diag='item-command removal bridge')
    add('/data get block 4 200 4',diag='full source body and bridge');clock()
    add('/forceload remove 0 0 15 15',1);add('/scoreboard objectives remove h193');add('/tick unfreeze')
    return out
