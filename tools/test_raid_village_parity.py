#!/usr/bin/env python3
"""Isolated real Game/Java raid village-loss fixture; official dev command registration only."""
import sys
import test_command_exec_parity as driver
from raid_village_corpus import raid_villages

if __name__ == '__main__':
    sys.argv.append('--raid-village-fixture')
    if '--work-dir' not in sys.argv:
        sys.argv += ['--work-dir',str(driver.lab.LOCAL/'raid-village-parity')]
    driver.corpus_module.groups = lambda: [('live raid village loss',raid_villages())]
    driver.main()
