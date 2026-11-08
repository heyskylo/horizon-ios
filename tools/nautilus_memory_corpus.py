"""H140: class-owned cooldown value/presence/TTL, not attack or trajectory parity."""
import hashlib

KEY = 'minecraft:attack_target_cooldown'
MAX = 9223372036854775807

def owner_words():
    raw = bytearray(hashlib.md5(b'OfflinePlayer:Player').digest())
    raw[6] = (raw[6] & 15) | 48
    raw[8] = (raw[8] & 63) | 128
    return [int.from_bytes(raw[i:i+4], 'big', signed=True) for i in range(0, 16, 4)]

def actors():
    out=[]
    states=[('absent',None,None),('zero',0,None),('permanent',37,None),('finite',37,5),('max',37,MAX)]
    for species_index,kind in enumerate(['nautilus','zombie_nautilus'],1):
        for name,value,ttl in states:
            out.append(dict(kind=kind,name=name,value=value,ttl=ttl,noai=True,species=species_index))
    for species_index,kind in enumerate(['nautilus','zombie_nautilus'],1):
        for ttl in [5,1,0]:
            out.append(dict(kind=kind,name=f'active_ttl{ttl}',value=37,ttl=ttl,noai=False,species=species_index))
    for i,a in enumerate(out):
        a.update(n=i+1,x=2.5+3*(i%4),z=2.5+3*(i//4),y=201.0)
        a['uuid']=f'[I;140,{a["species"]},{int(a["noai"])},{i+1}]'
    return out

def expected(a,elapsed):
    if a['value'] is None:return None
    if a['noai'] or elapsed==0:
        return a['value'],None if a['ttl'] in [None,MAX] else a['ttl']
    if a['ttl']==0:return None
    return a['value']-elapsed,None

def cases(cmd):
    out=[]
    def add(s,n=None,guard=False,target=False,diagnostic=None,callback=None,**kw):
        row=cmd(s) if n is None else cmd(s,expect_result=n)
        if guard:row['h140_admission']=True
        if target:row['h140_target']=True
        if diagnostic:row['diagnostic']=diagnostic
        if callback is not None:row['expect_callback']=callback
        row.update(kw);out.append(row)
    def selector(a):return '@e[type=minecraft:'+a['kind']+',nbt={UUID:'+a['uuid']+'},limit=1]'
    def memory(value,ttl):
        if value is None:return '{}'
        return '{"'+KEY+'":{value:'+str(value)+('' if ttl is None else ',ttl:'+str(ttl)+'L')+'}}'
    def guard_actor(a, elapsed=0):
        y = 201.0 if a['noai'] or elapsed == 0 else {1: 201.005, 6: 201.08914845}[elapsed]
        fields='UUID:'+a['uuid']+',Health:10f'+(',NoAI:1b' if a['noai'] else '')+',NoGravity:1b,Pos:['+f'{a["x"]}d,{y}d,{a["z"]}d'+'],Sitting:0b'
        if not a['noai']:fields+=',Owner:[I;'+','.join(map(str,owner_words()))+'],attributes:[{id:"minecraft:movement_speed",base:0d}]'
        add('/execute if entity @e[type=minecraft:'+a['kind']+',nbt={'+fields+'}]',1,True)
        if not a['noai']:
            add('/execute unless data entity '+selector(a)+' NoAI',1,True)
            add('/data get entity '+selector(a)+' Pos',3,True)
        add(f'/execute if block {int(a["x"])} 199 {int(a["z"])} stone',1,True)
        add(f'/execute if block {int(a["x"])} 201 {int(a["z"])} water',1,True)
    add('/tick freeze',guard=True)
    add('/gamerule spawn_mobs false',0,True)
    add('/gamerule show_advancement_messages false',0,True)
    add('/scoreboard objectives add h140 dummy',1,True)
    add('/forceload add 0 0 15 15',1,True);out.append({'op':'settle'})
    add('/execute if loaded 8 201 8',1,True)
    add('/fill 0 199 0 15 205 15 stone',guard=True)
    add('/fill 1 200 1 14 204 14 water',980,True)
    add('/gamemode spectator',guard=True);add('/tp @s 8.5 207 8.5',1,True)
    add('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:3,Pos:[8.5d,207d,8.5d],UUID:[I;'+','.join(map(str,owner_words()))+']}]',1,True)
    add('/data get entity @s UUID',4,True)
    add('/execute store result score #before h140 run time query gametime',diagnostic='actual absolute initial epoch')
    aa=actors()
    for a in aa:
        nbt='{UUID:'+a['uuid']+',Tags:["h140"],Health:10f,NoAI:'+str(int(a['noai']))+'b,NoGravity:1b,PersistenceRequired:1b,Brain:{memories:'+memory(a['value'],a['ttl'])+'}'
        if not a['noai']:nbt+=',Owner:[I;'+','.join(map(str,owner_words()))+'],attributes:[{id:"minecraft:movement_speed",base:0d}]'
        nbt+='}'
        add(f'/summon minecraft:{a["kind"]} {a["x"]} 201 {a["z"]} '+nbt,1,True)
        add('/data get entity '+selector(a)+' UUID',4,True)
        guard_actor(a)
    for elapsed in [0,1,6]:
        if elapsed:
            ticks=1 if elapsed==1 else 5
            add(f'/tick step {ticks}',guard=True)
            out.append({'op':'tick','n':ticks,'record':True,'h140_clock_receipt':elapsed})
        add('/execute store result score #elapsed h140 run time query gametime',diagnostic='actual absolute scene epoch')
        add('/scoreboard players operation #elapsed h140 -= #before h140')
        add('/scoreboard players get #elapsed h140',elapsed,True)
        add('/execute if entity @s[type=player,nbt={Health:20f,playerGameType:3,Pos:[8.5d,207d,8.5d]}]',1,True)
        for a in aa:
            if elapsed == 1 and a['noai']:
                continue  # NoAI retention is measured at 0 and 6; active boundaries at 0/1/6.
            if elapsed:
                guard_actor(a, elapsed)
            sel=selector(a);path='Brain.memories."'+KEY+'"';state=expected(a,elapsed)
            present=state is not None
            add('/scoreboard players set #r h140 91',91,True)
            add('/scoreboard players set #s h140 17',17,True)
            add('/execute store result score #r h140 store success score #s h140 if data entity '+sel+' '+path,target=True,callback=[present,int(present)],h140_probe=f'{a["kind"]}/{a["name"]}/{elapsed}/presence')
            add('/scoreboard players get #r h140',int(present),target=True)
            add('/scoreboard players get #s h140',int(present),target=True)
            add('/data get entity '+sel+' '+path+'.value',target=True,callback=[present,0 if not present else state[0]])
            ttl=None if not present else state[1]
            add('/data get entity '+sel+' '+path+'.ttl',target=True,callback=[ttl is not None,0 if ttl is None else ttl])
            if present:
                expected_mem=memory(state[0],ttl)
                add('/execute if entity @e[type=minecraft:'+a['kind']+',nbt={UUID:'+a['uuid']+',Brain:{memories:'+expected_mem+'}}]',1,target=True)
            else:
                add('/execute unless data entity '+sel+' '+path,1,target=True)
            add('/data get entity '+sel+' '+path,diagnostic='complete memory compound: type/field-order retained separately')
            if elapsed in [0,6]:
                add('/data get entity '+sel+' Brain',diagnostic='complete Brain/provider state')
                add('/data get entity '+sel,diagnostic='complete actor body/pose/unrelated defaults')
    add('/forceload remove 0 0 15 15',1)
    add('/scoreboard objectives remove h140',0)
    add('/tick unfreeze')
    return out
