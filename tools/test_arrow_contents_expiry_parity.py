#!/usr/bin/env python3
"""Five tipped-arrow contents/600-grounded-tick expiry cases versus vanilla 26.3.

This is a CI parity harness, discovered by tools/parity.py. It covers empty,
colour-only, glowing-only, swiftness and name-only contents, not splash or
lingering potions. The oracle accepts only glowing and swiftness for these rows.
No server construction, world bootstrap, game executable, networking or display.
The default runs the oracle then the fixed fixture; --phase fixed reuses a
matching oracle run. Baseline reproduction is separate from this comparison.
"""
import argparse
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

TEST = 'game::potions::arrow_contents::actual_arrow_contents_match_pinned_methods'
ORACLE = 'VanillaArrowContentsExpiryOracle'
PRODUCTION = [
    'crates/horizon-client-render/src/brewing.rs',
    'crates/horizon-client/src/arrows.rs',
    'crates/horizon-client/src/game/potions.rs',
]


def effect(name='glowing', duration=800, amp=0, ambient=False, visible=True):
    return dict(id=name, duration=duration, amp=amp, ambient=ambient, visible=visible)


def corpus():
    rows = []
    for name, extra in [('empty', {}), ('color', {'color':123456}), ('custom', {'custom':[effect()]}), ('base', {'potion':'swiftness'}), ('name', {'name':'fixture'})]:
        rows.append(dict(id='arrow-'+name, custom=[]))
        rows[-1].update(extra)
    return rows


def sha(data):
    return hashlib.sha256(data).hexdigest()



def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--phase', default='all', choices=['all','oracle','fixed'])
    p.add_argument('--work-dir', type=Path, default=lab.LOCAL/'arrow-contents-expiry-parity')
    p.add_argument('--lab-root', type=Path, help='existing authenticated .local artifact directory, read only')
    p.add_argument('--resources', type=Path, default=lab.RESOURCES)
    a=p.parse_args()
    root=lab.ROOT
    if a.resources.resolve()!=lab.RESOURCES.resolve() and (root/'.local/horizon-client').exists():
        p.error('default prepared resources must remain absent for this explicit fixture')
    build=a.work_dir.resolve(); build.mkdir(parents=True,exist_ok=True)
    if a.lab_root:
        lab.LOCAL=a.lab_root.resolve(); lab.GAME=lab.LOCAL/'minecraft'/lab.VERSION
    lock,jars,libraries=artifacts.verify_artifacts()
    rows=corpus(); input_bytes=(json.dumps(rows,indent=2)+'\n').encode()
    inputs=build/'inputs.json'; expected=build/'java.json'
    if a.phase=='fixed' and inputs.read_bytes()!=input_bytes:
        p.error('input changed since the pinned Java oracle run')
    inputs.write_bytes(input_bytes)
    source=root/f'mods/horizon/src/test/java/dev/horizonlab/parity/{ORACLE}.java'
    native=root/'crates/horizon-client/src/game/potions.rs'
    provenance=dict(source_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                    phase=a.phase,rows=len(rows),input_sha256=sha(input_bytes),
                    oracle_source_sha256=sha(source.read_bytes()),native_source_sha256=sha(native.read_bytes()),
                    driver_source_sha256=sha(Path(__file__).read_bytes()),
                    helper_source_sha256={'ParityData.java':sha((source.parent/'ParityData.java').read_bytes())},
                    server_sha256=lab.digest(jars['server'],'sha256'),
                    methods=['PotionContents.getColor/equals','Arrow.tick/AbstractArrow.tick'],
                    limits=['Only glowing custom effects and swiftness base potion are accepted',
                            'Arrow environmental block effects are intercepted; actual ground counter and potion expiry remain native'])
    def run(command,label,env=None,cwd=None,timeout=1800):
        with (build/(label+'.log')).open('w',encoding='utf-8') as log:
            result=subprocess.run(list(map(str,command)),cwd=cwd or root,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=timeout)
        if result.returncode:
            raise RuntimeError(f'{label} failed ({result.returncode}); see {build/(label+".log")}')
    if a.phase in ('all','oracle'):
        classes=build/'classes'; classes.mkdir(exist_ok=True)
        cp=os.pathsep.join(map(str,[jars['server'],*libraries]))
        expected.unlink(missing_ok=True)
        with tempfile.TemporaryDirectory(prefix='horizon-arrow-java-') as scratch:
            homes={k:Path(scratch)/k for k in ('HORIZON_HOME','HORIZON_LAUNCHER_HOME','XDG_DATA_HOME','XDG_CONFIG_HOME')}
            env=lab.game_env(os.environ,**homes)
            env.pop('JAVA_TOOL_OPTIONS',None)
            run(['javac','--release',lock['minecraft']['java_major'],'-proc:none','-cp',cp,'-d',classes,source,source.parent/'ParityData.java'],'compile-java',env,cwd=scratch,timeout=180)
            run(['java','-Xmx512M','-Duser.home='+scratch,'-cp',str(classes)+os.pathsep+cp,'dev.horizonlab.parity.'+ORACLE,inputs,expected],'java',env,cwd=scratch,timeout=240)
        assert expected.exists(), 'oracle did not write a fresh reference'
        observed=json.loads(expected.read_text(encoding='utf-8'))
        assert len(observed)==len(rows) and [r['id'] for r in observed]==[r['id'] for r in rows]
        provenance['reference_sha256']=sha(expected.read_bytes())
        (build/'oracle-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n',encoding='utf-8')
        print(f'{len(rows)} isolated pinned-method rows recorded; no Horizon execution yet',flush=True)
        if a.phase=='oracle':
            return
    reference=json.loads((build/'oracle-provenance.json').read_text(encoding='utf-8'))
    for key in ('input_sha256','oracle_source_sha256','native_source_sha256','driver_source_sha256','helper_source_sha256','server_sha256'):
        assert reference[key]==provenance[key], f'provenance changed: {key}'
    assert reference['reference_sha256']==sha(expected.read_bytes()), 'pinned output changed'
    # Override every location even if the parent process has a real home set.
    with tempfile.TemporaryDirectory(prefix='horizon-arrow-methods-') as scratch:
        homes={k:Path(scratch)/k for k in ('HORIZON_HOME','HORIZON_LAUNCHER_HOME','XDG_DATA_HOME','XDG_CONFIG_HOME')}
        env=lab.game_env(os.environ,**homes,HORIZON_TEST_RESOURCES=a.resources.resolve(),
                         HORIZON_POTION_INPUT=inputs,HORIZON_POTION_EXPECTED=expected,
                         HORIZON_POTION_OUTPUT=build/'fixed.json',CARGO_BUILD_JOBS='2',RUST_TEST_THREADS='2')
        def native_run(label):
            (build/(label+'.log')).unlink(missing_ok=True)
            provenance['production_source_sha256']={path:sha((root/path).read_bytes()) for path in PRODUCTION}
            command=['cargo','test','-p','horizon-client','--lib','--no-run','--message-format=json']
            run(command,label+'-build',env)
            artifacts=[]
            for line in (build/(label+'-build.log')).read_text(encoding='utf-8',errors='replace').splitlines():
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
            assert TEST+': test' in (build/(label+'-list.log')).read_text(encoding='utf-8'), 'exact fixture not registered'
            result=build/'fixed.json'; result.unlink(missing_ok=True)
            run([binary,TEST,'--ignored','--exact','--nocapture'],label,env,cwd=scratch,timeout=300)
            log=(build/(label+'.log')).read_text(encoding='utf-8')
            assert re.search(r'running 1 test\b',log) and re.search(r'1 passed; 0 failed',log), 'fixture did not execute exactly once'
            assert result.exists(), 'fixture did not write a fresh result'
            observed=json.loads(result.read_text(encoding='utf-8'))
            assert len(observed)==len(rows), 'native row count changed'
            assert observed==json.loads(expected.read_text(encoding='utf-8')), 'native output differs from pinned methods'
            assert provenance['native_binary_sha256']==lab.digest(binary,'sha256'), 'native binary changed during run'
            assert provenance['production_source_sha256']=={path:sha((root/path).read_bytes()) for path in PRODUCTION}, 'production source changed during run'
        native_run('fixed')
    provenance['reference_sha256']=sha(expected.read_bytes())
    (build/'fixed-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n',encoding='utf-8')
    print(f'fixed: {len(rows)} arrow rows match actual pinned methods')


if __name__=='__main__':
    main()
