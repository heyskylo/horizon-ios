#!/usr/bin/env python3
"""Run the isolated live Enderman goal/Game placement contract (no random timing comparison)."""
import sys
import harness_args
import test_command_exec_parity as driver
from enderman_placement_corpus import enderman_placement

if __name__ == '__main__':
    diagnostic = '--selector-position-diagnostic' in sys.argv
    if diagnostic:
        sys.argv.remove('--selector-position-diagnostic')
    property_control = '--property-control' in sys.argv
    if property_control:
        sys.argv.remove('--property-control')
    harness_args.ensure_work_dir(driver.lab.LOCAL / 'enderman-placement-parity')
    driver.corpus_module.groups = lambda: [('enderman neighbour placement', enderman_placement(diagnostic, property_control))]
    driver.main()
