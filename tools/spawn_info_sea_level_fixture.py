#!/usr/bin/env python3
"""Builtin flat/normal generator sea level in real login and respawn packets."""
import argparse,hashlib,json,os,subprocess,sys
from pathlib import Path
import test_respawn_world_point_parity as route
net=route.net
PROGRAM={'scenes':['flat','normal'],'route':'existing H330 actual login,disconnect,rejoin,kill,ClientCommand0,respawn','strict':'existing independent identity/health/pose/load/floor/world-point/clock guards; actual packet login/respawn count/order; complete decoded SpawnInfo dimension/type/mode/debug/flat/portal/sea fields','raw':'complete packet payloads/all decoded SpawnInfo fields and seed/previous-mode/lastDeath differences,full bodies/absolute clocks','sea':{'flat':-63,'normal':63}}

def main():
    p=argparse.ArgumentParser();p.add_argument('--scene',choices=['flat','normal'],required=True);p.add_argument('--side',choices=['java','native'],required=True);p.add_argument('--label',required=True);p.add_argument('--binary',type=Path,required=True);p.add_argument('--reference',type=Path);a=p.parse_args()
    net.PLAYERS=[route.PLAYER];net.PROPERTIES.update({'gamemode':'survival','force-gamemode':'false','view-distance':'2','simulation-distance':'2','level-type':'minecraft\\:'+('flat' if a.scene=='flat' else 'normal')})
    _,build,java,native,client=net.sides('h258/'+a.scene,a.binary.resolve())
    (build/'declaration.json').write_text(json.dumps(dict(program=PROGRAM,sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()),indent=2)+'\n')
    world=build/'generate/world'
    if not world.exists():world=route.generate(java,build)
    assert not (build/a.label).exists(),'retain attempts;new label'
    route.run(a.label,java if a.side=='java' else native,client,build,world)
    value=route.facts(build,a.label)
    # Validate actual lifecycle/admissions independently, never from the sea field.
    route.validate_java(value)
    _,jars,libs=net.base.verify_artifacts();cp=os.pathsep.join(map(str,[build/'classes',jars['server'],*libs]))
    subprocess.run(['javac','-proc:none','-cp',cp,'-d',build/'classes',net.lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/SpawnInfoDecode.java'],check=True)
    output=build/f'{a.label}-spawn.jsonl'
    subprocess.run(['java','-Xmx1g','-cp',cp,'dev.horizonlab.parity.SpawnInfoDecode',build/f'{a.label}-packets.jsonl',output],check=True)
    rows=[json.loads(x) for x in output.read_text().splitlines()];assert [x['wire']['name'] for x in rows]==['login','login','respawn'],rows
    for i,row in enumerate(rows):
     for key in ['dimension_type','dimension','mode','debug','flat','portal_cooldown','sea_level']:value[f'spawn/{i}/{key}']=row[key]
     assert row['dimension_type']==row['dimension']=='minecraft:overworld'
     assert row['flat']==(a.scene=='flat') and not row['debug'] and row['mode']==0 and row['portal_cooldown']==0
     if a.side=='java':assert row['sea_level']==PROGRAM['sea'][a.scene],row
    path=build/f'{a.label}-facts.json';path.write_text(json.dumps(value,indent=2,sort_keys=True)+'\n')
    if a.reference:
     reference=a.reference.read_bytes();expected=json.loads(reference);assert expected.keys()==value.keys()
     diff=[dict(label=k,expected=expected[k],actual=value[k]) for k in sorted(value) if value[k]!=expected[k]]
     (build/f'{a.label}-report.json').write_text(json.dumps(dict(records=len(value),passed=len(value)-len(diff),differences=diff,reference_sha256=hashlib.sha256(reference).hexdigest()),indent=2)+'\n');print(len(value)-len(diff),'/',len(value));sys.exit(bool(diff))
    print(len(value),'declared facts')

if __name__ == "__main__":
    main()
