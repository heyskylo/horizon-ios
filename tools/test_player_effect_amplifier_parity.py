#!/usr/bin/env python3
"""Player saved amplifier default through real effect commands and saved-entity reads.

All declared scalar/callback/guard records compare without masks. Complete effect
and player compounds plus absolute clocks are explicitly raw trace diagnostics.
"""
import hashlib
import json
from pathlib import Path
import sys

import test_command_exec_parity as driver


def program():
    ops = []
    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = dict(op='cmd', cmd=text, label=label, admission=admission)
        if result is not None:
            op['expect_result'] = result
        if diagnostic:
            op.update(record=False, trace=True)
        ops.append(op)
    cmd('/tick freeze', 'freeze')
    cmd('/gamerule spawn_mobs false', 'no-spawn')
    cmd('/gamerule advance_time false', 'no-daylight')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append(dict(op='settle', required_chunks=[[0, 0]]))
    cmd('/fill 0 199 0 15 199 15 stone', 'floor', 256)
    cmd('/tp @s 8.5 200 8.5 0 0', 'position', 1)
    cmd('/gamemode creative @s', 'mode')
    raw = bytearray(hashlib.md5(b'OfflinePlayer:Player').digest())
    raw[6] = (raw[6] & 15) | 48; raw[8] = (raw[8] & 63) | 128
    uuid = ','.join(str(int.from_bytes(raw[i:i+4], 'big', signed=True)) for i in range(0, 16, 4))
    body = f'@s[type=player,name=Player,nbt={{UUID:[I;{uuid}],Health:20f,playerGameType:1,Pos:[8.5d,200d,8.5d]}}]'
    for condition, label in [('loaded 8 200 8', 'loaded'), ('block 8 199 8 stone', 'support'),
                             ('block 8 200 8 air', 'feet-air'), ('block 8 201 8 air', 'eye-air'),
                             ('entity '+body, 'actor')]:
        cmd('/execute if '+condition, label, 1)
    for field, result in [('UUID',4), ('Health',20), ('Pos',3), ('playerGameType',1)]:
        cmd('/data get entity @s '+field, 'actor-'+field, result)
    cmd('/execute unless data entity @s active_effects', 'initial-empty', 1)
    cmd('/data get entity @s', 'initial-body', diagnostic=True)
    cmd('/scoreboard objectives add h100 dummy', 'objective')
    cmd('/execute store result score start h100 run time query gametime', 'clock-start', diagnostic=True)
    path = 'active_effects[{id:"minecraft:luck"}]'
    for amp in [0, 1, 255]:
        prefix = f'amp{amp}'
        cmd('/execute unless data entity @s active_effects', prefix+'-reset-empty', 1)
        cmd(f'/effect give @s minecraft:luck infinite {amp} true', prefix+'-give', 1)
        cmd('/execute if entity '+body, prefix+'-actor', 1)
        cmd('/data get entity @s active_effects', prefix+'-full-effects', diagnostic=True)
        cmd('/data get entity @s', prefix+'-full-body', diagnostic=True)
        cmd('/execute if data entity @s active_effects[0]', prefix+'-one-effect', 1)
        cmd('/execute unless data entity @s active_effects[1]', prefix+'-no-second-effect', 1)
        cmd('/data get entity @s '+path+'.id', prefix+'-id', 14)
        cmd('/data get entity @s '+path+'.duration', prefix+'-duration', -1)
        cmd('/execute if data entity @s {active_effects:[{id:"minecraft:luck",duration:-1,show_particles:0b,show_icon:0b}]}', prefix+'-typed-flags', 1)
        # ambient=false is its effective default; its separate saved omission
        # is diagnostic, never a reason to broaden this amplifier repair.
        cmd('/execute unless data entity @s {active_effects:[{id:"minecraft:luck",ambient:1b}]}', prefix+'-not-ambient', 1)
        for holder in ['result','success']:
            cmd(f'/scoreboard players set {holder} h100 -99', prefix+'-reset-'+holder, -99)
        cmd('/execute store result score result h100 store success score success h100 run data get entity @s '+path+'.amplifier', prefix+'-amplifier-read', admission=False)
        for holder in ['result','success']:
            cmd(f'/scoreboard players get {holder} h100', prefix+'-'+holder, admission=False)
        if amp == 0:
            cmd('/execute unless data entity @s '+path+'.amplifier', prefix+'-omitted', admission=False)
        else:
            byte = amp if amp<128 else amp-256
            cmd(f'/execute if data entity @s {{active_effects:[{{id:"minecraft:luck",amplifier:{byte}b}}]}}', prefix+'-typed-byte', 1)
        # Accepted removal witnesses a real live effect independently of the
        # saved amplifier field whose absence is under test.
        cmd('/effect clear @s minecraft:luck', prefix+'-live-clear', 1)
        cmd('/execute unless data entity @s active_effects', prefix+'-cleared-empty', 1)
    cmd('/execute store result score end h100 run time query gametime', 'clock-end', diagnostic=True)
    cmd('/scoreboard players operation end h100 -= start h100', 'clock-delta-operation', 0)
    cmd('/scoreboard players get end h100', 'clock-delta', 0)
    return ops


def main():
    reference = None
    if '--reference' in sys.argv:
        i = sys.argv.index('--reference'); reference = Path(sys.argv[i+1]).resolve()
        del sys.argv[i:i+2]
    if '--work-dir' not in sys.argv:
        # tools/parity.py runs every harness with no arguments: a fresh folder under .local.
        import shutil
        default = Path(__file__).resolve().parents[1] / '.local' / 'player-effect-amplifier-parity'
        shutil.rmtree(default, ignore_errors=True)
        sys.argv += ['--work-dir', str(default)]
    build = Path(sys.argv[sys.argv.index('--work-dir')+1]).resolve()
    assert not build.exists(), 'preserve earlier attempts; use a fresh directory'
    build.mkdir(parents=True)
    ops = program(); recorded = [op for op in ops if op['op']=='cmd' and op.get('record', True)]
    declaration = dict(script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                       records=len(recorded), labels=[op['label'] for op in recorded],
                       raw_diagnostics=[op['label'] for op in ops if op.get('trace')],
                       scope='Complete declared command records strict, no masks/tolerances. Full effect/body and absolute clock traces retained separately; no other default-flag/whole-body/raw-clock equality claim.')
    (build/'declaration.json').write_text(json.dumps(declaration, indent=2)+'\n')
    expected_bytes = reference.read_bytes() if reference else None
    driver.corpus_module.groups = lambda: [('player saved effect amplifier', ops)]
    sys.argv.append('--allow-gaps'); driver.main()
    java_bytes = (build/'vanilla.jsonl').read_bytes()
    if expected_bytes is not None:
        assert java_bytes == expected_bytes, 'fresh Java reference changed'
        assert reference.read_bytes() == expected_bytes
    java = [json.loads(s) for s in java_bytes.splitlines()]
    if '--reference-only' in sys.argv:
        return
    native = [json.loads(s) for s in (build/'rust.jsonl').read_bytes().splitlines()]
    assert len(java)==len(native)==len(recorded)
    residuals = [dict(label=op['label'], expected=a, actual=b) for op,a,b in zip(recorded,java,native) if a!=b]
    report = dict(records=len(recorded), passed=len(recorded)-len(residuals), residuals=residuals,
                  reference_sha256=hashlib.sha256(java_bytes).hexdigest())
    (build/'strict-report.json').write_text(json.dumps(report, indent=2)+'\n')
    for op,a,b in zip(recorded,java,native):
        if op['admission']:
            assert a==b, ('admission',op,a,b)
    print(f'Strict records: {report["passed"]}/{report["records"]}')
    if residuals:
        raise SystemExit(1)


if __name__=='__main__':
    main()
