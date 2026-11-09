"""A rejected wildcard array `set` on command storage (H246).

`StorageDataAccessor.getData` returns `CommandStorage.get`, the stored
compound itself, and `DataCommands.manipulateData` edits it in place. For a
non-empty byte, int or long array, `NbtPathArgument.AllElementsNode.setTag`
clears the array before `addTag` rejects a string. So the command fails
(`commands.data.merge.failed`, result 0, success 0) and the stored array
is empty after it. An entity's data is a snapshot (`Entity.saveWithoutId`):
the same rejected edit leaves the entity as it was.

Label classes, declared before any native run: `target-` rows read the
storage after a failed wildcard set on a non-empty array; `diag-` rows
are recorded diagnostics; every other row is a setup admission or a control
that must hold.
"""

ARRAYS = [('byte', '[B;1b,2b]'), ('int', '[I;1,2]'), ('long', '[L;1L,2L]')]
MARKER = '00000246-0000-0000-0000-000000000001'
Y = 100


def storage_alias(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/scoreboard objectives add h246 dummy', 'objective')
    cmd('/execute store result score start h246 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    cmd('/data merge storage h246:witness {v:123}', 'witness', admission=False)
    cmd('/data get storage h246:witness v', 'witness-v', 123)

    def reads(key, store):
        cmd(f'/data get storage {store}', f'{key}-all', admission=False)
        cmd(f'/data get storage {store} value', f'{key}-value', admission=False)
        cmd(f'/execute if data storage {store} value[0]', f'{key}-has-first', admission=False)
        cmd('/data get storage h246:witness v', f'{key}-witness', 123)

    def rejected(key, store, initial, value='"bad"'):
        # The plain command, then the same again with fresh result and
        # success stores, each from the same initial array.
        cmd(f'/data merge storage {store} {{value:{initial}}}', f'{key}-setup', admission=False)
        cmd(f'/data get storage {store} value', f'{key}-initial', admission=False)
        cmd(f'/data modify storage {store} value[] set value {value}', f'{key}-set', admission=False)
        reads(key, store)
        cmd(f'/data merge storage {store} {{value:{initial}}}', f'{key}-reset', admission=False)
        cmd('/scoreboard players set r h246 -7', f'{key}-r-reset', admission=False)
        cmd('/scoreboard players set s h246 -7', f'{key}-s-reset', admission=False)
        cmd(f'/execute store result score r h246 store success score s h246 run data modify storage {store} '
            f'value[] set value {value}', f'{key}-stored-set', admission=False)
        cmd('/scoreboard players get r h246', f'{key}-result', admission=False)
        cmd('/scoreboard players get s h246', f'{key}-success', admission=False)
        reads(f'{key}-stored', store)

    for kind, initial in ARRAYS:
        empty = initial.split(';')[0] + ';]'
        rejected(f'target-{kind}', f'h246:{kind}', initial)
        rejected(f'empty-{kind}', f'h246:empty_{kind}', empty)
        # A numeric value replaces every element (the positive).
        rejected(f'number-{kind}', f'h246:number_{kind}', initial, '7')
    # The same value everywhere changes nothing and clears nothing.
    rejected('same-int', 'h246:same', '[I;7,7]', '7')
    # A missing array (a control, if vanilla admits it).
    cmd('/data modify storage h246:missing value[] set value "bad"', 'missing-set', admission=False)
    cmd('/data get storage h246:missing', 'missing-all', admission=False)

    # The entity counterpart: the same rejected edit on a snapshot.
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd(f'/summon armor_stand 4.5 {Y} 4.5 {{NoGravity:1b,Invisible:1b,UUID:[I;582,0,0,1]}}', 'stand',
        admission=False)
    cmd(f'/execute as {MARKER} if entity @s[type=armor_stand,x=4.5,y={Y},z=4.5,distance=..0.01]', 'stand-at', 1)
    cmd(f'/data get entity {MARKER} UUID', 'stand-uuid', admission=False)
    cmd(f'/data modify entity {MARKER} UUID[] set value "bad"', 'entity-set', admission=False)
    cmd(f'/data get entity {MARKER} UUID', 'entity-uuid-after', admission=False)
    cmd(f'/execute if entity {MARKER}', 'entity-still', 1)
    cmd('/execute store result score now h246 run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h246 -= start h246', 'clock', 0)
    return ops
