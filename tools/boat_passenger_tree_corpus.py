"""Boat command passenger trees; live relation admissions and raw diagnostics."""
def cases(cmd, original_full=False):
    out=[]
    def add(s,n=None,guard=False,diag=None,**kw):
        o=cmd(s) if n is None else cmd(s,expect_result=n)
        if guard:o['h151_admission']=True
        if diag:o['diagnostic']=diag
        o.update(kw);out.append(o)
    U=lambda n:f'[I;{n},151,1,1]'
    S=lambda n:f'@e[nbt={{UUID:{U(n)}}},limit=1]'
    def clock(n):
        add('/execute store result score #now h151 run time query gametime',diag='absolute epoch')
        add('/scoreboard players operation #now h151 -= #start h151')
        add('/scoreboard players get #now h151',n,True)
    def actor(n,kind,health=None):
        add(f'/execute if entity @e[type=minecraft:{kind},nbt={{UUID:{U(n)}}}]',1,True)
        add(f'/data get entity {S(n)} UUID',4,True)
        add(f'/data get entity {S(n)} Pos',diag='actual typed actor position, not physics parity')
        if health is not None:add(f'/execute if entity @e[nbt={{UUID:{U(n)},Health:{health}f,NoAI:1b,NoGravity:1b}}]',1,True)
    def tree(parent,child,name,positive=True,control=False):
        def q(s,n=None,**kw):add(s,n,guard=control,**kw)
        add('/scoreboard players set #r h151 91',91,True);add('/scoreboard players set #s h151 17',17,True)
        query=(f'data get entity {S(parent)} Passengers' if original_full or not positive
               else f'execute if data entity {S(parent)} Passengers[0]')
        text=f'/execute store result score #r h151 store success score #s h151 run {query}'
        if positive:q(text,1)
        else:q(text,expect_callback=[False,0])
        q('/scoreboard players get #r h151',int(positive));q('/scoreboard players get #s h151',int(positive))
        if positive:
            if not original_full:q(f'/execute unless data entity {S(parent)} Passengers[1]',1)
            q(f'/data get entity {S(parent)} Passengers[0].UUID',4)
            q(f'/execute if data entity {S(parent)} {{Passengers:[{{id:"minecraft:{"pig" if parent==4 else "cow"}",UUID:{U(child)},Health:10f,NoAI:1b,NoGravity:1b,CustomName:"{name}"}}]}}',1)
            q(f'/data get entity {S(parent)} Passengers[0].CustomName',len(name))
            add(f'/data get entity {S(parent)} Passengers[0]',diag='complete typed child body; defaults/order/seat diagnostics')
        else:q(f'/execute unless data entity {S(parent)} Passengers',1)
        add(f'/data get entity {S(parent)}',diag='complete parent body and passenger tree')
    add('/tick freeze',guard=True);add('/gamemode spectator',guard=True);add('/gamerule spawn_mobs false',0,True);add('/gamerule spawn_mobs',0,True);add('/scoreboard objectives add h151 dummy',1,True)
    add('/forceload add 0 0 31 31',4,True);out.append({'op':'settle'})
    for x,z in [(0,0),(16,0),(0,16),(16,16)]:add(f'/execute if loaded {x} 200 {z}',1,True)
    add('/fill 0 199 0 31 204 31 barrier',6144,True);add('/fill 0 200 0 31 204 31 air',5120,True);add('/fill 0 199 0 31 199 31 stone',1024,True)
    add('/tp @s 16.5 203 16.5',1,True);add('/execute if entity @s[nbt={Health:20f,playerGameType:3,Pos:[16.5d,203d,16.5d]}]',1,True);add('/data get entity @s UUID',4,True)
    flags='NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:10f'
    add(f'/summon happy_ghast 11.5 201 8.5 {{UUID:{U(9)},NoAI:1b,NoGravity:1b,Silent:1b,PersistenceRequired:1b,Health:20f}}',1,True)
    add(f'/summon oak_boat 8.5 200 8.5 {{UUID:{U(1)},NoGravity:1b,Silent:1b,CustomName:"Hull",Tags:["h151_boat","keep"],leash:{{UUID:{U(9)}}},Passengers:[{{id:"minecraft:cow",UUID:{U(2)},{flags},CustomName:"Rider",Tags:["h151_child"]}}]}}',1,True)
    add(f'/summon oak_boat 20.5 200 8.5 {{UUID:{U(3)},NoGravity:1b,Silent:1b,CustomName:"Empty"}}',1,True)
    add(f'/summon cow 8.5 200 20.5 {{UUID:{U(4)},{flags},CustomName:"MobHull",Passengers:[{{id:"minecraft:pig",UUID:{U(5)},{flags},CustomName:"MobRider"}}]}}',1,True)
    add('/execute store result score #start h151 run time query gametime',diag='absolute epoch');clock(0)
    add('/tick step 10',1,True);out.append({'op':'tick','n':10,'record':True,'diagnostic':'complete setup tick receipt'})
    clock(10)
    for n,k,h in [(1,'oak_boat',None),(2,'cow',10),(3,'oak_boat',None),(4,'cow',10),(5,'pig',10),(9,'happy_ghast',20)]:actor(n,k,h)
    for x,z in [(8,8),(20,8),(8,20),(11,8)]:add(f'/execute if block {x} 199 {z} stone',1,True)
    add(f'/execute as {S(2)} on vehicle if entity @s[nbt={{UUID:{U(1)}}}]',1,True)
    for n,x,y,z in [(1,8.5,200,8.5),(3,20.5,200,8.5),(4,8.5,200,20.5),(9,11.5,201,8.5)]:add(f'/execute positioned {x} {y} {z} if entity @e[nbt={{UUID:{U(n)}}},distance=..1]',1,True)
    add(f'/execute as {S(5)} on vehicle if entity @s[nbt={{UUID:{U(4)}}}]',1,True)
    add(f'/execute positioned 8.5 200 8.5 if entity @e[type=cow,nbt={{UUID:{U(2)}}},distance=..3]',1,True)
    add(f'/execute as {S(1)} on leasher if entity @s[nbt={{UUID:{U(9)}}}]',1,True)
    def unchanged():
        add(f'/execute if data entity {S(1)} {{UUID:{U(1)},CustomName:"Hull",Silent:1b,NoGravity:1b,Tags:["h151_boat","keep"],leash:{{UUID:{U(9)}}}}}',1,True)
        add(f'/data get entity {S(1)} leash.UUID',4,True)
        add(f'/execute if entity {S(2)}',1,True)
    unchanged();tree(3,0,'',False,True);tree(4,5,'MobRider',True,True)
    tree(1,2,'Rider');tree(1,2,'Rider')
    add(f'/ride {S(2)} dismount',1,True)
    add('/scoreboard players set #vehicle h151 0',0,True)
    add(f'/execute as {S(2)} on vehicle run scoreboard players set #vehicle h151 1',diag='complete empty vehicle relation callback')
    add('/scoreboard players get #vehicle h151',0,True)
    tree(1,2,'Rider',False,True);unchanged()
    add(f'/ride {S(2)} mount {S(1)}',1,True)
    add(f'/execute as {S(2)} on vehicle if entity @s[nbt={{UUID:{U(1)}}}]',1,True)
    tree(1,2,'Rider');unchanged();clock(10)
    add(f'/data get entity {S(2)}',diag='complete live child after remount')
    add('/forceload remove 0 0 31 31',4);add('/scoreboard objectives remove h151');add('/tick unfreeze')
    return out
