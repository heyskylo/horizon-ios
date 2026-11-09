#!/usr/bin/env python3
"""Real vanilla PlaceRecipe packets and authoritative cooking-menu state.

Full chronological packets, typed command storage and decoded menu bodies remain
raw. Display/window IDs are discovered from actual packets, never registered here.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time
import datetime
import hashlib
import struct
import uuid
import re

try:
    from . import anvil_dump as nbt, net_capture, test_locator_bar_parity as net
except ImportError:
    import anvil_dump as nbt
    import net_capture
    import test_locator_bar_parity as net

def offline(name):
    raw = bytearray(hashlib.md5(f'OfflinePlayer:{name}'.encode()).digest())
    raw[6], raw[8] = (raw[6] & 15) | 48, (raw[8] & 63) | 128
    return str(uuid.UUID(bytes=bytes(raw)))


def uuid_words(name):
    import struct
    return list(struct.unpack('>4i', uuid.UUID(offline(name)).bytes))


def stack(value):
    return None if value is None else nbt.read_named(bytes.fromhex(value['nbt']))


def wait_new(server, start, pattern):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        if any(re.search(pattern, line) for line in server.lines[start:]):
            return
        if server.process.poll() is not None:
            break
        time.sleep(.02)
    raise AssertionError(f'no new {pattern}: {server.lines[start:][-10:]}')


def generate(command, build):
    directory = build / 'generate'
    net.server_dir(directory)
    server = net.Server(command, directory, 'generate')
    try:
        server.wait_for(r'Done \(')
        for cmd, ack in [('tick freeze', 'frozen'), ('gamerule spawn_mobs false', 'spawn_mobs'),
                         ('gamerule advance_time false', 'advance_time'), ('gamerule respawn_radius 0', 'respawn_radius'),
                         ('forceload add 0 0 31 31', 'force loaded'), ('fill 0 99 0 31 99 31 stone', '1024'),
                         ('fill 0 100 0 31 104 31 air', 'blocks'), ('setworldspawn 4 100 4', 'world spawn'),
                         ('save-all flush', 'Saved the game')]:
            start = len(server.lines)
            server.console(cmd)
            wait_new(server, start, ack)
    finally:
        server.stop()
    (directory / 'finished').touch()
    return directory / 'world'


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def dump(client, label):
    client.tell('dump ' + label)
    while (line := client.process.stdout.readline()) != 'dumped ' + label + '\n':
        if not line:
            raise AssertionError('client exited before dump')
        client.log.write(line)


class Commands:
    def __init__(self, server, path):
        self.server, self.path, self.serial, self.rows = server, path, 0, []

    def reply(self, command):
        self.serial += 1
        start = len(self.server.lines)
        row = dict(command=command, sent_at=now(), receipt_at=None, lines=[])
        self.rows.append(row)
        self.server.console(command)
        self.server.console(f'data modify storage h068:barrier value set value {self.serial}')
        self.server.console('data get storage h068:barrier value')
        try:
            self.server.wait_for(rf'h068:barrier.*contents: {self.serial}$', timeout=90)
            row['receipt_at'] = now()
            return '\n'.join(self.server.lines[start:])
        finally:
            row['lines'] = self.server.lines[start:]
            self.path.write_text(json.dumps(self.rows, indent=2) + '\n')

    def passed(self, command):
        text = self.reply(command)
        assert 'Test passed' in text, (command, text)


PLAYER = 'Bob'
SCENES = [('inventory', None, 'oak_planks', 'minecraft:crafting_shapeless', 3),
          ('table', 'crafting_table', 'oak_planks', 'minecraft:crafting_shapeless', 3),
          ('furnace', 'furnace', 'charcoal', 'minecraft:furnace', 3),
          ('ghost', 'furnace', 'charcoal', 'minecraft:furnace', 0),
          ('forgotten', 'furnace', 'charcoal', 'minecraft:furnace', 3)]


def program():
    return dict(scenes=SCENES, player=PLAYER, pos=[4.5,100.,4.5], observation_ticks=0,
                action='real ServerboundPlaceRecipe, same-socket time-query receipt barrier; no result clicks',
                strict='typed actual player inventory and machine slots, real menu input/output/fuel, ghost presence/window, guards and relative clock',
                diagnostics='complete raw packets/order, display IDs/body, menu state revisions, saved bodies and absolute clocks',
                packet_receipt_time=None)


def packets(path):
    result=[]
    if path.exists():
        for line in path.read_text().splitlines():
            if line.startswith(PLAYER+' step=') and ' menu-wire ' in line:
                prefix,body=line.split(' menu-wire ',1)
                result.append((prefix.split(' step=',1)[1],json.loads(body)))
    return result


def menu(path):
    windows={}; opened=0; carried=None; revisions={}
    for _,p in packets(path):
        k=p['kind']
        if k=='open': opened=p['window']
        elif k=='content':
            windows[p['window']]=[stack(x) for x in p['items']]
            revisions[p['window']]=p['state'];carried=stack(p['carried'])
        elif k=='slot' and p['window'] in windows:
            windows[p['window']][p['slot']]=stack(p['item']);revisions[p['window']]=p['state']
        elif k=='cursor':carried=stack(p['item'])
        elif k=='inventory':
            i=p['slot']; item=stack(p['item'])
            if 0 in windows:
                s=36+i if i<9 else i if i<36 else 44-i if i<40 else 45
                windows[0][s]=item
            if opened in windows and opened and i<36:
                own=len(windows[opened])-36
                windows[opened][own+27+i if i<9 else own+i-9]=item
    return dict(windows=windows,opened=opened,carried=carried,revisions=revisions)


def run(label,command,client_command,build,world):
    directory=build/label;port=net.server_dir(directory,world)
    server=net.Server(command,directory,label);commands=Commands(server,build/(label+'-commands.json'))
    wire=build/(label+'-wire.txt');log=(build/(label+'-client.log')).open('w')
    capture=build/(label+'-packets.jsonl');listen=net.free_port();stop=None;client=None;checkpoints={};actions=[]
    try:
        server.wait_for(r'Done \(')
        commands.reply('tick freeze')
        stop=net_capture.start_background(listen,f'127.0.0.1:{port}',capture,full=True)
        client=net.Client(client_command,listen,wire,log)
        for line in ['menus on','chat on','state on','join '+PLAYER]:client.tell(line)
        server.wait_for(PLAYER+' joined the game',timeout=120);client.sync([PLAYER])
        def cmd(c):return commands.reply(c)
        def checkpoint(name):
            client.sync([PLAYER]);dump(client,name);m=menu(wire);checkpoints[name]=m
            (build/(label+'-menus.json')).write_text(json.dumps(checkpoints,indent=2)+'\n');return m
        def wait(name,predicate):
            end=time.monotonic()+10
            while True:
                m=checkpoint(name)
                if predicate(m):return m
                assert time.monotonic()<end,(name,m)
                time.sleep(.05)
        def guard(name,condition):
            cmd('data modify storage h234:proof guards.'+name+' set value {result:91,success:17b}')
            cmd('execute store result storage h234:proof guards.'+name+'.result int 1 store success storage h234:proof guards.'+name+'.success byte 1 run execute '+condition)
        def snapshot(name,pos=None):
            cmd(f'data modify storage h234:proof {name}.player set from entity {PLAYER}')
            cmd(f'data modify storage h234:proof {name}.clock set value 0L')
            cmd(f'execute store result storage h234:proof {name}.clock long 1 run time query gametime')
            if pos:cmd(f'data modify storage h234:proof {name}.block set from block {pos}')
            checkpoint(name)
        cmd('gamemode survival Bob');cmd('tp Bob 4.5 100 4.5 0 0');cmd('clear Bob')
        words=','.join(map(str,uuid_words(PLAYER)))
        guard('identity',f'if entity @a[name=Bob,type=player,nbt={{UUID:[I;{words}],Health:20f,Pos:[4.5d,100d,4.5d],playerGameType:0}}]')
        guard('loaded','if loaded 6 100 4');guard('floor','if block 4 99 4 stone')
        snapshot('initial')
        for name,block,recipe,display_type,logs in SCENES:
            client.tell('step '+name+'-setup')
            if name!='inventory':
                # Close only the actually opened window; a close is not a result click.
                previous=menu(wire)['opened']
                if previous:client.tell(f'close-menu Bob {previous}');client.sync([PLAYER])
            cmd('clear Bob');cmd('item replace entity Bob hotbar.8 with stick[minecraft:custom_data={h234:"untouched"}] 7')
            if logs:cmd(f'item replace entity Bob hotbar.0 with oak_log {logs}')
            cmd(f'recipe take Bob minecraft:{recipe}');cmd(f'recipe give Bob minecraft:{recipe}')
            client.sync([PLAYER]);checkpoint(name+'-known')
            matches=[e for _,p in packets(wire) if p['kind']=='recipes' for e in p['entries']
                     if e['display']['type']==display_type and 'minecraft:'+('charcoal' if recipe=='charcoal' else 'oak_planks') in json.dumps(e['display']['result'])]
            assert matches,('actual advertised recipe missing',name,packets(wire)[-10:])
            rid=matches[-1]['id'];pos='6 100 4' if block else None
            if block:
                cmd(f'setblock {pos} air strict');cmd(f'setblock {pos} {block} strict')
                guard(name+'_block',f'if block {pos} {block}')
                if block=='furnace':
                    cmd(f'item replace block {pos} container.1 with coal[minecraft:custom_data={{h234:"fuel"}}] 2')
                client.tell(f'use-on Bob {pos} 1')
                m=wait(name+'-open',lambda m:m['opened'] in m['windows'] and m['opened']!=previous and len(m['windows'][m['opened']])==(46 if block=='crafting_table' else 39) and any(p['kind']=='open' and p['window']==m['opened'] and p['type']==('minecraft:crafting' if block=='crafting_table' else 'minecraft:furnace') for _,p in packets(wire)))
                window=m['opened']
            else:window=0
            if name=='forgotten':cmd('recipe take Bob minecraft:charcoal')
            snapshot(name+'_before',pos if block=='furnace' else None)
            client.tell('step '+name+'-place');client.sync([PLAYER])
            actions.append(dict(scene=name,window=window,display=rid,use_max=False))
            (build/(label+'-actions.json')).write_text(json.dumps(actions,indent=2)+'\n')
            client.tell(f'place-recipe Bob {window} {rid} false');client.sync([PLAYER])
            # Time-query barrier admits receive order. Normal container broadcast runs
            # independently of frozen world simulation; preserve every raw revision.
            time.sleep(.15);snapshot(name+'_after',pos if block=='furnace' else None)
        client.tell('step cleanup')
        previous=menu(wire)['opened']
        if previous:client.tell(f'close-menu Bob {previous}');client.sync([PLAYER])
        cmd('clear Bob');cmd('tick unfreeze');cmd('save-all flush');client.end()
    finally:
        if client:client.kill()
        if stop:stop.set()
        server.stop();log.close()
    storage=list((directory/'world').rglob('*command_storage*.dat'))
    raw={str(p.relative_to(directory)):nbt.read_dat(p) for p in storage}
    (build/(label+'-storage.json')).write_text(json.dumps(raw,indent=2)+'\n')
    return facts(build,label)


def facts(build,label):
    raw=json.loads((build/(label+'-storage.json')).read_text())
    proof=next(v['data']['contents']['proof'] for v in raw.values() if 'proof' in v['data']['contents'])
    points=json.loads((build/(label+'-menus.json')).read_text()); actions=json.loads((build/(label+'-actions.json')).read_text())
    out={};start=proof['initial']['clock'][1]
    for k,v in proof['guards'].items():out['guard/'+k]=v
    for name,v in proof.items():
        if name=='guards':continue
        out[name+'/clock']=[v['clock'][0],v['clock'][1]-start]
        for key in ['UUID','Health','Pos','playerGameType','Inventory']:out[name+'/player/'+key]=v['player'].get(key)
        if name!='initial':
            scene=name.rsplit('_',1)[0];recipe=next(row[2] for row in SCENES if row[0]==scene)
            out[name+'/known']=('minecraft:'+recipe) in v['player'].get('recipeBook',{}).get('recipes',[])
        if 'block' in v:
            out[name+'/block/id']=v['block']['id'];out[name+'/block/Items']=v['block'].get('Items',[])
            out[name+'/block/xyz']=[v['block'][k] for k in ['x','y','z']]
    for a in actions:
        name=a['scene'];w=str(a['window']);m=points[name+'_after']
        out[name+'/menu/own']=m['windows'][w][:5 if name=='inventory' else 10 if name=='table' else 3]
        out[name+'/menu/cursor']=m['carried']
        out[name+'/menu/slot-count']=len(m['windows'][w])
        out[name+'/menu/state-valid']=type(m['revisions'][w]) is int and m['revisions'][w]>=0
        ghosts=[p for stage,p in packets(build/(label+'-wire.txt')) if stage==name+'-place' and p['kind']=='ghost']
        out[name+'/ghost/windows']=[p['window']==a['window'] for p in ghosts]
    raw_packets=net_capture.load(build/(label+'-packets.jsonl'))
    requests=[p for p in raw_packets if p['dir']=='c2s' and p['state']=='play' and p['name']=='place_recipe']
    decoded=[]
    for p in requests:
        data=bytes.fromhex(p['data']);window,i=net_capture.read_varint(data,0);display,i=net_capture.read_varint(data,i)
        assert len(data)==i+1 and data[i] in (0,1),'incomplete official PlaceRecipe payload'
        decoded.append([window,display,bool(data[i])])
    out['protocol/actual-requests-match-actions']=decoded==[[a['window'],a['display'],a['use_max']] for a in actions]
    out['protocol/request-count']=len(decoded)
    out['protocol/result-click-count']=sum(p['dir']=='c2s' and p['name']=='container_click' for p in raw_packets)
    acks=[]
    for p in raw_packets:
        if p['dir']=='s2c' and p['state']=='play' and p['name']=='block_changed_ack':
            data=bytes.fromhex(p['data']);value,i=net_capture.read_varint(data,0);assert i==len(data);acks.append(value)
    out['protocol/actual-use-acks']=acks
    return out


def admit(f):
    assert all(v=={'result':1,'success':['b',1]} for k,v in f.items() if k.startswith('guard/')),f
    assert all(v==['l',0] for k,v in f.items() if k.endswith('/clock'))
    assert f['protocol/actual-requests-match-actions'] and f['protocol/request-count']==5
    assert f['protocol/result-click-count']==0 and f['protocol/actual-use-acks']==[1,2,3,4]
    for key,value in f.items():
        if key.endswith('/player/UUID'):assert value==['I',uuid_words(PLAYER)],(key,value)
        if key.endswith('/player/Health'):assert value==['f',20.],(key,value)
        if key.endswith('/player/Pos'):assert value==[['d',4.5],['d',100.],['d',4.5]],(key,value)
        if key.endswith('/player/playerGameType'):assert value==0,(key,value)
    for name,_,_,_,n in SCENES:
        assert f[name+'_before/known']==(name!='forgotten') and f[name+'_after/known']==(name!='forgotten')
        assert f[name+'/menu/slot-count']==(46 if name in ['inventory','table'] else 39)
        assert f[name+'/menu/state-valid']
        before=f[name+'_before/player/Inventory'];after=f[name+'_after/player/Inventory']
        def count(xs,id):return sum(x['count'] for x in xs if x['id']=='minecraft:'+id)
        assert count(before,'oak_log')==n,(name,before)
        assert count(after,'charcoal')==0,(name,after)
        assert count(after,'stick')==7,(name,after)
        placed=name in ['inventory','table','furnace']
        assert count(after,'oak_log')==n-int(placed),(name,after)
        own=f[name+'/menu/own']
        if name in ['inventory','table']:
            assert own[0]=={'id':'minecraft:oak_planks','count':4},(name,own)
            assert own[1]=={'id':'minecraft:oak_log','count':1},(name,own)
        else:
            assert own[0]==({'id':'minecraft:oak_log','count':1} if name=='furnace' else None),(name,own)
            assert own[1]=={'id':'minecraft:coal','count':2,'components':{'minecraft:custom_data':{'h234':'fuel'}}},(name,own)
            assert own[2] is None,(name,own)
            assert f[name+'_before/block/xyz']==[6,100,4] and f[name+'_after/block/id']=='minecraft:furnace'
            expected=[dict(own[1],Slot=['b',1])]
            assert f[name+'_before/block/Items']==expected,(name,f[name+'_before/block/Items'])
            if name=='furnace':expected.insert(0,dict(own[0],Slot=['b',0]))
            assert f[name+'_after/block/Items']==expected,(name,f[name+'_after/block/Items'])
        assert f[name+'/ghost/windows']==([True] if name=='ghost' else []),(name,f[name+'/ghost/windows'])
    return len(f)


def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--build',default='h234-machine-recipe-placement');p.add_argument('--side',choices=['java','native'],required=True)
    args=p.parse_args();lock,build,java,native,client=net.sides(args.build,args.binary)
    net.PROPERTIES['force-gamemode']='false'
    (build/'program.json').write_text(json.dumps(program(),indent=2)+'\n')
    if args.side=='java':
        world=generate(java,build);actual=run('java',java,client,build,world);admit(actual)
        (build/'reference.json').write_text(json.dumps(actual,indent=2)+'\n');print('JAVA ADMITTED',len(actual))
    else:
        actual=run('native',native,client,build,build/'generate/world');reference=json.loads((build/'reference.json').read_text())
        diff={k:dict(java=v,native=actual.get(k)) for k,v in reference.items() if actual.get(k)!=v}
        (build/'native-facts.json').write_text(json.dumps(actual,indent=2)+'\n');(build/'diff.json').write_text(json.dumps(diff,indent=2)+'\n');print('EXACT',len(reference)-len(diff),'/',len(reference),'DIFF',json.dumps(diff,indent=2));return bool(diff)
    return 0

if __name__=='__main__':raise SystemExit(main())
