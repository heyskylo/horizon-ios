#!/usr/bin/env python3
"""Compare falling-impact equipment wear and armor reduction without starting a server."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

try:
    from . import lab, test_rust_parity as base, integration_env
except ImportError:
    import lab, test_rust_parity as base, integration_env

PARITY_RUST_SCOPE = [
    'crates/horizon-world/src/living_hurt.rs',
    'crates/horizon-world/src/player_hurt.rs',
    'crates/horizon-world/src/living_damage.rs',
    'crates/horizon-world/src/server_player_combat.rs',
    'crates/horizon-world/src/server_player_damage.rs',
    'crates/horizon-client/src/health',
    'crates/horizon-client/src/health.rs',
    'crates/horizon-client/src/game.rs',
    'crates/horizon-client/src/game/creatures.rs',
    'crates/horizon-client/src/game/potions.rs',
    'crates/horizon-client/src/game/falling_damage',
    'crates/horizon-client/src/game/falling_damage.rs',
]
TEST = 'game::falling_damage::helmet_tests::falling_impacts_wear_the_helmet_before_normal_armor_and_reduction'


def main():
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'helmet-damage-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    oracle_root = lab.ROOT / 'mods/horizon/src/test/java'
    oracle = oracle_root / 'dev/horizonlab/parity/VanillaHelmetDamageOracle.java'
    fixtures = lab.ROOT / 'tests/fixtures/helmet_damage.json'
    sources = [Path(__file__), lab.ROOT / 'tools/lab.py', lab.ROOT / 'tools/test_rust_parity.py',
               lab.ROOT / 'tools/integration_env.py', oracle, fixtures,
               oracle_root / 'dev/horizonlab/parity/ParityData.java',
               oracle_root / 'dev/horizonlab/parity/VanillaBlockUpdateOracle.java',
               lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock']
    for entry in PARITY_RUST_SCOPE:
        path = lab.ROOT / entry
        sources.extend(sorted(path.rglob('*.rs')) if path.is_dir() else [path])
    tested = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))

    def run(command, name, cwd=lab.ROOT, env=None):
        with (build / (name + '.log')).open('w', encoding='utf-8') as log:
            subprocess.run(list(map(str, command)), cwd=cwd, env=env, check=True,
                           stdout=log, stderr=subprocess.STDOUT, timeout=900)

    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-sourcepath',
         oracle_root, '-cp', dependencies, '-d', classes, oracle], 'compile-java')
    with tempfile.TemporaryDirectory(prefix='horizon-helmet-parity-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env['HORIZON_TEST_RESOURCES'] = str(Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES)).resolve())
        run(['java', '-Xmx768m', '-Djava.awt.headless=true', '-cp',
             str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.VanillaHelmetDamageOracle',
             fixtures, build / 'server.txt'], 'server', cwd=scratch, env=env)
        run(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
             TEST, '--', '--exact', '--ignored', '--nocapture'], 'rust', env=env)
    records = [line for line in (build / 'rust.log').read_text(encoding='utf-8').splitlines()
               if re.fullmatch(r'HELMET \d+ -?\d+ -?\d+ [0-9a-f]{8} -?\d+', line)]
    (build / 'rust.txt').write_text('\n'.join(records) + '\n', encoding='utf-8')
    matched = base.compare(build / 'server.txt', build / 'rust.txt')
    count = 2 * len(json.loads(fixtures.read_text(encoding='utf-8')))
    if matched != count:
        raise AssertionError(f'Incomplete output: {matched} of {count} cases')
    base.ensure_sources_unchanged(sources, tested)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=tested, server_jar_sha256=lab.digest(jars['server'], 'sha256'),
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope='Real Player.hurtHelmet, Player.hurtArmor, LivingEntity.doHurtEquipment, ItemStack durability, '
              'onEquippedItemBroken modifier removal and armor reduction versus the production Game falling-hit path. '
              'Includes bare heads, nondamageable head items, breaks in both equipment phases, ordinary armor '
              'breaks, all three falling sources, a generic-source control, an ordinary mob_attack hit that breaks '
              'a boot (whose reduction must not apply to that hit) beside one that leaves it worn (#11696), and '
              'seeded Unbreaking with next RNG state. '
              'Vanilla outer hurt guards/cooldown, effects, health storage, transport and player notifications '
              'are not compared by this oracle; the Game test also checks both falling impacts and the /damage player path, '
              'cooldown-sensitive repeat hits and that draining events cannot wear equipment twice.'))
    print(f'{matched} helmet damage cases match pinned vanilla: {report}', flush=True)


if __name__ == '__main__':
    main()
