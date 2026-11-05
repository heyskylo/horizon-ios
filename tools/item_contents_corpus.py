"""Live /item component contents: exact inventories, callbacks and zero ticks."""
try:
    from .command_exec_corpus import cmd, setup
except ImportError:
    from command_exec_corpus import cmd, setup


def item_contents(provider_empty_control=False):
    out = setup('/tick freeze', '/gamerule minecraft:spawn_mobs false',
                '/gamerule minecraft:advance_time false', '/gamemode spectator',
                '/tp @s 24.5 110 24.5 0 0')
    out += [cmd('/forceload add 0 0 31 31',expect_result=4),{'op':'settle'}]
    for x in [0,16]:
        for z in [0,16]:
            out.append(cmd(f'/execute if loaded {x} 100 {z}',expect_result=1))
    out += [cmd('/fill 0 100 0 31 102 31 stone',expect_result=3072),
            cmd('/fill 0 100 0 31 102 31 air',expect_result=3072),
            cmd('/fill 0 99 0 31 99 31 stone',expect_result=1024),
            cmd('/setblock 0 100 0 chest{Items:['
                '{Slot:0b,id:"minecraft:shulker_box",count:1,components:{"minecraft:container":['
                '{slot:0,item:{id:"minecraft:stone",count:3}},'
                '{slot:5,item:{id:"minecraft:dirt",count:1}}]}},'
                '{Slot:1b,id:"minecraft:bundle",count:1,components:{"minecraft:bundle_contents":['
                '{id:"minecraft:stick",count:4},{id:"minecraft:apple",count:2}]}},'
                '{Slot:2b,id:"minecraft:crossbow",count:1,components:{"minecraft:charged_projectiles":['
                '{id:"minecraft:arrow",count:1}]}},'
                '{Slot:3b,id:"minecraft:stone",count:1}]}',expect_result=1),
            cmd('/setblock 2 100 0 chest',expect_result=1)]
    for x in [0,2]:
        out += [cmd(f'/execute if block {x} 100 0 chest',expect_result=1),
                cmd(f'/execute if block {x} 99 0 stone',expect_result=1)]
        for field,value in [('id',None),('x',x),('y',100),('z',0)]:
            out.append(cmd(f'/data get block {x} 100 0 {field}',expect_result=value))
    out += setup('/scoreboard objectives add contents_audit dummy',
                 '/execute store result score start contents_audit run time query gametime')

    def state():
        out.extend([cmd('/data get block 0 100 0 Items'),cmd('/data get block 2 100 0 Items')])
        # Outer item identities/counts must survive component writes.
        for slot,item in [(0,'shulker_box'),(1,'bundle'),(2,'crossbow'),(3,'stone')]:
            out.append(cmd(f'/execute if data block 0 100 0 Items[{{Slot:{slot}b,id:"minecraft:{item}",count:1}}]',expect_result=1))
    def source(component,slot):
        return '{type:"contents",component:"'+component+'",slot_source:{type:"slot_range",slots:"container.'+str(slot)+'"}}'
    container=source('container',0)
    bundle=source('bundle_contents',1)
    charged=source('charged_projectiles',2)
    state()
    operations=[
        ('/item modify block 0 100 0 '+container+' {type:"minecraft:set_count",count:7}',2),
        ('/item replace block 0 100 0 '+container+' with minecraft:diamond',1),
        ('/item fill block 0 100 0 '+bundle+' with minecraft:gold_ingot',64),
        ('/item override block 0 100 0 '+charged+' with minecraft:spectral_arrow 2',1),
        ('/item replace block 0 100 0 '+source('container',3)+' with minecraft:emerald',1),
        ('/item replace block 2 100 0 container.0 from block 0 100 0 '+container,1),
        ('/item modify block 0 100 0 '+source('damage',0)+' {type:"minecraft:set_count",count:7}',None),
        # Independently measured composition and repeated access controls.
        ('/item modify block 0 100 0 ['+container+','+container+'] {type:"minecraft:set_count",count:9}',4),
        ('/item fill block 0 100 0 {type:"limit_slots",limit:1,slot_source:'+container+'} with minecraft:gold_ingot',1),
        ('/item replace block 2 100 0 container.* from block 0 100 0 '+container,6),
    ]
    if provider_empty_control:
        operations += [
            ('/item replace block 0 100 0 container.2 with minecraft:crossbow',1),
            ('/item replace block 0 100 0 container.4 with minecraft:dirt',1),
            ('/item replace block 0 100 0 ['+charged+', {type:"slot_range",slots:"container.4"}] from block 2 100 0 container.1',1),
        ]
    for command,expected in operations:
        out += setup('/data modify storage h308:callbacks state set value {}')
        out.append(cmd('/data get storage h308:callbacks state'))
        for key in ['result','success']:
            op=cmd(f'/execute if data storage h308:callbacks state.{key}')
            op['expect_callback']=[False,0];out.append(op)
        out.append(cmd('/execute store result storage h308:callbacks state.result int 1 '
                       'store success storage h308:callbacks state.success int 1 run '+command[1:]))
        for key in ['result','success']:
            out.append(cmd(f'/data get storage h308:callbacks state.{key}',
                           expect_result=(expected if key=='result' else 1) if expected is not None else None))
        if expected is None:
            for key in ['result','success']:
                op=cmd(f'/execute if data storage h308:callbacks state.{key}')
                op['expect_callback']=[False,0];out.append(op)
        state()
    if provider_empty_control:
        out.append(cmd('/execute unless data block 0 100 0 Items[{Slot:4b}]',expect_result=1))
    out += setup('/execute store result score elapsed contents_audit run time query gametime',
                 '/scoreboard players operation elapsed contents_audit -= start contents_audit')
    out.append(cmd('/scoreboard players get elapsed contents_audit',expect_result=0))
    return out
