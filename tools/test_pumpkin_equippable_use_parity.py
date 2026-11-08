"""Connected UseItem/ACK for a head pumpkin's default-allowed Equippable override."""
import argparse
import hashlib
import json
from pathlib import Path
import sys
import tempfile
try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver


def program():
    ops = []
    def cmd(text, label, result=None, admission=True, diagnostic=None, **extra):
        row = dict(op='cmd', cmd='/' + text, label=label, admission=admission)
        if result is not None: row['expect_result'] = result
        if diagnostic: row['diagnostic'] = diagnostic
        row.update(extra)
        ops.append(row)
    cmd('tick freeze', 'freeze', 1)
    cmd('gamerule show_advancement_messages false', 'disable-advancement-chat', 0)
    cmd('gamerule show_advancement_messages', 'actual-advancement-chat-disabled', 0)
    cmd('gamemode creative', 'initial-mode')
    cmd('forceload add 0 0', 'load', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('tp @s 8.5 200 8.5 0 0', 'position', 1)
    cmd('data get entity @s UUID', 'actual-owner-uuid', 4)
    cmd('execute if loaded 8 200 8', 'actual-load', 1)
    cmd('execute if block 8 199 8 stone', 'actual-floor', 1)
    for i, obj in enumerate(['h207', 'h207r', 'h207s'], 1):
        cmd('scoreboard objectives add ' + obj + ' dummy', 'objective-' + obj, i)
    cmd('execute store result score #start h207 run time query gametime', 'epoch-start', diagnostic='absolute epoch')
    # Only the default-allowed slot/swappable shape is in the repair. No allowed_entities override.
    pumpkin = 'minecraft:carved_pumpkin'
    eq = 'minecraft:equippable={slot:"head"}'
    false_eq = 'minecraft:equippable={slot:"head",swappable:0b}'
    marker = 'minecraft:custom_data={h207:207}'
    helmet = 'minecraft:diamond_helmet[minecraft:custom_data={old:1}]'
    cases = [
        ('empty', 'survival', eq, 3, None, 2, 'pumpkin', True),
        ('swap', 'survival', eq, 1, helmet, 1, 'pumpkin', True),
        ('creative', 'creative', eq, 3, None, 3, 'pumpkin', True),
        ('prototype', 'survival', None, 3, None, 3, None, False),
        ('false', 'survival', false_eq, 3, None, 3, None, False),
        ('binding', 'survival', eq, 3, 'minecraft:diamond_helmet[minecraft:enchantments={"minecraft:binding_curse":1},minecraft:custom_data={old:1}]', 3, 'helmet', False),
        ('same', 'survival', eq, 3, pumpkin + '[' + eq + ',' + marker + ']', 3, 'pumpkin', False),
    ]
    for seq, (label, mode, override, count, old, after_count, head, changed) in enumerate(cases, 101):
        prefix = label + '-'
        # Cleanup is setup, never the item-use proof. A positive empty inventory check follows.
        ops.append(dict(op='cmd', cmd='/clear @s', record=False))
        ops.append(dict(op='cmd', cmd='/item replace entity @s armor.head with air', record=False))
        cmd('execute unless data entity @s Inventory[0]', prefix+'empty-inventory', 1)
        cmd('execute unless data entity @s equipment.head', prefix+'empty-head', 1)
        cmd('gamemode ' + mode, prefix+'mode')
        cmd('execute if entity @s[type=player,nbt={Health:20f,Pos:[8.5d,200d,8.5d],Rotation:[0f,0f],playerGameType:'+('1' if mode=='creative' else '0')+'}]', prefix+'actual-player', 1)
        held = pumpkin + '[' + ','.join(v for v in [override, marker] if v) + ']'
        cmd('item replace entity @s weapon.mainhand with ' + held + ' ' + str(count), prefix+'held-setup', 1)
        if old: cmd('item replace entity @s armor.head with ' + old, prefix+'head-setup', 1)
        base = 'Inventory[{Slot:0b}]'
        def stack_fields(path, item, n, name, admission=True):
            cmd('data get entity @s ' + path + '.id', name+'-id', len(item), admission)
            cmd('data get entity @s ' + path + '.count', name+'-count', n, admission)
            cmd('data get entity @s ' + path + '.components', name+'-components', admission=admission)
        stack_fields(base, pumpkin, count, prefix+'before-hand')
        cmd('execute if entity @s[nbt={Inventory:[{Slot:0b,id:"minecraft:carved_pumpkin",count:'+str(count)+',components:{"minecraft:custom_data":{h207:207}'+(',"minecraft:equippable":{slot:"head"'+(',swappable:0b' if label=='false' else '')+'}' if override else '')+'}}]}]', prefix+'typed-actual-held', 1)
        if old: stack_fields('equipment.head', pumpkin if label=='same' else 'minecraft:diamond_helmet', 1, prefix+'before-head')
        cmd('data get entity @s', prefix+'full-before', diagnostic='complete player schema/body/order/clock')
        ops.append(dict(op='use_item', sequence=seq, yaw=0, pitch=0))
        # Successful execution of this op requires the actual connection ACK on both engines.
        after_item = 'minecraft:diamond_helmet' if label=='swap' else pumpkin
        stack_fields(base, after_item, after_count, prefix+'after-hand', not changed)
        if head:
            stack_fields('equipment.head', pumpkin if head=='pumpkin' else 'minecraft:diamond_helmet', 1, prefix+'after-head', not changed)
        else:
            cmd('execute unless data entity @s equipment.head', prefix+'still-empty-head', 1)
        hand_nbt = '{Slot:0b,id:"'+after_item+'",count:'+str(after_count)+'}'
        predicate = 'execute if entity @s[nbt={Inventory:['+hand_nbt+']'+(',equipment:{head:{id:"'+(pumpkin if head=='pumpkin' else 'minecraft:diamond_helmet')+'",count:1}}' if head else '')+'}]'
        cmd('scoreboard players set #r h207r 91', prefix+'fresh-result', 91)
        cmd('scoreboard players set #s h207s 17', prefix+'fresh-success', 17)
        cmd('execute store result score #r h207r store success score #s h207s run '+predicate, prefix+'actual-state-branch', 1, not changed)
        cmd('scoreboard players get #r h207r', prefix+'result', 1, not changed)
        cmd('scoreboard players get #s h207s', prefix+'success', 1, not changed)
        cmd('execute unless data entity @s Inventory[{Slot:1b}]', prefix+'no-extra-inventory', 1)
        cmd('execute unless entity @e[type=item]', prefix+'no-dropped-piece', 1)
        cmd('data get entity @s Inventory', prefix+'full-inventory', diagnostic='complete inventory/body/order')
        cmd('data get entity @s equipment', prefix+'full-equipment', diagnostic='complete equipment/body/order')
        cmd('data get entity @s', prefix+'full-after', diagnostic='complete player schema/body/order/clock')
    # Component-removal bridge is independently observable and never folded into the primary gate.
    ops.append(dict(op='cmd', cmd='/clear @s', record=False))
    ops.append(dict(op='cmd', cmd='/item replace entity @s armor.head with air', record=False))
    cmd('gamemode survival', 'removal-mode', diagnostic='component-removal prerequisite')
    cmd('item replace entity @s weapon.mainhand with minecraft:carved_pumpkin[!minecraft:equippable,minecraft:custom_data={h207:207}] 3', 'removal-setup', 1, diagnostic='component-removal prerequisite')
    cmd('data get entity @s Inventory', 'removal-before', diagnostic='component-removal prerequisite/full inventory')
    cmd('data get entity @s Inventory[{Slot:0b}].components', 'removal-components', diagnostic='component-removal prerequisite')
    ops.append(dict(op='use_item', sequence=108, yaw=0, pitch=0))
    cmd('data get entity @s Inventory', 'removal-after', diagnostic='component-removal prerequisite/full inventory')
    cmd('execute unless data entity @s equipment.head', 'removal-no-equip', 1, diagnostic='component-removal prerequisite')
    cmd('execute store result score #now h207 run time query gametime', 'epoch-end', diagnostic='absolute epoch')
    cmd('scoreboard players operation #now h207 -= #start h207', 'relative-clock-op', 0)
    cmd('scoreboard players get #now h207', 'relative-clock', 0)
    ops.append(dict(op='cmd', cmd='/clear @s', record=False))
    ops.append(dict(op='cmd', cmd='/item replace entity @s armor.head with air', record=False))
    cmd('forceload remove 0 0', 'cleanup-load', 1)
    for i, obj in enumerate(['h207','h207r','h207s']): cmd('scoreboard objectives remove '+obj, 'cleanup-'+obj, 2-i)
    cmd('tick unfreeze', 'cleanup-unfreeze', 0)
    return ops


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir', type=Path)
    options, rest = parser.parse_known_args()
    sys.argv = [sys.argv[0]] + rest
    if '--help' in rest or '-h' in rest:
        driver.main()
        return 0
    if '--work-dir' not in rest:
        # The automatic parity runner invokes harnesses without arguments.
        rest += ['--work-dir', str(Path(tempfile.mkdtemp(prefix='horizon-pumpkin-use-')) / 'capture')]
        sys.argv = [sys.argv[0]] + rest
    folder = Path(rest[rest.index('--work-dir') + 1]).resolve()
    assert not folder.exists(), 'preserve previous attempts'
    driver.corpus_module.groups = lambda: [('Pumpkin Equippable UseItem', program())]
    if '--allow-gaps' not in sys.argv: sys.argv.append('--allow-gaps')
    driver.main()
    rows = lambda path: [json.loads(line) for line in path.read_text().splitlines()]
    ops = [op for op in program() if op['op']=='cmd' and op.get('record', True)]
    fresh = rows(folder/'vanilla.jsonl')
    assert len(fresh)==len(ops)
    if '--reference-only' in sys.argv: return 0
    reference = options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java, native = rows(reference), rows(folder/'rust.jsonl')
    assert len(java)==len(native)==len(ops)
    assert all(a==b for op,a,b in zip(ops,java,fresh) if not op.get('diagnostic')), 'fresh Java strict records changed'
    report = dict(strict=0, admissions=0, strict_failures=[], diagnostic_records=0, diagnostic_differences=[], reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diag = bool(op.get('diagnostic'))
        report['diagnostic_records' if diag else 'strict'] += 1
        if a!=b: report['diagnostic_differences' if diag else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diag:
            report['admissions'] += 1
            assert a==b, ('actual admission/control',op,a,b)
    (folder/'pumpkin-equippable-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])


if __name__ == '__main__': sys.exit(main())
