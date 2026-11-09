"""Required function-tag dependency validity through real pack/command routes."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import sys
import uuid

try:
    from . import test_command_exec_parity as driver
except ImportError:
    import test_command_exec_parity as driver

FUNCTIONS = {f'h145:{name}': f'scoreboard players add {name} h145 1\nscoreboard players operation order h145 *= ten h145\nscoreboard players add order h145 {digit}\n'
             for digit, name in enumerate(('a', 'b', 'c'), 1)}
TAGS = {
    'empty': [], 'ordered': ['h145:a', 'h145:b', 'h145:a'],
    'nested': ['#h145:empty', '#h145:ordered', 'h145:a'],
    'optional_member': ['h145:a', {'id': 'h145:missing', 'required': False}, 'h145:b'],
    'optional_child': ['h145:a', {'id': '#h145:missing', 'required': False}, 'h145:b'],
    'missing_first': ['h145:missing', 'h145:a'],
    'missing_last': ['h145:a', 'h145:missing'],
    'missing_child': ['h145:a', '#h145:missing'],
    'invalid_child': ['h145:c', 'h145:missing'],
    'required_invalid_child': ['h145:a', '#h145:invalid_child', 'h145:b'],
    'optional_invalid_child': ['h145:a', {'id': '#h145:invalid_child', 'required': False}, 'h145:b'],
}
CASES = [('h145:a', (1, 0, 0, 1)), ('h145:b', (0, 1, 0, 2)), ('h145:c', (0, 0, 1, 3))] + [
    ('#h145:' + name, (1, 1, 0, 12) if name in ('ordered', 'nested', 'optional_member', 'optional_child', 'optional_invalid_child')
     else (0, 0, 0, 0)) for name in TAGS]


def program():
    ops = []
    def command(text, label, expected=None, admission=True, diagnostic=None, record=True):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if expected is not None: op['expect_result'] = expected
        if diagnostic: op['diagnostic'] = diagnostic
        if not record: op['record'] = False
        ops.append(op)
    command('/tick freeze', 'freeze', 1)
    command('/gamerule spawn_mobs false', 'no-spawning')
    command('/forceload add 0 0', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    command('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    command('/tp @s 4.5 200 4.5 0 0', 'position', 1)
    command('/gamemode creative @s', 'mode')
    identity = uuid.UUID(bytes=hashlib.md5(b'OfflinePlayer:Player').digest(), version=3)
    parts = [int.from_bytes(identity.bytes[i:i+4], 'big', signed=True) for i in range(0, 16, 4)]
    saved_uuid = '[I;' + ','.join(map(str, parts)) + ']'
    guard = f'@s[type=player,name=Player,nbt={{UUID:{saved_uuid},Health:20f,playerGameType:1,Pos:[4.5d,200d,4.5d]}}]'
    command('/execute if entity ' + guard, 'owner', 1)
    command('/data get entity @s UUID', 'uuid', 4)
    command('/execute if loaded 4 200 4', 'loaded', 1)
    command('/execute if block 4 199 4 stone', 'support', 1)
    command('/scoreboard objectives add h145 dummy', 'objective', 1)
    command('/scoreboard players set ten h145 10', 'order-base', 10)
    command('/datapack list enabled', 'packs-initial', diagnostic='complete pack-list/order')
    command('/execute store result score before h145 run time query gametime', 'clock-start', diagnostic='absolute epoch; relative zero strict')
    command('/scoreboard players set reload_sentinel h145 73', 'reload-sentinel', 73)
    command('/reload', 'reload')
    command('/scoreboard players get reload_sentinel h145', 'reload-no-action', 73)
    command('/datapack list enabled', 'packs-reloaded', diagnostic='complete pack-list/order')
    for target, expected in CASES:
        label = target.replace('#', 'tag-')
        for key in ('a', 'b', 'c', 'order'):
            command(f'/scoreboard players set {key} h145 0', label+'-reset-'+key, 0, record=False)
        command('/scoreboard players set result h145 31', label+'-fresh-result', 31)
        command('/scoreboard players set success h145 17', label+'-fresh-success', 17)
        command(f'/execute store result score result h145 store success score success h145 run function {target}', label+'-call', admission=False)
        for key in ('result', 'success'):
            command(f'/scoreboard players get {key} h145', label+'-'+key, admission=False)
        positive = target in ('h145:a', 'h145:b', 'h145:c', '#h145:empty', '#h145:ordered', '#h145:nested', '#h145:optional_member', '#h145:optional_child')
        for key, value in zip(('a', 'b', 'c', 'order'), expected):
            command(f'/scoreboard players get {key} h145', label+'-after-'+key, value, admission=positive)
    command('/execute if entity ' + guard, 'owner-final', 1)
    command('/execute store result score after h145 run time query gametime', 'clock-end', diagnostic='absolute epoch; relative zero strict')
    command('/execute if score before h145 = after h145', 'relative-clock-zero', 1)
    command('/scoreboard objectives remove h145', 'cleanup', 0)
    command('/tick unfreeze', 'unfreeze', 0)
    return ops


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument('--reference-dir', type=Path)
    options, rest = parser.parse_known_args()
    sys.argv = [sys.argv[0], *rest]
    if '--work-dir' in sys.argv:
        folder = Path(sys.argv[sys.argv.index('--work-dir') + 1]).resolve()
        assert not folder.exists(), 'preserve every prior capture'
    else:
        # The parity runner passes no --work-dir: start from an empty scratch folder.
        folder = driver.lab.LOCAL / 'function-tags-parity'
        shutil.rmtree(folder, ignore_errors=True)
        sys.argv += ['--work-dir', str(folder)]
    original = driver.install_datapack_fixtures
    def install(world, *args, **kwargs):
        original(world, *args, **kwargs)
        for name, entries in TAGS.items():
            path = world/'datapacks/command-selector-fixture/data/h145/tags/function'/f'{name}.json'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps({'values': entries})+'\n')
    driver.install_datapack_fixtures = install
    driver.corpus_module.function_return_fixtures = lambda names: FUNCTIONS
    driver.corpus_module.groups = lambda: [('Function required dependency validity', program())]
    if '--allow-gaps' not in sys.argv: sys.argv.append('--allow-gaps')
    driver.main()
    if '--reference-only' in sys.argv: return 0
    rows = lambda path: [json.loads(s) for s in path.read_text().splitlines()]
    ops = [op for op in program() if op['op']=='cmd' and op.get('record', True)]
    fresh, native = rows(folder/'vanilla.jsonl'), rows(folder/'rust.jsonl')
    reference = options.reference_dir/'vanilla.jsonl' if options.reference_dir else folder/'vanilla.jsonl'
    java = rows(reference)
    assert len(ops)==len(java)==len(fresh)==len(native)
    assert all(a==b for op,a,b in zip(ops,java,fresh) if not op.get('diagnostic')), 'fresh strict reference differs'
    report = dict(strict=0, strict_failures=[], diagnostic_records=0, diagnostic_differences=[],
                  reference_sha256=hashlib.sha256(reference.read_bytes()).hexdigest())
    for i,(op,a,b) in enumerate(zip(ops,java,native)):
        diagnostic = bool(op.get('diagnostic'))
        report['diagnostic_records' if diagnostic else 'strict'] += 1
        if a!=b: report['diagnostic_differences' if diagnostic else 'strict_failures'].append(dict(index=i,label=op['label'],expected=a,actual=b))
        if op['admission'] and not diagnostic: assert a==b, ('admission',op,a,b)
    (folder/'function-tags-report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    return bool(report['strict_failures'])


if __name__ == '__main__':
    sys.exit(main())
