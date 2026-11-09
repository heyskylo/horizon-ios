#!/usr/bin/env python3
"""Compare actual Cow.baseTick air supply and drowning health with pinned vanilla.

Java controls eye immersion and active effect queries, then runs the real breathing
and health-damage path until tickEffects. Horizon runs production base_tick with
prepared fluid tables and real block/effect state. Covers dry air, water and bubble
columns (including opposite eye/feet blocks), all Water Breathing/Conduit Power/Breath of the Nautilus combinations,
and air values around full supply and the drowning threshold. Full fluid movement,
effect expiry, network transport and specialized aquatic breathing are out of scope.
"""
import argparse
import os
from pathlib import Path
import tempfile
try:
    from . import lab, oracle_harness as harness, test_rust_parity as base, integration_env
except ImportError:
    import lab, oracle_harness as harness, test_rust_parity as base, integration_env


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path,
                        default=Path(os.environ.get('HORIZON_TEST_RESOURCES', lab.RESOURCES)))
    args = parser.parse_args()
    resources = args.resources.resolve()
    if not (resources / 'entity-types.json').is_file():
        raise SystemExit('prepared resources required; pass --resources PATH')
    lock, jars, libraries = base.verify_artifacts()
    build = lab.LOCAL / 'rust-mob-breathing-parity'
    build.mkdir(parents=True, exist_ok=True)
    report = build / 'report.json'
    report.unlink(missing_ok=True)
    java_root = lab.ROOT / 'mods/horizon/src/test/java'
    oracle = java_root / 'dev/horizonlab/parity/VanillaMobBreathingOracle.java'
    sources = [Path(__file__), oracle, lab.ROOT / 'tools/lab.py',
               lab.ROOT / 'tools/oracle_harness.py', lab.ROOT / 'tools/test_rust_parity.py',
               lab.ROOT / 'tools/integration_env.py', lab.ROOT / 'Cargo.toml', lab.ROOT / 'Cargo.lock',
               lab.ROOT / 'crates/horizon-client/src/mobs.rs',
               lab.ROOT / 'crates/horizon-client/src/breathing.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/ai.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/living.rs',
               lab.ROOT / 'crates/horizon-client/src/mobs/world.rs',
               java_root / 'dev/horizonlab/parity/ParityData.java',
               java_root / 'dev/horizonlab/parity/VanillaBlockUpdateOracle.java']
    hashes = base.source_hashes(sources)
    classes = build / 'classes'
    classes.mkdir(exist_ok=True)
    dependencies = os.pathsep.join(map(str, [jars['server'], *libraries]))
    harness.run_logged(['javac', '--release', lock['minecraft']['java_major'], '-proc:none',
                        '-sourcepath', java_root, '-cp', dependencies, '-d', classes, oracle],
                       build, 'compile-java')
    harness.run_logged(['java', '-Xmx512M', '-Djava.awt.headless=true', '-cp',
                        str(classes) + os.pathsep + dependencies,
                        'dev.horizonlab.parity.VanillaMobBreathingOracle', build / 'server.txt'],
                       build, 'server')
    with tempfile.TemporaryDirectory(prefix='horizon-mob-breathing-') as scratch:
        env = integration_env.game_environment(os.environ, scratch)
        env.update(HORIZON_TEST_RESOURCES=str(resources),
                   HORIZON_MOB_BREATHING_OUTPUT=str(build / 'rust.txt'))
        env.setdefault('CARGO_BUILD_JOBS', '1')
        env.setdefault('RUST_TEST_THREADS', '2')
        harness.run_logged(['cargo', 'test', '--locked', '-p', 'horizon-client-lib', '--lib',
                            'mobs::ai::breathing_effect_tests::breathing_effects_and_bubble_columns_preserve_mob_air',
                            '--', '--exact', '--ignored'], build, 'rust', env=env)
    matched = harness.compare(build / 'server.txt', build / 'rust.txt', 280)
    base.ensure_sources_unchanged(sources, hashes)
    lab.write_json(report, dict(schema=1, minecraft=lock['minecraft']['version'],
        sources_sha256=hashes, server_jar_sha256=lab.digest(jars['server'], 'sha256'),
        matched_output_lines=matched, output_sha256=lab.digest(build / 'rust.txt', 'sha256'),
        scope=__doc__))
    print(f'{matched} mob breathing cases match the pinned server: {report}', flush=True)


if __name__ == '__main__':
    main()
