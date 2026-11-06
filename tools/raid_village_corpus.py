"""Live claimed-bed removal after two admitted first waves, 1024 blocks apart."""
from command_exec_corpus import cmd


def raid_villages():
    ops = []
    def run(c, expected=None, record=True, trace=False, label=None):
        o = cmd(c, record, expected)
        if trace:
            o['trace'] = True
        if label:
            o['label'] = label
        ops.append(o)
    def quiet(c):
        run(c, record=False, trace=True)
    def step(n):
        run(f'/tick step {n}')
        ops.append({'op':'tick','n':n})
    def actor(i, noai=False):
        x = (i-1)*1024
        u = f'000000a2-0000-0000-0000-{i:012x}'
        run(f'/execute if entity @e[type=villager,tag=h162_{i},nbt={{UUID:[I;162,0,0,{i}],Health:20f,PersistenceRequired:1b}}]',1)
        run(f'/data get entity {u} UUID')
        run(f'/data get entity {u} Health',20)
        run(f'/execute {"if" if noai else "unless"} data entity {u} {{NoAI:1b}}',1)
        run(f'/data get entity {u} Brain.memories."minecraft:home".value.pos')
        run(f'/execute if data entity {u} {{Brain:{{memories:{{"minecraft:home":{{value:{{dimension:"minecraft:overworld",pos:[I;{x+2},200,1]}}}}}}}}}}',1)
        run(f'/execute if entity @e[type=villager,tag=h162_{i},x={x},y=199,z=0,dx=6,dy=7,dz=6]',1)
        run(f'/data get entity {u}',record=False,trace=True)
    def raid(i, label, status='ongoing', started=True, groups=1, village=True, members=False):
        state={'id':i+1,'present':True,'status':status,'started':started,'active':True,
               'groups_spawned':groups,'center':[(i-1)*1024+2,200,1],'village':village}
        if members:
            state['member_present']=True
        ops.append({'op':'raid_state','id':i+1,'label':label,'poi':[(i-1)*1024+2,200,1],
                    'members':members,'expect_poi_occupied':1 if village else 0,
                    'villagers':[f'000000a2-0000-0000-0000-{j:012x}' for j in [1,2]],
                    'expect_observation':state})
    run('/tick freeze',1)
    for c in ['/gamemode creative','/difficulty normal','/gamerule spawn_mobs false',
              '/gamerule advance_time false','/gamerule advance_weather false',
              '/gamerule random_tick_speed 0','/gamerule raids true',
              '/gamerule spawn_patrols false','/gamerule spawn_wandering_traders false',
              '/time set midnight','/weather clear']:
        run(c)
    quiet('/kill @e[type=!player]')
    run('/scoreboard objectives add h162 dummy',1)
    for x in [0,1024]:
        run(f'/forceload add {x-80} -80 {x+79} 79',100)
    chunks=[[x+dx,dz] for x in [0,64] for dx in range(-5,5) for dz in range(-5,5)]
    ops.append({'op':'settle','required_chunks':chunks})
    for cx,cz in chunks:
        run(f'/execute if loaded {cx*16} 200 {cz*16}',1)
    for i,x in enumerate([0,1024],1):
        run(f'/fill {x-63} 199 -63 {x+63} 199 63 stone',16129)
        for y in [200,202,204]:
            quiet(f'/fill {x-63} {y} -63 {x+63} {y+1} 63 air')
        run(f'/fill {x} 200 0 {x+6} 205 6 glass',294)
        run(f'/fill {x+1} 200 1 {x+5} 204 5 air',125)
        for dx,dz in [(-63,-63),(-63,63),(63,-63),(63,63),(0,0),(2,1),(8,8)]:
            run(f'/execute if block {x+dx} 199 {dz} stone',1)
        run(f'/setblock {x+2} 200 2 white_bed[facing=north,part=foot]')
        run(f'/setblock {x+2} 200 1 white_bed[facing=north,part=head]')
        run(f'/execute if block {x+2} 200 2 white_bed[facing=north,part=foot]',1)
        run(f'/execute if block {x+2} 200 1 white_bed[facing=north,part=head]',1)
        run(f'/summon villager {x+3}.5 200 2.5 {{UUID:[I;162,0,0,{i}],Tags:["h162_{i}"],PersistenceRequired:1b}}',1)
        run(f'/execute if entity @e[type=villager,tag=h162_{i},nbt={{UUID:[I;162,0,0,{i}],Health:20f,Pos:[{x+3}.5d,200d,2.5d]}}]',1)
    run('/tp @s 8.5 200 8.5 0 0')
    quiet('/execute store result score start h162 run time query gametime')
    step(400)
    for i in [1,2]:
        actor(i)
    for i,x in enumerate([0,1024],1):
        run(f'/tp @s {x+8}.5 200 8.5 0 0')
        run('/raid start 1',1)
        raid(i,f'created-{i}',started=False,groups=0)
    step(400)
    for kind in ["pillager","vindicator","evoker","witch","ravager"]:
        quiet(f'/execute as @e[type={kind}] run data get entity @s UUID')
    for i in [1,2]:
        actor(i)
        raid(i,f'wave-{i}',members=True)
    quiet('/execute store result score before_write h162 run time query gametime')
    for i in [1,2]:
        run(f'/data merge entity 000000a2-0000-0000-0000-{i:012x} {{NoAI:1b}}',1)
        actor(i,True)
        raid(i,f'after-noai-{i}',members=True)
    quiet('/execute store result score delta h162 run time query gametime')
    quiet('/scoreboard players operation delta h162 -= before_write h162')
    run('/scoreboard players get delta h162',0)
    run('/fill 1026 200 1 1026 200 2 air')
    for z in [1,2]:
        run(f'/execute if block 1026 200 {z} air',1)
        run(f'/execute if block 2 200 {z} white_bed',1)
    for n,elapsed in [(1,1),(19,20),(580,600)]:
        step(n)
        quiet('/execute store result score delta h162 run time query gametime')
        quiet('/scoreboard players operation delta h162 -= before_write h162')
        run('/scoreboard players get delta h162',elapsed)
        for i in [1,2]:
            actor(i,True)
            raid(i,f'after-{elapsed}-{i}',status='ongoing' if i==1 else 'loss',village=i==1)
            run(f'/execute if loaded {(i-1)*1024+2} 200 1',1)
        run('/execute if entity @s[type=player,gamemode=creative,nbt={Health:20f,Pos:[1032.5d,200d,8.5d]}]',1)
        run('/data get entity @s UUID',record=False,trace=True)
    return ops


def validate_java_inputs(build, script):
    """Actual PoiManager occupation, and zero-tick NoAI write identity/binding controls."""
    import json
    records=[json.loads(line.split('raid input raw: ',1)[1]) for line in (build/'java.log').read_text().splitlines() if 'raid input raw: ' in line]
    ops=[op for op in script if op['op']=='raid_state']
    if len(records)!=len(ops):
        raise RuntimeError('Java raid input witness count differs')
    for op,raw in zip(ops,records):
        if raw['label']!=op['label'] or not raw['loaded'] or raw['occupied']!=op['expect_poi_occupied']:
            raise RuntimeError(f'Java actual occupied-POI prerequisite failed: {op}: {raw}')
    by_label={r['label']:r for r in records}
    for i in [1,2]:
        before,after=by_label[f'wave-{i}'],by_label[f'after-noai-{i}']
        def actors(row):
            return sorted((tuple(a['uuid']),a['type'],a['health'],tuple(a['pos']),a['raid_id']) for a in row['actors'])
        if not before['actors'] or actors(before)!=actors(after) or any(a['raid_id']!=i+1 or a['health']<=0 for a in before['actors']):
            raise RuntimeError('Java NoAI write changed actual admitted raid members')
        a,b=before['villagers'][i-1],after['villagers'][i-1]
        if a['noai'] or not b['noai'] or any(a[k]!=b[k] for k in ['uuid','type','health','pos','home']):
            raise RuntimeError('Java NoAI write changed live villager identity/health/home/position')


def validate_native_inputs(build, script):
    """The observer exposes production inputs, not a newly computed occupied-POI answer."""
    import json
    records=[json.loads(line.split('raid input raw: ',1)[1]) for line in (build/'rust.log').read_text().splitlines() if 'raid input raw: ' in line]
    ops=[op for op in script if op['op']=='raid_state']
    if len(records)!=len(ops) or any(not r['loaded'] or r['label']!=op['label'] for r,op in zip(records,ops)):
        raise RuntimeError('Native actual loaded input/record prerequisite failed')
    by_label={r['label']:r for r in records}
    for i in [1,2]:
        before,after=by_label[f'wave-{i}'],by_label[f'after-noai-{i}']
        def actors(row):
            return sorted((tuple(a['uuid']),a['type'],a['health'],tuple(a['pos']),a['raid_id']) for a in row['actors'])
        if not before['actors'] or actors(before)!=actors(after) or any(a['raid_id']!=i+1 or a['health']<=0 for a in before['actors']):
            raise RuntimeError('Native NoAI write changed actual admitted raid members')
        a,b=before['villagers'][i-1],after['villagers'][i-1]
        if a['noai'] or not b['noai'] or any(a[k]!=b[k] for k in ['uuid','type','health','pos','home']):
            raise RuntimeError('Native NoAI write changed actual villager identity/health/home/position')
        if before['live_poi']!='Some(Home)' or after['live_poi']!='Some(Home)' or before['claimed_pois']!=after['claimed_pois']:
            raise RuntimeError('Native NoAI write changed existing live POI/claimed-position inputs')
