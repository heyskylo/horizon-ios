"""Modeled jukebox saved-clock presence; explicit unsupported-input diagnostics."""

def cases(cmd):
    out=[]
    def add(s,result=None,admission=False,diagnostic=None,**extra):
        op=cmd(s) if result is None else cmd(s,expect_result=result)
        if admission:op['h032_admission']=True
        if diagnostic:op['diagnostic']=diagnostic
        op.update(extra);out.append(op)
    def clock(n):
        add('/execute store result score #now h032 run time query gametime',diagnostic='absolute scene epoch')
        add('/scoreboard players operation #now h032 -= #start h032')
        add('/scoreboard players get #now h032',n,True)
    def record(x,z,item='music_disc_cat',diag=None):
        pos=f'{x} 200 {z}'
        add(f'/execute if data block {pos} {{RecordItem:{{id:"minecraft:{item}",count:1}}}}',1,True)
        add(f'/data get block {pos} RecordItem.count',1,True)
        add(f'/execute if items block {pos} container.0 minecraft:{item}',1,True)
        if item=='music_disc_cat':add(f'/data get block {pos} RecordItem.components."minecraft:custom_data".h032',1,True)
        add(f'/data get block {pos} RecordItem',diagnostic=diag)
    def counter(x,z,value,diag=None):
        pos=f'{x} 200 {z}'
        add('/scoreboard players set #r h032 91',91,True)
        add('/scoreboard players set #s h032 17',17,True)
        query=f'/execute store result score #r h032 store success score #s h032 run data get block {pos} ticks_since_song_started'
        if diag:add(query,diagnostic=diag)
        elif value is None:add(query,expect_callback=[False,0])
        else:add(query,value)
        add('/scoreboard players get #r h032',None if diag else (value or 0),diagnostic=diag)
        add('/scoreboard players get #s h032',None if diag else int(value is not None),diagnostic=diag)
        if not diag:
            if value is None:add(f'/execute unless data block {pos} ticks_since_song_started',1)
            else:add(f'/execute if data block {pos} {{ticks_since_song_started:{value}L}}',1,True)
    def block(x,z,has):
        pos=f'{x} 200 {z}'
        add(f'/execute if loaded {pos}',1,True)
        add(f'/execute if block {x} 199 {z} stone',1,True)
        add(f'/execute if block {pos} jukebox[has_record={str(has).lower()}]',1,True)
        add(f'/execute if data block {pos} {{id:"minecraft:jukebox",x:{x},y:200,z:{z}}}',1,True)
    add('/tick freeze',admission=True)
    add('/gamemode spectator',admission=True)
    add('/scoreboard objectives add h032 dummy',1,True)
    add('/forceload add 0 0 15 15',1,True)
    out.append({'op':'settle'})
    add('/execute if loaded 11 200 11',1,True)
    add('/fill 0 199 0 15 204 15 barrier',1536,True)
    add('/fill 0 200 0 15 204 15 air',1280,True)
    add('/fill 0 199 0 15 199 15 stone',256,True)
    add('/tp @s 8.5 203 8.5',1,True)
    add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,203d,8.5d],playerGameType:3}]',1,True)
    add('/data get entity @s UUID',4,True)
    add('/execute store result score #start h032 run time query gametime',diagnostic='absolute scene epoch')
    rec='{id:"minecraft:music_disc_cat",count:1,components:{"minecraft:custom_data":{h032:1}}}'
    add('/setblock 3 200 3 jukebox',1,True)
    add('/setblock 7 200 3 jukebox',1,True)
    add('/setblock 11 200 3 jukebox[has_record=true]{RecordItem:'+rec+'}',1,True)
    add('/item replace block 7 200 3 container.0 with music_disc_cat[minecraft:custom_data={h032:1}]',1,True)
    for x,has in [(3,False),(7,True),(11,True)]:block(x,3,has)
    add('/execute unless data block 3 200 3 RecordItem',1,True)
    record(7,3);record(11,3)
    counter(3,3,None);counter(7,3,0);counter(11,3,None)
    for x in [3,7,11]:add(f'/data get block {x} 200 3',diagnostic='full primary block body, including assigned counter field')
    clock(0)
    add('/tick step 1',1,True)
    out.append({'op':'tick','n':1,'record':True,'diagnostic':'complete actual tick receipt; no song event/sound claim'})
    clock(1);counter(7,3,1)
    add('/tick step 1',1,True)
    out.append({'op':'tick','n':1,'record':True,'diagnostic':'complete actual tick receipt; no song event/sound claim'})
    clock(2)
    for x,has,value in [(3,False,None),(7,True,2),(11,True,None)]:
        block(x,3,has);counter(x,3,value)
        if has:record(x,3)
        else:add('/execute unless data block 3 200 3 RecordItem',1,True)
        add(f'/data get block {x} 200 3',diagnostic='full primary block body after actual two ticks')
    # No validity/lifecycle parity claim for these inputs. Admit the actual
    # loaded block and item before retaining complete diagnostic output.
    for x,label,item,raw in [(3,'invalid string clock','music_disc_cat','"bad"'),(7,'unplayable record','stone','41L'),(11,'expired loaded clock','music_disc_cat','100000L')]:
        itemtag=rec if item=='music_disc_cat' else '{id:"minecraft:stone",count:1}'
        add(f'/setblock {x} 200 11 jukebox[has_record=true]{{RecordItem:{itemtag},ticks_since_song_started:{raw}}}',1,True)
        block(x,11,True);record(x,11,item,label)
        counter(x,11,None,label)
        add(f'/data get block {x} 200 11',diagnostic=label+'; unsupported loaded-song/catalog semantics')
    clock(2)
    add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,203d,8.5d],playerGameType:3}]',1,True)
    add('/forceload remove 0 0 15 15',1)
    add('/scoreboard objectives remove h032')
    add('/tick unfreeze')
    return out
