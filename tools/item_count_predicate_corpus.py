"""Actual chest count pseudo-component checks; plain Int bounds only."""

def cases(cmd):
    out = []
    def add(command, result=None, admission=False, diagnostic=None, **extra):
        op = cmd(command) if result is None else cmd(command, expect_result=result)
        if admission: op['h194_admission'] = True
        if diagnostic: op['diagnostic'] = diagnostic
        op.update(extra)
        out.append(op)
    def clock():
        add('/execute store result score #now h194 run time query gametime', diagnostic='absolute scene epoch')
        add('/scoreboard players operation #now h194 -= #start h194')
        add('/scoreboard players get #now h194', 0, True)
    add('/tick freeze', admission=True)
    add('/gamemode spectator', admission=True)
    add('/scoreboard objectives add h194 dummy', 1, True)
    add('/forceload add 0 0 15 15', 1, True)
    out.append({'op':'settle'})
    add('/execute if loaded 4 200 4', 1, True)
    add('/fill 0 199 0 15 203 15 barrier', 1280, True)
    add('/fill 0 200 0 15 203 15 air', 1024, True)
    add('/fill 0 199 0 15 199 15 stone', 256, True)
    add('/tp @s 8.5 202 8.5', 1, True)
    add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,202d,8.5d],playerGameType:3}]', 1, True)
    add('/data get entity @s UUID', 4, True)
    add('/execute store result score #start h194 run time query gametime', diagnostic='absolute scene epoch')
    items = ['{Slot:%db,id:"minecraft:diamond",count:%d}' % (slot,count) for slot,count in enumerate([1,3,4])]
    items += ['{Slot:3b,id:"minecraft:dirt",count:3}', '{Slot:5b,id:"minecraft:stone",count:2,components:{"minecraft:custom_data":{h194:1}}}']
    add('/setblock 4 200 4 chest{Items:['+','.join(items)+']}', 1, True)
    add('/execute if block 4 199 4 stone', 1, True)
    add('/execute if block 4 200 4 chest', 1, True)
    add('/execute if data block 4 200 4 {id:"minecraft:chest",x:4,y:200,z:4}', 1, True)
    for slot,count in enumerate([1,3,4]):
        add(f'/data get block 4 200 4 Items[{{Slot:{slot}b}}].count', count, True)
        add(f'/execute if data block 4 200 4 Items[{{Slot:{slot}b,id:"minecraft:diamond",count:{count}}}]', 1, True)
        add(f'/execute if items block 4 200 4 container.{slot} diamond', count, True)
    add('/execute unless data block 4 200 4 Items[{Slot:4b}]', 1, True)
    add('/data get block 4 200 4 Items[{Slot:3b}].count', 3, True)
    add('/execute if items block 4 200 4 container.3 dirt', 3, True)
    add('/data get block 4 200 4 Items[{Slot:5b}].count', 2, True)
    add('/execute if items block 4 200 4 container.5 stone', 2, True)
    add('/data get block 4 200 4 Items[{Slot:5b}].components."minecraft:custom_data".h194', 1, True)
    add('/data get block 4 200 4 Items', diagnostic='whole saved inventory/order', h194_snapshot='before')
    clock()
    def test(slot, predicate, accepted, sign='if', count=0):
        value = (count if sign == 'if' else 1) if accepted else 0
        add('/scoreboard players set #r h194 91', 91, True)
        add('/scoreboard players set #s h194 17', 17, True)
        command = f'/execute store result score #r h194 store success score #s h194 {sign} items block 4 200 4 container.{slot} {predicate}'
        if accepted: add(command, value)
        else: add(command, expect_callback=[False,0])
        add('/scoreboard players get #r h194', value)
        add('/scoreboard players get #s h194', 1 if accepted else 0)
    for slot,count in enumerate([1,3,4]):
        for suffix,accepted in [('count=3',count==3),('count=4',count==4),('!count=3',count!=3),('count={min:2}',count>=2),('count~3',count==3),('minecraft:count={min:2,max:3}',2<=count<=3)]:
            test(slot,'diamond['+suffix+']',accepted,count=count)
        test(slot,'diamond[count=3]',count!=3,'unless',count=count)
    test(3,'diamond[count=3]',False)
    test(4,'diamond[count=3]',False)
    test(5,'stone[custom_data={h194:1}]',True,count=2)
    test(5,'stone[custom_data={h194:2}]',False)
    for slot,count in enumerate([1,3,4]):
        add(f'/data get block 4 200 4 Items[{{Slot:{slot}b}}].count', count, True)
        add(f'/execute if items block 4 200 4 container.{slot} diamond', count, True)
    add('/data get block 4 200 4 Items', diagnostic='whole saved inventory/order', h194_snapshot='after')
    add('/data get block 4 200 4', diagnostic='full source saved block entity/order')
    clock()
    add('/execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,202d,8.5d],playerGameType:3}]', 1, True)
    add('/forceload remove 0 0 15 15', 1)
    add('/scoreboard objectives remove h194')
    add('/tick unfreeze')
    return out
