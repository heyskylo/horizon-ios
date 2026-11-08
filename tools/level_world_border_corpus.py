"""Each builtin level keeps its own world border (ServerLevel.getWorldBorder).

WorldBorderCommand edits source.getLevel()'s border. The program sets the
Overworld to 50 and the Nether to 100 and leaves the End at its default,
and reads all three after each change, from the server's own source and
through /execute in. It includes failed commands (no change, a size out of
range) and restoration controls (each level set back to the default).
"""
from command_exec_corpus import cmd

LEVELS = ['minecraft:overworld', 'minecraft:the_nether', 'minecraft:the_end']


def level_world_border():
    ops = []

    def run(text, expected=None, record=True):
        ops.append(cmd(text, record=record, expect_result=expected))

    def read_all():
        run('/worldborder get')
        for level in LEVELS:
            run(f'/execute in {level} run worldborder get')
            # Not a pure read: the first one moves each level's center to the source.
            run(f'/execute in {level} run worldborder center ~ ~')
        # The source's own level, as the command reports it.
        run('/execute if dimension minecraft:overworld')

    def clock(label):
        run(f'/execute store result score #{label} h260 run time query gametime', record=False)

    run('/scoreboard objectives add h260 dummy', 1)
    for level in LEVELS:
        run(f'/execute in {level} run forceload add 0 0', 1)
    ops.append({'op': 'tick', 'n': 20})
    ops.append({'op': 'settle'})
    # Java's oracle loads only the Overworld chunk here; the others are recorded raw.
    run('/execute in minecraft:overworld if loaded 0 64 0', 1)
    for level in LEVELS[1:]:
        run(f'/execute in {level} if loaded 0 64 0')
    read_all()
    run('/worldborder set 50')
    read_all()
    run('/execute in minecraft:the_nether run worldborder set 100')
    read_all()
    # Failed commands: no change, and sizes out of range.
    run('/worldborder set 50')
    run('/execute in minecraft:the_nether run worldborder set 100')
    run('/execute in minecraft:the_end run worldborder set 0.5')
    run('/execute in minecraft:the_end run worldborder set 60000000')
    read_all()
    # A change that takes time, then a tick: only its own level moves.
    clock('a')
    run('/execute in minecraft:the_nether run worldborder add 10 1')
    ops.append({'op': 'tick', 'n': 25})
    read_all()
    # Restoration controls: each level back to the default size.
    run('/worldborder set 59999968')
    run('/execute in minecraft:the_nether run worldborder set 59999968')
    read_all()
    for level in LEVELS:
        run(f'/execute in {level} run forceload remove 0 0', record=False)
    run('/scoreboard objectives remove h260', record=False)
    return ops
