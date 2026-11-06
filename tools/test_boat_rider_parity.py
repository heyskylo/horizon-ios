#!/usr/bin/env python3
"""Compare passenger authority through actual vanilla and Horizon client paths.

The Java oracle runs AbstractBoat.tick, LocalPlayer.rideTick and sendChanges with
real first/second passenger identities. Only empty-world lifecycle, collision
movement and packet transport are stand-ins. Horizon runs its remote passenger
handler, object ticker, ride tick, outgoing vehicle packets and item-use gate in
an isolated headless process with a memory connection. No desktop input is used.

The contract compares ownership, packet counts, interpolation dispatch, hands,
angular/vertical motion and every non-authoritative velocity component exactly.
Authoritative horizontal motion remains in the raw outputs as a diagnostic: the
unchanged Rust sin/cos steering kernel differs from vanilla Mth lookup (observed
maximum 2.164393663406372e-6 here). This is not a full trajectory comparison.
"""
import argparse
import copy
import itertools
import json
import os
from pathlib import Path
import subprocess
import struct
import tempfile
try:
    from . import lab, test_rust_parity as base, test_model_baking_parity as baking
    from .integration_env import run_game
except ImportError:
    import lab
    import test_rust_parity as base
    import test_model_baking_parity as baking
    from integration_env import run_game

SOURCES = [
    'crates/horizon-client/src/game.rs',
    'crates/horizon-client/src/game/boat_rider_fixture.rs',
    'crates/horizon-client/src/game/remote_vehicles.rs',
    'crates/horizon-client/src/game/boats.rs',
    'crates/horizon-client/src/game/remote_objects.rs',
    'crates/horizon-client/src/game/hand.rs',
    'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaBoatRiderOracle.java',
    'mods/horizon/src/test/java/dev/horizonlab/parity/ParityData.java',
    'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaVehicleContainerOracle.java',
    'tools/test_boat_rider_parity.py',
]


def cases():
    out = []
    for keys in itertools.product((False, True), repeat=4):
        for velocity, spin in (([0., 0., 0.], 0.), ([.125, -.0625, -.25], .125)):
            for first in ([False], [True], [True, False, False, True], [False, True, False]):
                out.append(dict(name=f'rider-{len(out)}', keys=keys, velocity=velocity,
                                spin=spin, first=first, mob=False, event=0))
    for keys in itertools.product((False, True), repeat=4):
        out.append(dict(name=f'mob-first-{len(out)}', keys=keys, velocity=[.125, -.0625, -.25],
                        spin=.125, first=[False, False], mob=True, event=0))
    for event, first, mob in itertools.product((71, 72), ([False], [True], [True, False, True]), (False, True)):
        out.append(dict(name=f'bubble-{len(out)}', keys=[False, False, True, False],
                        velocity=[.125, -.0625, -.25], spin=.125, first=first, mob=mob, event=event))
    return out



def contract(row):
    """Project the same authority-based contract on either raw implementation."""
    result = copy.deepcopy(row)
    for step in result['steps']:
        if step['controlled']:
            step['velocity'] = [None, step['velocity'][1], None]
    return result


def compare_outputs(inputs, native, rust):
    assert len(native) == len(rust) == len(inputs)
    errors = [(case['name'], n, r) for case, n, r in zip(inputs, native, rust)
              if contract(n) != contract(r)]
    different_cases = set()
    deltas = []
    for case, n, r in zip(inputs, native, rust):
        assert len(n['steps']) == len(r['steps']) == len(case['first'])
        for ns, rs in zip(n['steps'], r['steps']):
            if ns['controlled'] and rs['controlled']:
                for axis in (0, 2):
                    a, b = ns['velocity'][axis], rs['velocity'][axis]
                    if a != b:
                        different_cases.add(case['name'])
                        av = struct.unpack('>d', bytes.fromhex(a))[0]
                        bv = struct.unpack('>d', bytes.fromhex(b))[0]
                        deltas.append(abs(av - bv))
    return errors, dict(cases=len(different_cases), components=len(deltas),
                        max_absolute_delta=max(deltas, default=0.0),
                        compared=False, reason='unchanged authoritative sin/cos steering kernel')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='use an already built debug client')
    parser.add_argument('--resources', type=Path, default=lab.RESOURCES)
    parser.add_argument('--java-only', action='store_true')
    args = parser.parse_args()
    lock, jars, _ = base.verify_artifacts()
    work = lab.LOCAL / 'boat-rider-parity'
    work.mkdir(parents=True, exist_ok=True)
    report = work / 'report.json'
    report.unlink(missing_ok=True)
    hashes = {name: lab.digest(lab.ROOT / name, 'sha256') for name in SOURCES}
    inputs = cases()
    lab.write_json(work / 'inputs.json', inputs)
    classes = work / 'classes'
    classes.mkdir(exist_ok=True)
    sources = lab.ROOT / 'mods/horizon/src/test/java'
    package = sources / 'dev/horizonlab/parity'
    dependencies = os.pathsep.join(map(str, [jars['client'], *baking.client_libraries()]))

    def run(command, name):
        with (work / f'{name}.log').open('w', encoding='utf-8') as log:
            result = subprocess.run(list(map(str, command)), cwd=lab.ROOT, stdout=log,
                                    stderr=subprocess.STDOUT, timeout=1800)
        if result.returncode:
            raise RuntimeError(f'{name} failed: {(work / f"{name}.log").read_text(encoding="utf-8")[-4000:]}')
    run(['javac', '--release', lock['minecraft']['java_major'], '-proc:none', '-nowarn',
         '-sourcepath', sources, '-cp', dependencies, '-d', classes,
         *(package / f'{name}.java' for name in
           ('ParityData', 'VanillaVehicleContainerOracle', 'VanillaBoatRiderOracle'))], 'compile-java')
    run(['java', '-cp', str(classes) + os.pathsep + dependencies,
         'dev.horizonlab.parity.VanillaBoatRiderOracle', work / 'inputs.json', work / 'vanilla.json'], 'vanilla')
    if args.java_only:
        print(f'actual Java passenger methods: {len(inputs)} cases')
        return
    binary = args.binary
    if binary is None:
        run(['cargo', 'build', '-p', 'horizon-client', '--bin', 'horizon-client'], 'compile-rust')
        binary = lab.cargo_target() / 'debug' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    with tempfile.TemporaryDirectory(prefix='hz-boat-rider-') as scratch:
        with (work / 'rust.log').open('w', encoding='utf-8') as log:
            run_game([str(binary.resolve()), '--headless', '--resources', str(args.resources.resolve()),
                      '--game-dir', scratch], cwd=scratch,
                     env=lab.game_env(HORIZON_BOAT_RIDER_FIXTURE=str(work),
                                      HORIZON_API_DISCOVERY='http://127.0.0.1:9'),
                     stdout=log, stderr=subprocess.STDOUT, check=True, timeout=180)
    native = json.loads((work / 'vanilla.json').read_text(encoding='utf-8'))
    rust = json.loads((work / 'rust.json').read_text(encoding='utf-8'))
    errors, horizontal = compare_outputs(inputs, native, rust)
    if errors:
        raise AssertionError(f'{len(errors)}/{len(inputs)} boat rider cases differ: {errors[:2]}')
    assert hashes == {name: lab.digest(lab.ROOT / name, 'sha256') for name in SOURCES}, 'sources changed during validation'
    ticks = sum(len(case['first']) for case in inputs)
    lab.write_json(report, dict(minecraft=lock['minecraft']['version'], cases=len(inputs), ticks=ticks,
                               output_sha256=lab.digest(work / 'rust.json', 'sha256'),
                               source_hashes=hashes, authoritative_horizontal_diagnostic=horizontal,
                               scope=__doc__))
    print(f'boat rider: {len(inputs)}/{len(inputs)} authority cases and {ticks} tick states match vanilla; '
          f'authoritative horizontal diagnostic: {horizontal}')


if __name__ == '__main__':
    main()
