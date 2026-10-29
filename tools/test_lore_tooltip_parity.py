#!/usr/bin/env python3
"""Compare item lore tooltip styles with the pinned vanilla client.

Vanilla prints what ``ItemLore.addToTooltip`` adds after ``ComponentUtils.mergeStyles``
with dark purple italic (VanillaLoreTooltipOracle). Horizon prints what
``item_tooltip::lore_texts`` gives (examples/probe/lore_tooltip_fixture).
"""
try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness

# SNBT stacks. A lore line is a text component; italic:0b is an explicit false.
CASES = [
    '{id:"minecraft:stick",count:1}',
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'"Plain"\']}}',
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'{"text":"Gold","color":"gold","italic":0b}\']}}',
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'{"text":"A","extra":[{"text":"B","color":"aqua","italic":0b}]}\']}}',
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'"One"\',\'{"text":"Two","bold":1b}\']}}',
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'"Hidden"\'],"minecraft:tooltip_display":{hidden_components:["minecraft:lore"]}}}',
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'{"text":"Line","underlined":1b,"strikethrough":1b}\']}}',
]


def cases(build):
    path = build / 'cases.txt'
    path.write_text(''.join(line + '\n' for line in CASES))
    return path, len(CASES)


if __name__ == '__main__':
    harness.differential(
        'lore-tooltip-parity', 'VanillaLoreTooltipOracle', 'lore_tooltip_fixture', cases,
        'Item lore tooltip lines: dark purple italic, child colours, explicit non-italic, and tooltip_display',
        sources=[
            lab.ROOT / 'crates/horizon-client/src/item_tooltip.rs',
            lab.ROOT / 'crates/horizon-items/src/item_data.rs',
            lab.ROOT / 'crates/horizon-client-core/src/text.rs',
            lab.ROOT / 'crates/horizon-client-core/src/component.rs',
        ],
        expected_lines=len(CASES))
