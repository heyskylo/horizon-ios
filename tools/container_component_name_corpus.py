"""READ-ONLY H210 draft: existing real command/use-block/break routes only."""
import json

STYLED = '{text:"Styled",color:"aqua",bold:0b,extra:[{text:"Box",italic:1b}]}'
CHANGED = '{text:"Changed",color:"gold",italic:1b}'
TRANSLATED = '{translate:"container.chest",color:"red",italic:0b}'
ITEMS = 'Items:[{Slot:0b,id:"minecraft:diamond",count:3,components:{"minecraft:custom_data":{h210:1}}},{Slot:26b,id:"minecraft:apple",count:5}]'
LOCK = 'lock:{items:"minecraft:diamond"}'
UUID = '[I;-1608632253,-450807400,-1786055591,-2146546415]'

class Program:
    def __init__(self, objective):
        self.ops = []
        self.objective = objective
    def cmd(self, command, *, admission=False, target=False, diagnostic=False, expected=None):
        row = dict(op='cmd', cmd='/' + command.lstrip('/'))
        if admission: row['h210_admission'] = True
        if target: row['h210_target'] = True
        if diagnostic: row['diagnostic'] = True
        if expected is not None: row['expect_callback'] = expected
        self.ops.append(row)
    def guard(self, condition, *, target=False):
        self.cmd('execute ' + condition, admission=not target, target=target, expected=[True, 1])
    def action(self, command, *, target=False):
        self.cmd(f'scoreboard players set #r {self.objective} 91')
        self.cmd(f'scoreboard players set #s {self.objective} 17')
        self.cmd(f'execute store result score #r {self.objective} store success score #s {self.objective} run {command}', target=target)
        self.cmd(f'scoreboard players get #r {self.objective}', target=target)
        self.cmd(f'scoreboard players get #s {self.objective}', target=target)
    def block(self, xyz, block, name=None, locked=False):
        body = ITEMS + (',' + LOCK if locked else '')
        if name is not None: body += ',CustomName:' + name
        self.cmd(f'setblock {xyz} minecraft:{block}{{{body}}}')
        self.be(xyz, block)
        if locked: self.guard(f'if data block {xyz} {{{LOCK}}}')
    def be(self, xyz, kind):
        x,y,z = xyz.split()
        be_id = 'shulker_box' if kind.endswith('shulker_box') else kind
        self.guard(f'if block {xyz} minecraft:{kind}')
        self.guard(f'if data block {xyz} {{id:"minecraft:{be_id}",x:{x},y:{y},z:{z}}}')
        self.guard(f'if data block {xyz} {{{ITEMS}}}')
        self.cmd(f'data get block {xyz}', diagnostic=True)
    def name(self, xyz, component, *, plain=False):
        if component is None:
            self.guard(f'unless data block {xyz} CustomName', target=not plain)
        else:
            self.guard(f'if data block {xyz} {{CustomName:{component}}}', target=not plain)
            self.cmd(f'data get block {xyz} CustomName', target=not plain, diagnostic=not component.startswith('"'))
    def clock(self, beginning=False):
        self.cmd(f'execute store result score #{"start" if beginning else "now"} {self.objective} run time query gametime', diagnostic=True)
        if not beginning:
            self.cmd(f'scoreboard players operation #now {self.objective} -= #start {self.objective}')
            self.guard(f'if score #now {self.objective} matches 0')
            self.cmd(f'scoreboard players get #now {self.objective}', admission=True)
        self.cmd('time query gametime', diagnostic=True)
    def setup(self, restart=False):
        self.cmd('tick freeze')
        self.cmd('gamerule spawn_mobs false')
        self.cmd('gamerule show_advancement_messages false')
        self.cmd('forceload add 0 0')
        self.cmd('gamemode survival @s')
        self.cmd('tp @s 8.5 200 2.5')
        self.ops.append(dict(op='settle'))
        self.cmd('scoreboard objectives add '+self.objective+' dummy')
        if not restart:
            self.cmd('fill 0 199 0 15 199 15 stone')
            self.cmd('fill 0 200 0 15 204 15 air')
            self.cmd('setblock 1 200 1 emerald_block')
        self.guard('if loaded 0 200 0')
        self.guard('if block 8 199 2 stone')
        self.guard('if block 1 200 1 emerald_block')
        self.guard('if entity @s[type=minecraft:player,gamemode=survival]')
        self.guard(f'if data entity @s {{UUID:{UUID},Health:20f,Pos:[8.5d,200d,2.5d]}}')
        self.cmd('data get entity @s', diagnostic=True)
        self.clock(beginning=True)

def stage_one():
    p=Program('h210a');p.setup()
    p.block('2 200 4','chest','"Box"')
    p.name('2 200 4','"Box"',plain=True)
    # A real unrelated lock preserves the actual original input as the existing
    # saved source. No open/permission or lock-copy claim is made.
    p.block('5 200 4','barrel',STYLED,locked=True)
    p.name('5 200 4',STYLED)
    for path in ['text','color','bold','extra[0].text','extra[0].italic']:
        p.cmd(f'data get block 5 200 4 CustomName.{path}',target=True)
    p.block('8 200 4','shulker_box',TRANSLATED)
    p.name('8 200 4',TRANSLATED)
    for path in ['translate','color','italic']:
        p.cmd(f'data get block 8 200 4 CustomName.{path}',target=True)
    p.block('11 200 4','chest')
    p.name('11 200 4',None,plain=True)
    p.action(f'data merge block 11 200 4 {{CustomName:{CHANGED}}}',target=True)
    p.name('11 200 4',CHANGED)
    p.action(f'data merge block 11 200 4 {{CustomName:{CHANGED}}}',target=True)
    p.action('data remove block 11 200 4 CustomName',target=True)
    p.name('11 200 4',None)
    p.action('data merge block 11 200 4 {CustomName:"PlainAgain"}')
    p.name('11 200 4','"PlainAgain"',plain=True)
    p.action('data merge block 11 200 4 {CustomName:"PlainAgain"}')
    p.action('data modify block 8 200 4 CustomName set value {text:"Chest",color:"red",italic:0b}',target=True)
    p.name('8 200 4','{text:"Chest",color:"red",italic:0b}')
    p.action(f'data modify block 8 200 4 CustomName set value {TRANSLATED}',target=True)
    p.name('8 200 4',TRANSLATED)
    p.cmd('clone 8 200 4 8 200 4 11 200 8 replace')
    p.be('11 200 8','shulker_box');p.name('11 200 8',TRANSLATED)
    # Actual connected-owner placement, with source item and consumed count.
    p.cmd('clear @s')
    p.guard('unless data entity @s Inventory[0]')
    p.cmd('tp @s 8.5 200 9.5')
    p.guard('if data entity @s {Pos:[8.5d,200d,9.5d],Health:20f}')
    source=f'{{Slot:0b,id:"minecraft:chest",count:1,components:{{"minecraft:custom_name":{STYLED},"minecraft:container":[{{slot:0,item:{{id:"minecraft:diamond",count:3,components:{{"minecraft:custom_data":{{h210:1}}}}}}}},{{slot:26,item:{{id:"minecraft:apple",count:5}}}}]}}}}'
    p.cmd(f'item replace entity @s hotbar.0 with minecraft:chest[custom_name={STYLED},container=[{{slot:0,item:{{id:"minecraft:diamond",count:3,components:{{"minecraft:custom_data":{{h210:1}}}}}}}},{{slot:26,item:{{id:"minecraft:apple",count:5}}}}]] 1')
    p.guard(f'if data entity @s {{Inventory:[{source}]}}')
    p.cmd('data get entity @s Inventory',diagnostic=True)
    p.ops.append(dict(op='use_block',pos=[8,199,11],sequence=1))
    p.be('8 200 11','chest');p.name('8 200 11',STYLED)
    p.guard('unless data entity @s Inventory[{Slot:0b}]')
    p.cmd('data get entity @s Inventory',diagnostic=True)
    for x,component,plain in [(2,'"PlainDrop"',True),(5,STYLED,False)]:
        xyz=f'{x} 200 11'
        p.block(xyz,'shulker_box',component)
        p.name(xyz,component,plain=plain)
        p.action(f'setblock {xyz} air destroy')
        p.guard(f'if block {xyz} air')
        area=f'@e[type=minecraft:item,x={x},y=199,z=11,dx=1,dy=2,dz=1'
        p.guard(f'if entity {area}]')
        item=area+',limit=1]'
        p.guard(f'if data entity {item} {{Item:{{id:"minecraft:shulker_box",count:1}}}}')
        for path in ['Item.id','Item.count','Item.components."minecraft:container"[{slot:0}].item.count','Item.components."minecraft:container"[{slot:26}].item.count','Item.components."minecraft:container"[{slot:0}].item.components."minecraft:custom_data".h210']:
            p.cmd(f'data get entity {item} {path}',admission=True)
        p.guard(f'if data entity {item} Item.components{{"minecraft:custom_name":{component}}}',target=not plain)
        p.cmd(f'data get entity {item} Item.components."minecraft:custom_name"',target=not plain,diagnostic=not plain)
        if not plain:
            for path in ['text','color','bold','extra[0].text','extra[0].italic']:
                p.cmd(f'data get entity {item} Item.components."minecraft:custom_name".{path}',target=True)
        for path in ['Item','UUID','Pos','Motion','']:
            p.cmd(f'data get entity {item} {path}'.strip(),diagnostic=True)
    p.guard('unless data entity @s Inventory[{Slot:0b}]')
    p.clock()
    return p.ops

def stage_two():
    p=Program('h210b');p.setup(restart=True)
    for xyz,kind,name,plain in [('2 200 4','chest','"Box"',True),('5 200 4','barrel',STYLED,False),('8 200 4','shulker_box',TRANSLATED,False),('11 200 4','chest','"PlainAgain"',True),('11 200 8','shulker_box',TRANSLATED,False),('8 200 11','chest',STYLED,False)]:
        p.be(xyz,kind);p.name(xyz,name,plain=plain)
    p.guard(f'if data block 5 200 4 {{{LOCK}}}')
    # Original executable owned-name/source-overlay removal control. Its disk
    # result stays visible; production scope was reported before all captures.
    p.action('data remove block 5 200 4 CustomName',target=True)
    p.name('5 200 4',None)
    p.guard(f'if data block 5 200 4 {{{LOCK}}}')
    p.clock()
    return p.ops

if __name__=='__main__':
    for name,program in [('stage-one',stage_one()),('stage-two',stage_two())]:
        rows=[x for x in program if x['op']=='cmd' and x.get('record',True)]
        print(name,'ops',len(program),'records',len(rows),'strict',sum(not r.get('diagnostic') for r in rows),'admissions',sum(bool(r.get('h210_admission')) for r in rows))


def overlay_restart_programs():
    """Additional, independently admitted plain-name load/remove/reopen proof.

    The locked barrel retains its real original source; the unlocked chest is
    an ordinary comparison. All six bodies retain the existing typed contents.
    """
    subjects = [('2 200 4','chest'),('5 200 4','barrel'),('8 200 4','shulker_box'),('11 200 4','chest'),('11 200 8','shulker_box'),('8 200 11','chest')]
    programs=[]
    for index in range(3):
        p=Program('h210o'+str(index));p.setup(restart=index>0)
        if index>0:
            # Predeclared embedded PrimaryLevelData metadata boundary, complete
            # chronological replies remain in the raw diagnostic channel.
            for op in p.ops:
                if op.get('cmd') in ['/gamerule spawn_mobs false','/gamerule show_advancement_messages false']:
                    op['diagnostic']=True
        for xyz,kind in subjects:
            if index==0:p.block(xyz,kind,'"Original"',locked=xyz=='5 200 4')
            else:p.be(xyz,kind)
            absent=index==2 and xyz in ['2 200 4','5 200 4']
            p.name(xyz,None if absent else '"Original"',plain=not absent)
        p.guard(f'if data block 5 200 4 {{{LOCK}}}')
        if index==1:
            for xyz in ['2 200 4','5 200 4']:
                p.action('data remove block '+xyz+' CustomName',target=True)
                p.name(xyz,None)
            p.guard(f'if data block 5 200 4 {{{LOCK}}}')
        p.clock();programs.append(('stage-'+['one','two','three'][index],p.ops))
    return programs
