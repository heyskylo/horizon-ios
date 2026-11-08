#!/usr/bin/env python3
"""Four built-in component send paths through real inventory packets.

Primary gate is four canonical forms plus independent setup controls. Historical
forms, saved canonicalization, armor trim and removal bridges remain complete,
separately executable diagnostics; no output masks or substring component scans.
"""
import argparse
import datetime
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import struct
import subprocess
import time
import uuid
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
    return list(struct.unpack('>4i', uuid.UUID(offline(name)).bytes))


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


PLAYER="Bob"
CASES=[{'label': 'trim material',
  'item': 'minecraft:diamond[minecraft:provides_trim_material="minecraft:gold"]',
  'group': 'primary',
  'count': 1,
  'slot': 9,
  'key': 'case0'},
 {'label': 'provides banner patterns',
  'item': 'minecraft:paper[minecraft:provides_banner_patterns="#minecraft:pattern_item/globe"]',
  'group': 'primary',
  'count': 1,
  'slot': 10,
  'key': 'case1'},
 {'label': 'can place on',
  'item': 'minecraft:stone[minecraft:can_place_on={blocks:"minecraft:dirt"}]',
  'group': 'primary',
  'count': 1,
  'slot': 11,
  'key': 'case2'},
 {'label': 'can break',
  'item': 'minecraft:diamond_pickaxe[minecraft:can_break={blocks:"minecraft:stone",state:{}}]',
  'group': 'primary',
  'count': 1,
  'slot': 12,
  'key': 'case3'},
 {'label': 'trim',
  'item': 'minecraft:iron_chestplate[minecraft:trim={material:"minecraft:gold",pattern:"minecraft:coast"}]',
  'group': 'h284',
  'count': 1,
  'slot': 13,
  'key': 'case4'},
 {'label': 'can place on list',
  'item': 'minecraft:stone[minecraft:can_place_on=[{blocks:["minecraft:dirt","minecraft:sand"]},{blocks:"#minecraft:logs"}]]',
  'group': 'historical',
  'count': 1,
  'slot': 14,
  'key': 'case5'},
 {'label': 'can break with nbt',
  'item': 'minecraft:diamond_pickaxe[minecraft:can_break={blocks:"minecraft:chest",nbt:"{Lock:{}}"}]',
  'group': 'historical',
  'count': 1,
  'slot': 15,
  'key': 'case6'},
 {'label': 'instrument',
  'item': 'minecraft:goat_horn[minecraft:instrument="minecraft:dream_goat_horn"]',
  'group': 'control',
  'count': 1,
  'slot': 16,
  'key': 'case7'},
 {'label': 'can place on with a state',
  'item': 'minecraft:stone[minecraft:can_place_on={blocks:"minecraft:oak_stairs",state:{half:"top",facing:"north"}}]',
  'group': 'historical',
  'count': 1,
  'slot': 17,
  'key': 'case8'},
 {'label': 'can break with a range',
  'item': 'minecraft:diamond_pickaxe[minecraft:can_break={blocks:["minecraft:wheat"],state:{age:{min:"3",max:"7"}}}]',
  'group': 'historical',
  'count': 1,
  'slot': 18,
  'key': 'case9'},
 {'label': 'inline trim',
  'item': 'minecraft:iron_chestplate[minecraft:trim={material:{palette_id:"minecraft:gold",description:"Gold"},pattern:{asset_id:"minecraft:coast",description:{text:"Coast",color:"red"},decal:true}}]',
  'group': 'h284',
  'count': 1,
  'slot': 19,
  'key': 'case10'},
 {'label': 'inline trim material',
  'item': 'minecraft:diamond[minecraft:provides_trim_material={palette_id:"minecraft:iron",description:"Iron"}]',
  'group': 'historical',
  'count': 1,
  'slot': 20,
  'key': 'case11'},
 {'label': 'can break with components',
  'item': 'minecraft:diamond_pickaxe[minecraft:can_break={blocks:"minecraft:chest",components:{"minecraft:custom_name":"Box"}}]',
  'group': 'historical',
  'count': 1,
  'slot': 21,
  'key': 'case12'},
 {'label': 'can break with predicates',
  'item': 'minecraft:diamond_pickaxe[minecraft:can_break={blocks:"minecraft:chest",predicates:{"minecraft:custom_data":{a:1}}}]',
  'group': 'historical',
  'count': 1,
  'slot': 22,
  'key': 'case13'},
 {'label': 'plain', 'item': 'minecraft:stone', 'group': 'control', 'count': 1, 'slot': 23, 'key': 'case14'},
 {'label': 'named',
  'item': 'minecraft:stone[minecraft:custom_name="H283 name",minecraft:custom_data={keep:283}]',
  'group': 'control',
  'count': 3,
  'slot': 24,
  'key': 'case15'},
 {'label': 'default trim',
  'item': 'minecraft:gold_ingot',
  'group': 'control',
  'count': 1,
  'slot': 25,
  'key': 'case16'},
 {'label': 'default banner',
  'item': 'minecraft:flower_banner_pattern',
  'group': 'control',
  'count': 1,
  'slot': 26,
  'key': 'case17'},
 {'label': 'empty break',
  'item': 'minecraft:diamond_pickaxe[minecraft:can_break={}]',
  'group': 'control',
  'count': 1,
  'slot': 27,
  'key': 'case18'},
 {'label': 'empty place',
  'item': 'minecraft:stone[minecraft:can_place_on={}]',
  'group': 'control',
  'count': 1,
  'slot': 28,
  'key': 'case19'},
 {'label': 'remove trim',
  'item': 'minecraft:gold_ingot[!minecraft:provides_trim_material]',
  'group': 'removal',
  'count': 1,
  'slot': 29,
  'key': 'case20'},
 {'label': 'remove banner',
  'item': 'minecraft:flower_banner_pattern[!minecraft:provides_banner_patterns]',
  'group': 'removal',
  'count': 1,
  'slot': 30,
  'key': 'case21'},
 {'label': 'remove break',
  'item': 'minecraft:diamond_pickaxe[!minecraft:can_break]',
  'group': 'removal',
  'count': 1,
  'slot': 31,
  'key': 'case22'},
 {'label': 'remove place',
  'item': 'minecraft:stone[!minecraft:can_place_on]',
  'group': 'removal',
  'count': 1,
  'slot': 32,
  'key': 'case23'}]


def packet_rows(path):
    out=[]
    if path.exists():
        for line in path.read_text().splitlines():
            if line.startswith(PLAYER+' step=') and ' component-wire ' in line:
                prefix,body=line.split(' component-wire ',1);out.append((prefix.split(' step=',1)[1],json.loads(body)))
    return out


def actual_slot(rows,stage,slot):
    found=[]
    for step,p in rows:
        if step!=stage:continue
        assert p['remaining']==0, p
        if (p['kind']=='slot' and p['window']==0 and p['slot']==slot) or (p['kind']=='inventory' and p['slot']==slot):found.extend(p['stacks'])
        elif p['kind']=='content' and p['window']==0:found.append(p['stacks'][slot])
    return found[-1] if found else None


def tag(value):
    raw=bytes.fromhex(value['nbt']);f=io.BytesIO(raw[1:]);out=nbt.read_tag(f,raw[0]);assert not f.read(),'trailing typed NBT';return out


def run(label,command,client_command,build,world):
    directory=build/label;port=net.server_dir(directory,world)
    server=net.Server(command,directory,label);commands=Commands(server,build/(label+'-commands.json'))
    wire=build/(label+'-wire.txt');log=(build/(label+'-client.log')).open('w');listen=net.free_port()
    capture=build/(label+'-packets.jsonl');stop=client=None
    try:
        server.wait_for(r'Done \(');commands.reply('tick freeze')
        stop=net_capture.start_background(listen,f'127.0.0.1:{port}',capture,full=True)
        client=net.Client(client_command,listen,wire,log)
        for line in ['menus on','components on','chat on','join '+PLAYER]:client.tell(line)
        server.wait_for('Bob joined the game',timeout=120);client.sync([PLAYER])
        def cmd(text):return commands.reply(text)
        cmd('gamemode creative Bob');cmd('tp Bob 4.5 100 4.5 0 0');cmd('clear Bob')
        def guard(name,condition):
            cmd(f'data modify storage h283:proof guards.{name} set value {{result:91,success:17b}}')
            cmd(f'execute store result storage h283:proof guards.{name}.result int 1 store success storage h283:proof guards.{name}.success byte 1 run execute {condition}')
        words=','.join(map(str,uuid_words(PLAYER)))
        guard('identity',f'if entity @a[name=Bob,type=player,nbt={{UUID:[I;{words}],Health:20f,Pos:[4.5d,100d,4.5d],playerGameType:1}}]')
        guard('loaded','if loaded 4 100 4');guard('floor','if block 4 99 4 stone')
        def snapshot(name):
            cmd(f'data modify storage h283:proof {name}.player set from entity Bob')
            cmd(f'data modify storage h283:proof {name}.clock set value 0L')
            cmd(f'execute store result storage h283:proof {name}.clock long 1 run time query gametime')
            client.sync([PLAYER]);dump(client,name)
        snapshot('initial')
        for c in CASES:
            key=c['key'];client.tell('step '+key);client.sync([PLAYER])
            cmd(f'data modify storage h283:proof {key}.action set value {{result:91,success:17b}}')
            client.tell(f'send-cmd Bob execute store result storage h283:proof {key}.action.result int 1 store success storage h283:proof {key}.action.success byte 1 run item replace entity @s inventory.{c["slot"]-9} with {c["item"]} {c["count"]}')
            client.sync([PLAYER]);snapshot(key)
            deadline=time.monotonic()+10
            while actual_slot(packet_rows(wire),key,c['slot']) is None:
                assert time.monotonic()<deadline,('no actual slot packet',c)
                time.sleep(.05);client.sync([PLAYER]);dump(client,key+'-receipt')
        client.tell('step cleanup');cmd('clear Bob');cmd('tick unfreeze');cmd('save-all flush');client.end()
    finally:
        if client:client.kill()
        if stop:stop.set()
        server.stop();log.close()
    raw={str(p.relative_to(directory)):nbt.read_dat(p) for p in (directory/'world').rglob('*command_storage*.dat')}
    (build/(label+'-storage.json')).write_text(json.dumps(raw,indent=2)+'\n')
    return facts(build,label)


def facts(build,label):
    raw=json.loads((build/(label+'-storage.json')).read_text());proof=next(d['data']['contents']['proof'] for d in raw.values() if 'proof' in d['data']['contents'])
    rows=packet_rows(build/(label+'-wire.txt'));result={};diagnostics={};start=proof['initial']['clock'][1]
    for name,v in proof['guards'].items():result['guard/'+name]=v
    result['initial/Inventory']=proof['initial']['player']['Inventory']
    for c in CASES:
        key=c['key'];v=proof[key];player=v['player'];slot=actual_slot(rows,key,c['slot'])
        fields={key+'/action':v['action'],key+'/clock':[v['clock'][0],v['clock'][1]-start]}
        for f in ['UUID','Health','Pos','playerGameType']:fields[key+'/'+f]=player.get(f)
        item=next((x for x in player['Inventory'] if x['Slot']==['b',c['slot']]),None)
        fields[key+'/saved-item-id']=item['id'] if item else None;fields[key+'/saved-count']=item['count'] if item else None
        fields[key+'/actual-slot-received']=slot is not None
        if slot:
            fields[key+'/packet-item']=slot['item'];fields[key+'/packet-count']=slot['count']
        result.update(fields)
        packet={}
        if slot:
            packet={'item':slot['item'],'count':slot['count'],'removed':slot['removed'],'effective':{n:tag(t) for n,t in slot['effective'].items()},'added':[{k:v for k,v in a.items() if k!='value'}|{'value':tag(a['value'])} for a in slot['added']]}
        if c['group']=='primary':
            result[key+'/saved-components']=item.get('components',{}) if item else None
            result[key+'/packet-components']=packet
        else:
            diagnostics[key]={'case':c,'actual_saved_stack':item,'actual_packet':packet,'whole_player':player}
    (build/(label+'-diagnostics.json')).write_text(json.dumps(diagnostics,indent=2)+'\n')
    return result


def admit(f,build,label):
    assert all(v=={'result':1,'success':['b',1]} for k,v in f.items() if k.startswith('guard/'))
    assert f['initial/Inventory']==[]
    for c in CASES:
        k=c['key'];assert f[k+'/action']=={'result':1,'success':['b',1]},(c,f[k+'/action'])
        assert f[k+'/clock']==['l',0],(c,f[k+'/clock'])
        assert f[k+'/UUID']==['I',uuid_words(PLAYER)] and f[k+'/Health']==['f',20.]
        assert f[k+'/Pos']==[['d',4.5],['d',100.],['d',4.5]] and f[k+'/playerGameType']==1
        assert f[k+'/saved-item-id']==c['item'].split('[',1)[0] and f[k+'/saved-count']==c['count'],c
        assert f[k+'/actual-slot-received'] and f[k+'/packet-item']==c['item'].split('[',1)[0] and f[k+'/packet-count']==c['count'],c
        if c['group']=='primary':
            expected={'case0':('minecraft:provides_trim_material','minecraft:gold'),'case1':('minecraft:provides_banner_patterns','#minecraft:pattern_item/globe'),'case2':('minecraft:can_place_on',{'blocks':'minecraft:dirt'}),'case3':('minecraft:can_break',{'blocks':'minecraft:stone','state':{}})}[k]
            name,value=expected;assert f[k+'/saved-components']=={name:value},(c,f[k+'/saved-components'])
            p=f[k+'/packet-components'];assert p['removed']==[] and len(p['added'])==1 and p['added'][0]['name']==name and p['added'][0]['value']==value and p['added'][0]['raw'],(c,p)
    # Historical exact/partial predicates are validated by actual full official codecs,
    # and retained as whole records. Their original wire Tag types are not inferred.
    diag=json.loads((build/(label+'-diagnostics.json')).read_text())
    partial=[(d['case']['label'],a.get('predicate_wire_tags')) for d in diag.values() for a in d.get('actual_packet',{}).get('added',[]) if a['name'] in ['minecraft:can_break','minecraft:can_place_on']]
    (build/(label+'-actual-predicate-wire-tags.json')).write_text(json.dumps(partial,indent=2)+'\n')
    return len(f)


def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True);p.add_argument('--build',default='h283-component-delivery');p.add_argument('--side',choices=['java','native'],required=True);p.add_argument('--strict-all',action='store_true');a=p.parse_args()
    try:
        lock,build,java,native,client=net.sides(a.build,a.binary)
    except subprocess.CalledProcessError as e:
        print(e.stdout.decode() if isinstance(e.stdout,bytes) else e.stdout)
        print(e.stderr.decode() if isinstance(e.stderr,bytes) else e.stderr)
        raise
    (build/'program.json').write_text(json.dumps({'cases':CASES,'primary_gate':'four canonical forms+actual action/identity/load/clock/saved/slot admissions','diagnostics':'all historical advanced/armortrim/removals whole records; full original payloads+order+IDs+epochs; no all-form/receive closure','observation_ticks':0},indent=2)+'\n')
    if a.side=='java':
        world=generate(java,build);f=run('java',java,client,build,world);n=admit(f,build,'java');(build/'reference.json').write_text(json.dumps(f,indent=2)+'\n');print('JAVA ADMITTED',n);return 0
    actual=run('native',native,client,build,build/'generate/world');ref=json.loads((build/'reference.json').read_text());diff={k:{'java':v,'native':actual.get(k)} for k,v in ref.items() if v!=actual.get(k)}
    (build/'native-facts.json').write_text(json.dumps(actual,indent=2)+'\n');(build/'diff.json').write_text(json.dumps(diff,indent=2)+'\n')
    print('EXACT',len(ref)-len(diff),'/',len(ref),'DIFF',json.dumps(diff,indent=2))
    if a.strict_all:
        j=json.loads((build/'java-diagnostics.json').read_text());n=json.loads((build/'native-diagnostics.json').read_text());extra={k:{'java':v,'native':n.get(k)} for k,v in j.items() if v!=n.get(k)};(build/'all-diagnostic-differences.json').write_text(json.dumps(extra,indent=2)+'\n');return bool(diff or extra)
    return bool(diff)


if __name__=='__main__':raise SystemExit(main())
