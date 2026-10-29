#!/usr/bin/env python3
"""Compare Creative mining eligibility for every item and tool overrides."""
import json

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness


def make_input(build):
    resources = lab.LOCAL / 'horizon-client' / '26.3'
    items = json.loads((resources / 'items.json').read_text())
    rows = [dict(item=item, creative=creative, count=count, tool=tool)
            for item in sorted(items)
            for creative in (False, True)
            for count in (0, 1)
            for tool in ('default', 'allow', 'deny', 'implicit')]
    path = build / 'inputs.jsonl'
    path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
    return path, len(rows)


def main():
    harness.differential(
        'rust-creative-mining-parity', 'VanillaCreativeMiningOracle',
        'creative_mining_probe', make_input,
        'Item.canDestroyBlock with each vanilla item, empty stacks, Survival/Creative '
        'abilities, and explicit/implicit tool overrides. Block destruction and network '
        'prediction call this predicate; end-to-end packet handling is not covered here.',
        sources=[lab.ROOT / 'crates/horizon-items/src/items.rs'])


if __name__ == '__main__':
    main()
