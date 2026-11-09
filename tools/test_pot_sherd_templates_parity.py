"""Decorated-pot template representation through actual command load/save."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import uuid
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

FRONT = 'minecraft:angler_pottery_sherd'
RIGHT = 'minecraft:archer_pottery_sherd'
ITEM_NAME = 'H290 item'
SHERD_NAME = 'H290 sherd'
CASES = [
    ('string', '"'+FRONT+'"', False),
    ('compound', '{id:"'+FRONT+'"}', False),
    ('defaults', '{id:"'+FRONT+'",count:1,components:{}}', False),
    ('components', '{id:"'+FRONT+'",count:3,components:{"minecraft:custom_data":{h290:1},"minecraft:custom_name":"'+SHERD_NAME+'"}}', True),
]


def program():
    ops = []
    def cmd(text, label, expected=None, admission=True, diagnostic=None, snapshot=None):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if expected is not None: op['expect_result'] = expected
        if diagnostic: op['diagnostic'] = diagnostic
        if snapshot: op.update(source_snapshot=snapshot[0], snapshot_phase=snapshot[1])
        ops.append(op)
    cmd('/tick freeze', 'freeze', 1)
    cmd('/gamerule spawn_mobs false', 'no-spawning')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0,0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 2.5 200 2.5 0 0', 'owner-position', 1)
    cmd('/gamemode creative @s', 'owner-mode')
    identity = uuid.UUID(bytes=hashlib.md5(b'OfflinePlayer:Player').digest(), version=3)
    parts = [int.from_bytes(identity.bytes[i:i+4], 'big', signed=True) for i in range(0,16,4)]
    saved_uuid = '[I;' + ','.join(map(str, parts)) + ']'
    owner = f'@s[type=player,name=Player,nbt={{UUID:{saved_uuid},Health:20f,playerGameType:1,Pos:[2.5d,200d,2.5d]}}]'
    cmd('/execute if entity '+owner, 'owner-body', 1)
    cmd('/scoreboard objectives add h290 dummy', 'objective', 1)
    cmd('/execute store result score before h290 run time query gametime', 'clock-start', diagnostic='absolute epoch; relative zero strict')
    for index,(name,front,rich) in enumerate(CASES):
        x = 4 + index*2; pos=f'{x} 200 4'
        item = '{id:"minecraft:wheat",count:3,components:{"minecraft:custom_name":"'+ITEM_NAME+'"}}'
        nbt = '{sherds:{front:'+front+',back:{id:"minecraft:brick"},right:{id:"'+RIGHT+'"}},item:'+item+'}'
        cmd(f'/setblock {pos} decorated_pot[facing=north,cracked=false,waterlogged=false]{nbt} strict', name+'-load', 1)
        cmd(f'/execute if loaded {pos}', name+'-loaded', 1)
        cmd(f'/execute if block {x} 199 4 stone', name+'-floor', 1)
        cmd(f'/execute if block {pos} decorated_pot[facing=north,cracked=false,waterlogged=false]', name+'-block-state', 1)
        for key,value in [('id',len('minecraft:decorated_pot')),('x',x),('y',200),('z',4),('item.count',3),('item.components."minecraft:custom_name"',len(ITEM_NAME)),('sherds.back.id',len('minecraft:brick')),('sherds.right.id',len(RIGHT))]:
            cmd(f'/data get block {pos} {key}', name+'-'+key, value)
        cmd(f'/execute unless data block {pos} sherds.left', name+'-default-left-absent', 1)
        cmd(f'/execute unless data block {pos} LootTable', name+'-no-loot', 1)
        cmd(f'/data get block {pos}', name+'-whole-before', diagnostic='complete saved body/order', snapshot=(name,'before'))
        assigned = name=='string'
        cmd(f'/execute if data block {pos} sherds.front{{id:"{FRONT}"}}', name+'-front-compound', 1, admission=not assigned)
        cmd(f'/data get block {pos} sherds.front.id', name+'-front-id', len(FRONT), admission=not assigned)
        cmd('/scoreboard players set result h290 31', name+'-fresh-result', 31)
        cmd('/scoreboard players set success h290 17', name+'-fresh-success', 17)
        cmd(f'/execute store result score result h290 store success score success h290 run data get block {pos} sherds.front.id', name+'-stored-front-id', len(FRONT), admission=not assigned)
        cmd('/scoreboard players get result h290', name+'-result', len(FRONT), admission=not assigned)
        cmd('/scoreboard players get success h290', name+'-success', 1, admission=not assigned)
        if rich:
            cmd(f'/data get block {pos} sherds.front.count', name+'-count', 3)
            cmd(f'/data get block {pos} sherds.front.components."minecraft:custom_data".h290', name+'-component-data', 1)
            cmd(f'/data get block {pos} sherds.front.components."minecraft:custom_name"', name+'-component-name', len(SHERD_NAME))
        else:
            cmd(f'/execute unless data block {pos} sherds.front.count', name+'-count-default-omitted', 1)
            cmd(f'/execute unless data block {pos} sherds.front.components', name+'-components-default-omitted', 1)
        cmd(f'/data get block {pos} sherds.front', name+'-front-body', 3 if rich else 1,
            admission=not assigned, diagnostic='complete component-bearing template/order' if rich else None)
        cmd(f'/data get block {pos} sherds', name+'-whole-sherds', 3, diagnostic='complete sherd map/order')
        cmd(f'/data get block {pos}', name+'-whole-after', diagnostic='complete saved body/order', snapshot=(name,'after'))
    cmd('/execute if entity '+owner, 'owner-final', 1)
    cmd('/execute store result score after h290 run time query gametime', 'clock-end', diagnostic='absolute epoch; relative zero strict')
    cmd('/execute if score before h290 = after h290', 'relative-clock-zero', 1)
    cmd('/scoreboard objectives remove h290', 'cleanup', 0)
    cmd('/tick unfreeze', 'unfreeze', 0)
    return ops


def source_changes(ops, rows):
    snapshots = {}
    for op,row in zip(ops,rows):
        if 'source_snapshot' in op:
            snapshots.setdefault(op['source_snapshot'],{})[op['snapshot_phase']]=row
    assert len(snapshots)==len(CASES)
    return [name for name,pair in snapshots.items() if pair['before']!=pair['after']]


def main():
    parser=argparse.ArgumentParser(add_help=False);parser.add_argument('--reference-dir',type=Path)
    options,rest=parser.parse_known_args();sys.argv=[sys.argv[0],*rest]
    if '--work-dir' not in sys.argv:parser.error('--work-dir must name a fresh capture directory')
    folder=Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve();assert not folder.exists(),'preserve previous attempts'
    driver.corpus_module.groups=lambda:[('Pot sherd template representation',program())]
    if '--allow-gaps' not in sys.argv:sys.argv.append('--allow-gaps')
    driver.main()
    rows=lambda file:[json.loads(line) for line in file.read_text().splitlines()]
    ops=[o for o in program() if o['op']=='cmd' and o.get('record',True)]
    fresh=rows(folder/'vanilla.jsonl');assert not source_changes(ops,fresh),'Java read changed source'
    if '--reference-only' in sys.argv:return 0
    reference=options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java,native=rows(reference),rows(folder/'rust.jsonl');assert len(ops)==len(java)==len(fresh)==len(native)
    assert all(a==b for o,a,b in zip(ops,java,fresh) if not o.get('diagnostic')),'fresh strict reference differs'
    report=dict(strict=0,strict_failures=[],diagnostic_records=0,diagnostic_differences=[],source_changes=source_changes(ops,native),reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diagnostic=bool(op.get('diagnostic'));report['diagnostic_records' if diagnostic else 'strict']+=1
        if a!=b:report['diagnostic_differences' if diagnostic else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diagnostic:assert a==b,('admission',op,a,b)
    (folder/'sherd-template-report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
    return bool(report['strict_failures'] or report['source_changes'])


if __name__=='__main__':
    sys.exit(main())
