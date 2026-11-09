"""H212: actual named crafter destruction; ordinary vanilla table only."""
import hashlib
import uuid
import argparse
import json
from pathlib import Path
import sys
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver



def program():
    ops = []
    def cmd(command, label, result=None, admission=True, diagnostic=None):
        op = dict(op='cmd', cmd=command, label=label, admission=admission)
        if result is not None:
            op['expect_result'] = result
        if diagnostic:
            op['diagnostic'] = diagnostic
        ops.append(op)
    cmd('/tick freeze', 'freeze', 1)
    cmd('/gamerule spawn_mobs false', 'no-mobs')
    cmd('/gamerule random_tick_speed 0', 'no-random-ticks')
    cmd('/gamerule block_drops true', 'block-drops')
    cmd('/gamemode spectator', 'owner-mode')
    cmd('/forceload add 0 0', 'force-load', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 7.5 204 7.5 0 0', 'owner-position', 1)
    ident = uuid.UUID(bytes=hashlib.md5(b'OfflinePlayer:Player').digest(), version=3)
    parts = [int.from_bytes(ident.bytes[i:i+4], 'big', signed=True) for i in range(0, 16, 4)]
    uid = '[I;' + ','.join(map(str, parts)) + ']'
    owner = f'@s[type=player,name=Player,nbt={{UUID:{uid},Health:20f,Pos:[7.5d,204d,7.5d],playerGameType:3}}]'
    cmd('/execute if entity '+owner, 'owner-identity', 1)
    cmd('/scoreboard objectives add h212 dummy', 'objective', 1)
    cmd('/execute store result score start h212 run time query gametime', 'absolute-start', diagnostic='absolute startup epoch')
    for case, x, z, block, name in [('named',3,3,'crafter','Rig'), ('plain',11,3,'crafter',None), ('chest',3,11,'chest','Box'), ('shulker',11,11,'shulker_box','Box')]:
        pos = f'{x} 200 {z}'
        fields = 'Items:[{Slot:0b,id:"minecraft:diamond",count:3,components:{"minecraft:custom_data":{h212:1}}}]'
        if name is not None:
            fields += f',CustomName:"{name}"'
        cmd(f'/setblock {pos} minecraft:{block}'+'{'+fields+'} strict',case+'-create',1)
        cmd(f'/execute if loaded {pos}',case+'-loaded',1)
        cmd(f'/execute if block {x} 199 {z} stone',case+'-floor',1)
        cmd(f'/execute if block {pos} minecraft:{block}',case+'-block',1)
        cmd(f'/execute if data block {pos} {{id:"minecraft:{block}",x:{x},y:200,z:{z}}}',case+'-source-identity',1)
        if name is None:
            cmd(f'/execute unless data block {pos} CustomName',case+'-source-unnamed',1)
        else:
            cmd(f'/data get block {pos} CustomName',case+'-source-name',len(name))
            cmd(f'/execute if data block {pos} {{CustomName:"{name}"}}',case+'-source-name-value',1)
        cmd(f'/data get block {pos} Items[{{Slot:0b}}].count',case+'-source-content-count',3)
        cmd(f'/data get block {pos} Items[{{Slot:0b}}].components."minecraft:custom_data".h212',case+'-source-component',1)
        cmd(f'/data get block {pos}',case+'-source-body',diagnostic='complete actual source block entity')
        cmd('/scoreboard players set result h212 31',case+'-fresh-break-result',31)
        cmd('/scoreboard players set success h212 17',case+'-fresh-break-success',17)
        cmd(f'/execute store result score result h212 store success score success h212 run setblock {pos} air destroy',case+'-destroy',1)
        cmd('/scoreboard players get result h212',case+'-break-result',1)
        cmd('/scoreboard players get success h212',case+'-break-success',1)
        cmd(f'/execute if block {pos} air',case+'-source-removed',1)
        area=f'@e[type=minecraft:item,x={x},y=199,z={z},dx=1,dy=2,dz=1'
        subject=area+f',nbt={{Item:{{id:"minecraft:{block}"}}}}]'
        one=subject[:-1]+',limit=1]'
        cmd('/execute if entity '+subject,case+'-one-block-drop',1)
        cmd('/data get entity '+one+' Item.id',case+'-drop-id',len('minecraft:'+block))
        cmd('/data get entity '+one+' Item.count',case+'-drop-count',1)
        cmd('/data get entity '+one+' Age',case+'-drop-age',0)
        cmd('/data get entity '+one+' PickupDelay',case+'-drop-delay',10)
        named = block!='crafter'
        assigned=case=='named'
        cmd(f'/execute {"if" if named else "unless"} data entity {one} Item.components."minecraft:custom_name"',case+'-name-policy',1,admission=not assigned)
        cmd('/scoreboard players set result h212 31',case+'-fresh-name-result',31)
        cmd('/scoreboard players set success h212 17',case+'-fresh-name-success',17)
        cmd(f'/execute store result score result h212 store success score success h212 run data get entity {one} Item.components."minecraft:custom_name"',case+'-name-query',len(name) if named else None,admission=not assigned)
        if not named:
            ops[-1]['expect_callback'] = [False, 0]
        cmd('/scoreboard players get result h212',case+'-name-result',len(name) if named else 0,admission=not assigned)
        cmd('/scoreboard players get success h212',case+'-name-success',int(named),admission=not assigned)
        diamond=area+',nbt={Item:{id:"minecraft:diamond"}}]'
        if block=='shulker_box':
            cmd('/execute unless entity '+diamond,case+'-no-spilled-content',1)
            cmd('/data get entity '+one+' Item.components."minecraft:container"[{slot:0}].item.count',case+'-kept-content-count',3)
            cmd('/data get entity '+one+' Item.components."minecraft:container"[{slot:0}].item.components."minecraft:custom_data".h212',case+'-kept-content-component',1)
        else:
            cmd('/execute if entity '+diamond,case+'-one-content-drop',1)
            content=diamond[:-1]+',limit=1]'
            cmd('/data get entity '+content+' Item.count',case+'-spilled-count',3)
            cmd('/data get entity '+content+' Item.components."minecraft:custom_data".h212',case+'-spilled-component',1)
        for path in ['Item','UUID','Pos','Motion','']:
            cmd('/data get entity '+one+(' '+path if path else ''),case+'-raw-'+(path or 'entity'),diagnostic='full item/random identity/kinematics/body order; no trajectory equality')
        cmd('/execute store result score now h212 run time query gametime',case+'-absolute-clock',diagnostic='absolute epoch')
        cmd('/execute if score now h212 = start h212',case+'-relative-zero',1)
    cmd('/execute if entity '+owner,'owner-final',1)
    cmd('/forceload remove 0 0','cleanup-load',1)
    cmd('/scoreboard objectives remove h212','cleanup-objective')
    cmd('/tick unfreeze','cleanup-unfreeze',0)
    return ops


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir', type=Path)
    opts, rest = parser.parse_known_args()
    sys.argv = [sys.argv[0], *rest]
    if '--work-dir' not in sys.argv:
        parser.error('--work-dir must be a fresh capture directory')
    folder = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not folder.exists(), 'preserve all previous capture attempts'
    driver.corpus_module.groups = lambda: [('Crafter ordinary drop name policy', program())]
    if '--allow-gaps' not in sys.argv:
        sys.argv.append('--allow-gaps')
    driver.main()
    rows = lambda f: [json.loads(line) for line in f.read_text().splitlines()]
    ops = [o for o in program() if o['op']=='cmd' and o.get('record', True)]
    fresh = rows(folder/'vanilla.jsonl')
    assert len(fresh)==len(ops)
    for op,row in zip(ops,fresh):
        if 'expect_callback' in op:
            assert row['results']==[op['expect_callback']], (op,row)
    if '--reference-only' in sys.argv:
        return 0
    ref = opts.reference_dir/'vanilla.jsonl' if opts.reference_dir else folder/'vanilla.jsonl'
    java, native = rows(ref), rows(folder/'rust.jsonl')
    assert len(java)==len(native)==len(ops)
    assert all(a==b for o,a,b in zip(ops,java,fresh) if not o.get('diagnostic')), 'fresh strict Java changed'
    report = dict(strict=0, admissions=0, strict_failures=[], diagnostic_records=0, diagnostic_differences=[], reference_sha256=hashlib.sha256(ref.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diagnostic = bool(op.get('diagnostic'))
        report['diagnostic_records' if diagnostic else 'strict'] += 1
        if a!=b:
            report['diagnostic_differences' if diagnostic else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diagnostic:
            report['admissions'] += 1
            assert a==b, ('positive admission',op,a,b)
    (folder/'crafter-name-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])


if __name__=='__main__':
    sys.exit(main())
