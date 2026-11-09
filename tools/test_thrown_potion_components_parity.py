#!/usr/bin/env python3
"""Isolated pinned potion methods versus actual in-memory Game paths.

No MinecraftServer constructor/main/spin, world bootstrap, game executable,
EULA, networking, display or device access. Invoke each phase only with a build
lease. Baseline source replacement is local and restored byte-for-byte finally.
"""
import argparse
from contextlib import contextmanager
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as artifacts
except ImportError:
    import lab
    import test_rust_parity as artifacts

BASE = '9d11d0c710705ee0901c2186d78641048a6660b0'
TEST = 'game::potions::contents_fixture::actual_thrown_contents_match_pinned_methods'
ORACLE = 'VanillaThrownPotionContentsOracle'
PRODUCTION = [
    'crates/horizon-client-render/src/brewing.rs',
    'crates/horizon-client-render/src/effects.rs',
    'crates/horizon-client/src/arrows.rs',
    'crates/horizon-client/src/game/potions.rs',
    'crates/horizon-client/src/game/splash_water.rs',
]
EXTRA_SOURCE_GUARDS = [
    'crates/horizon-client/src/game/potions/sound_on_added_tests.rs',
]
REGISTRATION = '\n#[cfg(test)]\n#[path = "potion_contents_fixture.rs"]\nmod contents_fixture;\n'


def effect(name='glowing', duration=800, amp=0, ambient=False, visible=True):
    return dict(id=name, duration=duration, amp=amp, ambient=ambient, visible=visible)


def corpus():
    rows = []
    def add(label, kind='splash', custom=(), scale=1, **extra):
        rows.append(dict(id=label, kind=kind, custom=list(custom), scale=scale, **extra))
    # This is deliberately first: stock main fails actual custom effect delivery
    # before colour, empty predicates, raw fields or other new tracking checks.
    add('custom-only', custom=[effect()])
    add('combined-scaled', potion='swiftness', custom=[effect('poison')], scale=.125)
    add('ambient-hidden', custom=[effect(ambient=True, visible=False)])
    add('infinite-scaled', custom=[effect(duration=-1)], scale=.125)
    add('custom-color', custom=[effect()], color=123456)
    add('water-plain', potion='water')
    add('water-custom', potion='water', custom=[effect()])
    add('empty')
    add('color-only', color=123456)
    add('name-only', name='fixture')
    for name in ('instant_health', 'instant_damage'):
        for amp in (31, 32, 33, 255):
            for half in (False, True):
                add(f'{name}-{amp}-{"half" if half else "full"}', custom=[effect(name, amp=amp)], half=half)
    add('cloud-custom', kind='lingering', custom=[effect()], scale=.5, color=123456)
    add('cloud-combined', kind='lingering', potion='swiftness', custom=[effect('poison', visible=False, ambient=True)], scale=.125)
    for name, extra in [('empty', {}), ('color', {'color':123456}), ('custom', {'custom':[effect()]}), ('base', {'potion':'swiftness'}), ('name', {'name':'fixture'})]:
        add('arrow-'+name, kind='arrow', **extra)
    return rows


def sha(data):
    return hashlib.sha256(data).hexdigest()


@contextmanager
def baseline_source(root):
    """Swap only this patch's production files; keep the identical new fixture."""
    saved = {path:(root/path).read_bytes() for path in PRODUCTION}
    try:
        for path in PRODUCTION:
            source = subprocess.check_output(['git', 'show', f'{BASE}:{path}'], cwd=root)
            if path.endswith('/game/potions.rs'):
                source += REGISTRATION.encode()
            (root/path).write_bytes(source)
        yield
    finally:
        for path, data in saved.items():
            (root/path).write_bytes(data)
        assert all((root/path).read_bytes() == data for path,data in saved.items()), 'source restoration failed'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--phase', default='all', choices=['all','oracle','baseline','fixed'])
    p.add_argument('--work-dir', type=Path, default=lab.LOCAL/'thrown-potion-components-parity')
    p.add_argument('--lab-root', type=Path, help='existing authenticated .local artifact directory, read only')
    p.add_argument('--resources', type=Path, default=lab.RESOURCES)
    a=p.parse_args()
    root=lab.ROOT
    requested_phase=a.phase
    if requested_phase=='all':
        a.phase='fixed'
    if a.resources.resolve()!=lab.RESOURCES.resolve() and (root/'.local/horizon-client').exists():
        p.error('default prepared resources must remain absent for this explicit fixture')
    build=a.work_dir.resolve(); build.mkdir(parents=True,exist_ok=True)
    if a.lab_root:
        lab.LOCAL=a.lab_root.resolve(); lab.GAME=lab.LOCAL/'minecraft'/lab.VERSION
    lock,jars,libraries=artifacts.verify_artifacts()
    rows=corpus(); input_bytes=(json.dumps(rows,indent=2)+'\n').encode()
    inputs=build/'inputs.json'; expected=build/'java.json'
    if requested_phase not in ('all','oracle') and inputs.read_bytes()!=input_bytes:
        p.error('input changed since the pinned Java oracle run')
    inputs.write_bytes(input_bytes)
    source=root/f'mods/horizon/src/test/java/dev/horizonlab/parity/{ORACLE}.java'
    native=root/'crates/horizon-client/src/game/potion_contents_fixture.rs'
    provenance=dict(base=BASE,phase=a.phase,rows=len(rows),input_sha256=sha(input_bytes),
                    oracle_source_sha256=sha(source.read_bytes()),native_source_sha256=sha(native.read_bytes()),
                    driver_source_sha256=sha(Path(__file__).read_bytes()),
                    helper_source_sha256={'ParityData.java':sha((source.parent/'ParityData.java').read_bytes())},
                    server_sha256=lab.digest(jars['server'],'sha256'),
                    methods=['PotionContents.getAllEffects/getColor/is/equals','ThrownSplashPotion.onHitAsPotion',
                             'HealOrHarmMobEffect.applyInstantaneousEffect','ThrownLingeringPotion.onHitAsPotion',
                             'AreaEffectCloud.applyImplicitComponents','Arrow.tick/AbstractArrow.tick'],
                    limits=['No cloud disk save/reload claim','Existing cloud flag lists keyed by effect ID; duplicate-ID flag variants excluded',
                            'Target heal/hurt overrides record actual pinned-method invocation amounts; no replacement amount arithmetic',
                            'Arrow environmental block effects are intercepted; actual ground counter and potion expiry remain native'])
    def run(command,label,env=None,cwd=None,timeout=1800):
        with (build/(label+'.log')).open('w',encoding='utf-8') as log:
            result=subprocess.run(list(map(str,command)),cwd=cwd or root,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=timeout)
        if result.returncode:
            raise RuntimeError(f'{label} failed ({result.returncode}); see {build/(label+".log")}')
    if requested_phase in ('all','oracle'):
        classes=build/'classes'; classes.mkdir(exist_ok=True)
        cp=os.pathsep.join(map(str,[jars['server'],*libraries]))
        expected.unlink(missing_ok=True)
        with tempfile.TemporaryDirectory(prefix='horizon-potion-java-') as scratch:
            homes={k:Path(scratch)/k for k in ('HORIZON_HOME','HORIZON_LAUNCHER_HOME','XDG_DATA_HOME','XDG_CONFIG_HOME')}
            env=lab.game_env(os.environ,**homes)
            env.pop('JAVA_TOOL_OPTIONS',None)
            run(['javac','--release',lock['minecraft']['java_major'],'-proc:none','-cp',cp,'-d',classes,source,source.parent/'ParityData.java'],'compile-java',env,cwd=scratch,timeout=180)
            run(['java','-Xmx512M','-Duser.home='+scratch,'-cp',str(classes)+os.pathsep+cp,'dev.horizonlab.parity.'+ORACLE,inputs,expected],'java',env,cwd=scratch,timeout=240)
        assert expected.exists(), 'oracle did not write a fresh reference'
        observed=json.loads(expected.read_text())
        assert len(observed)==len(rows) and [r['id'] for r in observed]==[r['id'] for r in rows]
        provenance['reference_sha256']=sha(expected.read_bytes())
        (build/'oracle-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
        print(f'{len(rows)} isolated pinned-method rows recorded; no Horizon execution yet',flush=True)
        if requested_phase=='oracle':
            return
    reference=json.loads((build/'oracle-provenance.json').read_text())
    for key in ('input_sha256','oracle_source_sha256','native_source_sha256','driver_source_sha256','helper_source_sha256','server_sha256'):
        assert reference[key]==provenance[key], f'provenance changed: {key}'
    assert reference['reference_sha256']==sha(expected.read_bytes()), 'pinned output changed'
    # Override every location even if the parent process has a real home set.
    with tempfile.TemporaryDirectory(prefix='horizon-potion-methods-') as scratch:
        homes={k:Path(scratch)/k for k in ('HORIZON_HOME','HORIZON_LAUNCHER_HOME','XDG_DATA_HOME','XDG_CONFIG_HOME')}
        env=lab.game_env(os.environ,**homes,HORIZON_TEST_RESOURCES=a.resources.resolve(),
                         HORIZON_POTION_INPUT=inputs,HORIZON_POTION_EXPECTED=expected,
                         HORIZON_POTION_OUTPUT=build/(a.phase+'.json'),CARGO_BUILD_JOBS='2',RUST_TEST_THREADS='2')
        def native_run(label):
            (build/(label+'.log')).unlink(missing_ok=True)
            provenance['production_source_sha256']={path:sha((root/path).read_bytes()) for path in PRODUCTION}
            provenance['additional_source_sha256']={path:sha((root/path).read_bytes()) for path in EXTRA_SOURCE_GUARDS}
            command=['cargo','test','-p','horizon-client','--lib','--no-run','--message-format=json']
            run(command,label+'-build',env)
            artifacts=[]
            for line in (build/(label+'-build.log')).read_text(errors='replace').splitlines():
                if not line.startswith('{'):
                    continue
                message=json.loads(line)
                target=message.get('target',{})
                if message.get('reason')=='compiler-artifact' and message.get('executable') and Path(target.get('src_path','')).resolve()==(root/'crates/horizon-client/src/lib.rs').resolve():
                    artifacts.append(Path(message['executable']).resolve())
            assert len(set(artifacts))==1, ('expected exactly one gameplay library test artifact',artifacts)
            binary=artifacts[0]
            provenance['native_binary_path']=str(binary)
            provenance['native_binary_sha256']=lab.digest(binary,'sha256')
            run([binary,'--list'],label+'-list',env,cwd=scratch,timeout=30)
            assert TEST+': test' in (build/(label+'-list.log')).read_text(), 'exact fixture not registered'
            result=build/(a.phase+'.json'); result.unlink(missing_ok=True)
            run([binary,TEST,'--ignored','--exact','--nocapture'],label,env,cwd=scratch,timeout=300)
            log=(build/(label+'.log')).read_text()
            assert re.search(r'running 1 test\b',log) and re.search(r'1 passed; 0 failed',log), 'fixture did not execute exactly once'
            assert result.exists(), 'fixture did not write a fresh result'
            observed=json.loads(result.read_text())
            assert len(observed)==len(rows), 'native row count changed'
            assert observed==json.loads(expected.read_text()), 'native output differs from pinned methods'
            assert provenance['native_binary_sha256']==lab.digest(binary,'sha256'), 'native binary changed during run'
            assert provenance['production_source_sha256']=={path:sha((root/path).read_bytes()) for path in PRODUCTION}, 'production source changed during run'
            assert provenance['additional_source_sha256']=={path:sha((root/path).read_bytes()) for path in EXTRA_SOURCE_GUARDS}, 'additional guarded source changed during run'
        if a.phase=='baseline':
            before={path:sha((root/path).read_bytes()) for path in PRODUCTION}
            try:
                with baseline_source(root):
                    native_run('baseline')
            except RuntimeError as error:
                provenance['failure']=str(error)
                provenance['source_restored']=all(sha((root/path).read_bytes())==value for path,value in before.items())
                log_path=build/'baseline.log'
                log=log_path.read_text(errors='replace') if log_path.exists() else ''
                provenance['baseline_reproduced']=bool(re.search(r'running 1 test\b',log) and re.search(r'0 passed; 1 failed',log) and re.search(r'actual player potion effects:[^\n]*custom-only',log))
                (build/'baseline-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
                if not provenance['baseline_reproduced']:
                    raise RuntimeError('baseline failed before the required actual custom-effect assertion; not reproduction evidence') from error
                print('baseline: actual custom-only player effect delivery failed against the pinned oracle; sources restored')
                return
            provenance['source_restored']=all(sha((root/path).read_bytes())==value for path,value in before.items())
            provenance['baseline_reproduced']=False
            provenance['failure']='baseline unexpectedly matched; no bug reproduction'
            (build/'baseline-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
            raise RuntimeError(provenance['failure'])
        else:
            native_run('fixed')
    provenance['reference_sha256']=sha(expected.read_bytes())
    (build/(a.phase+'-provenance.json')).write_text(json.dumps(provenance,indent=2)+'\n')
    print(f'{a.phase}: {len(rows)} rows match actual pinned methods')


if __name__=='__main__':
    main()
