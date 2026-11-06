#!/usr/bin/env python3
"""Compare item lore tooltip styles with the pinned vanilla client.

Vanilla prints what ``ItemLore.addToTooltip`` adds after ``ComponentUtils.mergeStyles``
with dark purple italic (VanillaLoreTooltipOracle). Horizon prints what
``item_tooltip::lore_texts`` gives (examples/probe/lore_tooltip_fixture).
"""
import json

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
    # A line with no text of its own: the root's explicit italic:0b reaches the
    # children that set no italic, and a child's italic:1b still wins there.
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'{"italic":0b,"extra":[{"text":"A"},{"text":"B","italic":1b}]}\']}}',
    # A plain-string child next to an object child, and an explicit bold:0b
    # under the dark purple italic line style.
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'{"text":"A","extra":["B",{"text":"C","color":"red","bold":0b}]}\']}}',
    # A packed RGB colour and an explicit strikethrough that a child inherits.
    '{id:"minecraft:stick",count:1,components:{"minecraft:lore":[\'{"text":"Z","color":"#12ab34","strikethrough":1b,"extra":[{"text":"Y"}]}\']}}',
]


COLORS = ['black', 'dark_blue', 'dark_green', 'dark_aqua', 'dark_red', 'dark_purple', 'gold',
          'gray', 'dark_gray', 'blue', 'green', 'aqua', 'red', 'light_purple', 'yellow', 'white',
          '#000000', '#ffffff', '#12ab34', '#c0ffee']
STYLES = ['bold', 'italic', 'underlined', 'strikethrough', 'obfuscated']


def stack(entries, display=None):
    """SNBT of a stick whose lore is the given list of NBT text components."""
    components = '"minecraft:lore":[' + ','.join(entries) + ']'
    if display is not None:
        components += ',"minecraft:tooltip_display":' + display
    return '{id:"minecraft:stick",count:1,components:{' + components + '}}'


def component(value):
    """A lore entry: a plain string stays a string, an object is JSON in NBT single quotes."""
    return "'" + json.dumps(value, separators=(',', ':'), ensure_ascii=False) + "'"


def lore(*entries):
    return stack([component(entry) for entry in entries])


def generated():
    """Styles, colours and nesting across the whole text-component surface."""
    out = []
    for color in COLORS:
        out.append(lore({'text': 'C', 'color': color}))
    for style in STYLES:
        out.append(lore({'text': 'S', style: True}))
        out.append(lore({'text': 'S', style: False}))
        # The dark purple italic line style under an explicit false, and a child
        # that turns the line's style back on or off.
        out.append(lore({'text': 'P', style: False, 'extra': [{'text': 'C', style: True}]}))
        out.append(lore({'text': 'P', style: True, 'extra': [{'text': 'C', style: False}]}))
        out.append(lore({'text': 'P', style: True, 'extra': ['plain']}))
    for color in COLORS[:16]:
        for style in STYLES:
            out.append(lore({'text': 'P', 'color': color, style: True,
                             'extra': [{'text': 'C', 'color': 'red', 'italic': False}]}))
    # Nesting chains: each level overrides one more field.
    for depth in range(1, 5):
        child = {'text': 'd' + str(depth), 'color': COLORS[depth % len(COLORS)],
                 'bold': bool(depth % 2), 'italic': bool((depth + 1) % 2),
                 'underlined': bool(depth % 3 == 0), 'strikethrough': bool(depth % 2 == 0)}
        for level in range(depth - 1, 0, -1):
            child = {'text': 'd' + str(level), 'color': COLORS[level % len(COLORS)],
                     'extra': [child], 'bold': bool(level % 2),
                     'obfuscated': bool(level % 4 == 0)}
        out.append(lore(child))
    # Siblings at one level, one plain string and one object, and empty texts.
    out.append(lore({'extra': [{'text': 'A', 'color': 'gold'}, 'B', {'text': '', 'color': 'red'},
                               {'extra': [{'text': 'C', 'italic': False}]}]}))
    out.append(lore({'text': ''}))
    out.append(lore({'text': '', 'color': 'aqua'}))
    out.append(lore('', {'text': ''}))
    # Many lines: order, mixed styles and repeated colours.
    out.append(lore(*[{'text': 'L' + str(i), 'color': COLORS[i % len(COLORS)],
                       'bold': bool(i % 2), 'italic': bool(i % 3 == 0)} for i in range(10)]))
    out.append(lore(*['line' + str(i) for i in range(6)]))
    # Text outside ASCII, with styles, and a nested non-ASCII child.
    out.append(lore({'text': '\u00e9\u4e2d\U0001F600', 'color': 'gold', 'bold': True}))
    out.append(lore({'text': 'p', 'extra': [{'text': '\U0001F600'}], 'italic': False}))
    # tooltip_display: hidden lore, another hidden component, an empty and a
    # repeated entry, and hide_tooltip (which must not hide the lore lines).
    out.append(stack([component({'text': 'H'})], '{hidden_components:["minecraft:lore"]}'))
    out.append(stack([component({'text': 'H'})], '{hidden_components:["minecraft:enchantments"]}'))
    out.append(stack([component({'text': 'H'})], '{hidden_components:[]}'))
    out.append(stack([component({'text': 'H'})], '{hidden_components:["minecraft:lore","minecraft:lore"]}'))
    out.append(stack([component({'text': 'H'})], '{hide_tooltip:1b}'))
    out.append(stack([component({'text': 'H'})], '{hide_tooltip:1b,hidden_components:["minecraft:enchantments"]}'))
    out.append(stack([component({'text': 'H'})], '{hide_tooltip:0b,hidden_components:["minecraft:lore"]}'))
    return out


def corpus():
    return CASES + generated()


def cases(build):
    lines = corpus()
    path = build / 'cases.txt'
    path.write_text(''.join(line + '\n' for line in lines))
    return path, len(lines)


if __name__ == '__main__':
    harness.differential(
        'lore-tooltip-parity', 'VanillaLoreTooltipOracle', 'lore_tooltip_fixture', cases,
        'Item lore tooltip lines: dark purple italic, child colours, explicit non-italic, a line with no text of its own,'
        ' plain-string children and packed colours, and tooltip_display; every named and packed colour, every style flag'
        ' explicit and inherited, nesting four levels deep, sibling strings and objects, empty texts, ten-line lore,'
        ' non-ASCII text and the hidden_components/hide_tooltip combinations',
        sources=[
            lab.ROOT / 'crates/horizon-client/src/item_tooltip.rs',
            lab.ROOT / 'crates/horizon-items/src/item_data.rs',
            lab.ROOT / 'crates/horizon-client-core/src/text.rs',
            lab.ROOT / 'crates/horizon-client-core/src/component.rs',
        ],
        expected_lines=len(corpus()))
