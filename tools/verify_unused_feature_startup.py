#!/usr/bin/env python3
"""Check the two complete startup contracts, preserving process status separately from readiness."""
import argparse
import hashlib
import json
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def outcome(parent, kind, case):
    folder = parent / f'{kind}-{case}'
    facts = json.loads((folder / 'outcome.json').read_text())
    assert facts['log_sha256'] == digest(folder / 'server.log')
    pack = folder / 'game/world/datapacks/h259'
    entry = pack / 'data/h259/worldgen/feature/unused_probe.json'
    assert facts['input_sha256'] == digest(entry)
    assert facts['metadata_sha256'] == digest(pack / 'pack.mcmeta')
    assert facts['reaped'] and not facts['timed_out'] and not facts.get('probe_error'), facts
    text = (folder / 'server.log').read_text()
    return facts, text, entry.read_bytes(), (pack / 'pack.mcmeta').read_bytes()


def verify(reference, native):
    report = {'scope': 'PARTIAL: two unused fill_layer startup outcomes only', 'cases': {}}
    for case in ['valid', 'invalid']:
        java, jlog, jinput, jmeta = outcome(reference, 'java', case)
        game, glog, ginput, gmeta = outcome(native, 'native', case)
        assert jinput == ginput and jmeta == gmeta
        assert java['return_code'] == 0, java
        if case == 'valid':
            assert java['ready'] and java['service'] and 'file/h259' in jlog
            passed = game['ready'] and game['service'] and 'file/h259' in glog
            assert not any(x['action'] == 'kill' for x in java['probe_termination'])
            passed = passed and game['return_code'] == 0 and not any(
                x['action'] == 'kill' for x in game['probe_termination'])
        else:
            assert not java['ready'] and not java['service'] and not java['probe_termination']
            assert 'h259:unused_probe' in jlog and 'Value -1 outside of range [0:4064]' in jlog
            passed = (not game['ready'] and not game['service'] and not game['probe_termination']
                      and 'h259:unused_probe' in glog and 'fill-layer height' in glog)
        report['cases'][case] = {
            'contract_matches': passed,
            'java': {k: java[k] for k in ['ready', 'service', 'timed_out', 'return_code', 'reaped', 'probe_termination']},
            'native': {k: game[k] for k in ['ready', 'service', 'timed_out', 'return_code', 'reaped', 'probe_termination']},
        }
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', type=Path, required=True)
    parser.add_argument('--native', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    report = verify(args.reference, args.native)
    args.report.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(report, indent=2))
    return 0 if all(x['contract_matches'] for x in report['cases'].values()) else 1


if __name__ == '__main__':
    raise SystemExit(main())
