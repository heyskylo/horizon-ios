#!/usr/bin/env python3
"""Compare actual vanilla and Horizon client boat ticks across ownership changes.

An empty stand-in vanilla client level isolates cached motion from block effects
and displacement, while AbstractBoat.tick and floatBoat remain real. Horizon
loads its resources headlessly and runs tick_boat plus the actual remote boat
ticker. Both report velocity and angular-velocity bits after each tick, including
controlled ticks and dismount/reboard transitions. Real ice/control/network
behavior is covered separately by lan_e2e --check boat_ice_reboard.

Paddle cases additionally exercise actual AbstractBoat.tick/getRowingTime against
Horizon's controlled ticker and remote-copy ticker. Synced flag values and the
first passenger are controlled inputs, including stale true flags after a
player leaves and a non-player first passenger. These checks compare animation
phase and interpolation, not rendered screenshots or network packet delivery.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
try:
    from . import lab, test_rust_parity as base, test_model_baking_parity as baking
    from .integration_env import run_game
except ImportError:
    import lab
    import test_rust_parity as base
    import test_model_baking_parity as baking
    from integration_env import run_game


def cases():
    out = []
    for spin in (0.0, 0.125, -0.5, 2.0, -17.0):
        for velocity in ([0.0, 0.0, 0.0], [0.125, -0.0625, -0.25], [-1.0, 0.5, 0.75]):
            for owned in ([False], [True], [True, False], [True, False, True],
                          [False, True, True], [True, True, False, False, True]):
                out.append(dict(name=f'authority-{len(out)}', spin=spin, velocity=velocity, owned=owned))
    # A rider who leaves while the boat is moving, sits out long enough for the
    # empty boat to stop, and boards again (#6589): the re-boarded boat must
    # keep the velocity it had while empty, not the one the rider left with.
    for spin in (0.0, 0.125, -17.0):
        for velocity in ([1.0, 0.25, -0.5], [-0.125, 0.0, 0.0625]):
            for empty in (5, 25, 60):
                for back in (1, 5, 12):
                    owned = [True] * 8 + [False] + [False] * empty + [True] * back
                    out.append(dict(name=f'reboard-{len(out)}', spin=spin, velocity=velocity, owned=owned))
            # Empty from the first tick, then boarded: the boat has only ever
            # moved while nobody rode it.
            out.append(dict(name=f'reboard-{len(out)}', spin=spin, velocity=velocity,
                            owned=[False] * 40 + [True] * 8))
    # Remote observers keep rowing smoothly, then an empty boat ignores stale paddle bits.
    for initial in ([0.0, 0.0], [1.5707964, 0.7853982], [7.0, 3.0]):
        for flags in ([True, True], [True, False], [False, True]):
            steps = [dict(occupied=True, flags=flags) for _ in range(8)]
            steps += [dict(occupied=False, flags=flags) for _ in range(3)]
            steps += [dict(occupied=False, flags=[False, False])]
            steps += [dict(occupied=True, flags=flags) for _ in range(3)]
            out.append(dict(name=f'paddles-{len(out)}', spin=0, velocity=[0, 0, 0],
                            paddles=dict(initial=initial, steps=steps)))
    for flags in ([True, True], [True, False], [False, True]):
        steps = [dict(occupied=True, owned=True, flags=flags) for _ in range(5)]
        steps += [dict(occupied=False, flags=flags) for _ in range(2)]
        steps += [dict(occupied=True, owned=True, flags=[False, False])]
        steps += [dict(occupied=True, owned=True, flags=flags) for _ in range(5)]
        out.append(dict(name=f'paddles-owned-{len(out)}', spin=0, velocity=[0, 0, 0],
                        paddles=dict(initial=[0, 0], steps=steps)))
    # A non-player in the first seat is not a paddle controller, even if a
    # player occupies the other seat and metadata still has true paddle bits.
    out.append(dict(name='paddles-mob-first', spin=0, velocity=[0, 0, 0],
                    paddles=dict(initial=[1.0, 2.0], steps=[
                        dict(occupied=True, first_player=False, flags=[True, True]),
                        dict(occupied=True, first_player=False, flags=[True, False]),
                        dict(occupied=True, first_player=True, flags=[True, True]),
                        dict(occupied=False, flags=[True, True]),
                    ])))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    args = parser.parse_args()
    lock, jars, _ = base.verify_artifacts()
    source_paths = [lab.ROOT / path for path in (
        'crates/horizon-client/src/game/boats.rs',
        'crates/horizon-client/src/game/remote_vehicles.rs',
        'crates/horizon-client/src/game/lan_vehicles.rs',
        'crates/horizon-client/src/game/boat_authority_fixture.rs',
        'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaBoatAuthorityOracle.java',
        'tools/test_boat_authority_parity.py')]
    source_hashes = base.source_hashes(source_paths)
    work = lab.LOCAL / 'boat-authority-parity'
    work.mkdir(parents=True, exist_ok=True)
    (work / 'report.json').unlink(missing_ok=True)
    inputs = cases()
    lab.write_json(work / 'inputs.json', inputs)
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    sources = lab.ROOT / 'mods/horizon/src/test/java'
    package = sources / 'dev/horizonlab/parity'
    dependencies = os.pathsep.join(map(str, [jars['client'], *baking.client_libraries()]))

    def run(command, name):
        with (work / f'{name}.log').open('w') as log:
            result = subprocess.run(list(map(str, command)), cwd=lab.ROOT, stdout=log, stderr=subprocess.STDOUT,
                                    timeout=900)
        if result.returncode:
            raise RuntimeError(f'{name} failed: {(work / f"{name}.log").read_text()[-4000:]}')
    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-nowarn',
         '-sourcepath', sources, '-cp', dependencies, '-d', classes,
         *(package / f'{name}.java' for name in ('ParityData', 'VanillaVehicleContainerOracle', 'VanillaRemoteAttackOracle', 'VanillaBoatAuthorityOracle'))],
        'compile-java')
    run(['java', '-cp', str(classes) + os.pathsep + dependencies, 'dev.horizonlab.parity.VanillaBoatAuthorityOracle',
         work / 'inputs.json', work / 'vanilla.json'], 'vanilla')
    run(['cargo', 'build', '--locked', '-p', 'horizon-client', '--bin', 'horizon-client'], 'compile-rust')
    binary = lab.cargo_target() / 'debug' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    with (work / 'rust.log').open('w') as log, tempfile.TemporaryDirectory(prefix='boat-authority-') as game_dir:
        run_game([str(binary), '--headless', '--resources', str(args.resources.resolve()),
                  '--game-dir', game_dir], cwd=game_dir,
                 env=lab.game_env(HORIZON_BOAT_AUTHORITY_FIXTURE=work), stdout=log,
                 stderr=subprocess.STDOUT, check=True, timeout=180)
    native = json.loads((work / 'vanilla.json').read_text())
    rust = json.loads((work / 'rust.json').read_text())
    assert len(native) == len(rust) == len(inputs)
    errors = [(case['name'], n, r) for case, n, r in zip(inputs, native, rust) if n != r]
    if errors:
        raise AssertionError(f'{len(errors)}/{len(inputs)} boat authority cases differ: {errors[:2]}')
    ticks = sum(len(case.get('owned', case.get('paddles', {}).get('steps', []))) for case in inputs)
    base.ensure_sources_unchanged(source_paths, source_hashes)
    lab.write_json(work / 'report.json', dict(minecraft=lock['minecraft']['version'], cases=len(inputs), ticks=ticks,
                                            sources_sha256=source_hashes, client_jar_sha256=lab.digest(jars['client'], 'sha256'),
                                            output_sha256=lab.digest(work / 'rust.json', 'sha256'), scope=__doc__))
    print(f'boat authority: {len(inputs)}/{len(inputs)} cases and {ticks} tick states match vanilla bit-for-bit')


if __name__ == '__main__':
    main()
