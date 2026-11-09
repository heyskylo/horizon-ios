#!/usr/bin/env python3
"""Check powered client placement guards in vanilla and Horizon's prediction preview.

The Java side calls the shipped onPlace methods on a client level. The Rust
regression runs actual placement through DryLevel, including server-side controls
and obstruction/loading/height checks. This does not replay a flying machine.
"""
import os
try:
    from . import lab, test_rust_parity as base
except ImportError:
    import lab
    import test_rust_parity as base


def main():
    lock, jars, libraries = base.verify_artifacts()
    resources = lab.RESOURCES / 'block-behaviours.json'
    if not resources.is_file():
        raise FileNotFoundError(resources)
    work = lab.LOCAL / 'client-redstone-preview-parity'
    work.mkdir(parents=True, exist_ok=True)
    oracle = lab.ROOT / 'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaClientRedstonePreviewOracle.java'
    dependencies = os.pathsep.join(map(str, [jars['client'], *libraries]))
    base.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
              '-cp', dependencies, '-d', str(work), str(oracle)])
    base.run(['java', '-cp', str(work) + os.pathsep + dependencies,
              'dev.horizonlab.parity.VanillaClientRedstonePreviewOracle'])
    with (work / 'rust.log').open('w', encoding='utf-8') as output:
        base.run(['cargo', 'test', '--locked', '-p', 'horizon-client', '--lib',
                  'game::remote::tests::placement_preview_checks_obstruction_without_changing_the_level',
                  '--', '--exact'], stdout=output)
    if '1 passed; 0 failed' not in (work / 'rust.log').read_text(encoding='utf-8'):
        raise AssertionError('placement preview regression did not run')
    print('Powered client TNT/piston placement guards match vanilla; server controls pass.')


if __name__ == '__main__':
    main()
