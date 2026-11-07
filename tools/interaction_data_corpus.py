"""A live `minecraft:interaction` entity's saved state through `/data` (H042).

`Interaction.readAdditionalSaveData` reads `width` and `height` (1.0 when
absent), the `attack` and `interaction` records (`PlayerAction`: a player
UUID and a long `timestamp`, absent when not set) and `response` (false when
absent); `addAdditionalSaveData` writes the same. `EntityDataAccessor`
loads the edited compound into the live entity and keeps its UUID. No
input event, hitbox, packet or restart is measured here.

The world is frozen and no tick runs. Interactions with fixed UUIDs are
summoned with and without settings, edited with `/data merge`, `/data
modify` and `/data remove`, and read back field by field after every edit.
A marker is the positive control of a non-living entity's `/data` write.

Label classes, declared before any native run: `target-` rows read an
Interaction after a `/data` edit; `diag-` rows are recorded diagnostics
(full bodies, malformed values, absolute clocks); every other row is a
setup admission or a control.
"""

Y = 100
EDITED = '00000042-0000-0000-0000-000000000001'
SUPPLIED = '00000042-0000-0000-0000-000000000002'
MARKER = '00000042-0000-0000-0000-000000000003'
FIELDS = ('width', 'height', 'response', 'attack', 'interaction')


def interaction_data(full=False):
    ops = []

    def cmd(text, label, result=None, admission=True, diagnostic=False):
        op = {'op': 'cmd', 'cmd': text, 'label': label, 'admission': admission}
        if result is not None:
            op['expect_result'] = result
        if diagnostic and not full:
            op.update(record=False, trace=True)
        ops.append(op)

    cmd('/tick freeze', 'freeze')
    cmd('/forceload add 0 0', 'force', 1)
    ops.append({'op': 'settle', 'required_chunks': [[0, 0]]})
    cmd(f'/execute if loaded 8 {Y} 8', 'loaded', 1)
    cmd(f'/fill 0 {Y - 1} 0 15 {Y - 1} 15 stone', 'floor', 256)
    cmd('/kill @e[type=!player]', 'clear-entities', admission=False, diagnostic=True)
    cmd(f'/summon interaction 4.5 {Y} 4.5 {{UUID:[I;66,0,0,1]}}', 'edited-summon', admission=False)
    cmd(f'/summon interaction 8.5 {Y} 4.5 {{UUID:[I;66,0,0,2],width:2.5f,height:1.25f,response:1b}}',
        'supplied-summon', admission=False)
    cmd(f'/summon marker 12.5 {Y} 4.5 {{UUID:[I;66,0,0,3]}}', 'marker-summon', admission=False)
    for name, uuid, kind, x in (('edited', EDITED, 'interaction', 4.5), ('supplied', SUPPLIED, 'interaction', 8.5),
                                ('marker', MARKER, 'marker', 12.5)):
        cmd(f'/execute as {uuid} if entity @s[type={kind},x={x},y={Y},z=4.5,distance=..0.01]', f'{name}-at', 1)
        cmd(f'/data get entity {uuid} UUID', f'{name}-uuid', admission=False)
    cmd('/scoreboard objectives add h042 dummy', 'objective')
    cmd('/execute store result score start h042 run time query gametime', 'diag-clock-start',
        admission=False, diagnostic=True)
    # The player's own UUID, for the PlayerAction records.
    cmd('/data modify storage h042:s player set from entity @p UUID', 'player-uuid', admission=False)

    def reads(key, uuid=EDITED):
        for field in FIELDS:
            cmd(f'/data get entity {uuid} {field}', f'{key}-{field}', admission=False)
        cmd(f'/data get entity {uuid} UUID', f'{key}-uuid', admission=False)
        cmd(f'/data get entity {uuid}', f'diag-{key}-body', admission=False)

    def edit(key, command):
        cmd(command, f'{key}-edit', admission=False)
        reads(key)

    # Defaults, and settings supplied when summoned.
    reads('default')
    reads('supplied', SUPPLIED)
    # Valid edits.
    edit('target-merge', f'/data merge entity {EDITED} {{width:2.5f,height:1.25f,response:1b}}')
    edit('target-merge-again', f'/data merge entity {EDITED} {{width:2.5f,height:1.25f,response:1b}}')
    edit('target-modify-width', f'/data modify entity {EDITED} width set value 3.0f')
    cmd('/data modify storage h042:s rec set value {timestamp:5L}', 'record', admission=False)
    cmd('/data modify storage h042:s rec.player set from storage h042:s player', 'record-player', admission=False)
    edit('target-attack', f'/data modify entity {EDITED} attack set from storage h042:s rec')
    cmd('/data modify storage h042:s rec.timestamp set value 9L', 'record-later', admission=False)
    edit('target-interaction', f'/data modify entity {EDITED} interaction set from storage h042:s rec')
    # Fresh result and success stores.
    cmd('/scoreboard players set r h042 -7', 'r-reset', admission=False)
    cmd('/scoreboard players set s h042 -7', 's-reset', admission=False)
    cmd(f'/execute store result score r h042 store success score s h042 run data merge entity {EDITED} '
        '{response:0b}', 'target-stored-edit', admission=False)
    cmd('/scoreboard players get r h042', 'target-stored-result', admission=False)
    cmd('/scoreboard players get s h042', 'target-stored-success', admission=False)
    reads('target-stored')
    # Removal: absent fields read their defaults again.
    edit('target-remove-attack', f'/data remove entity {EDITED} attack')
    edit('target-remove-width', f'/data remove entity {EDITED} width')
    # Malformed values (diagnostics).
    edit('diag-bad-width', f'/data merge entity {EDITED} {{width:"wide"}}')
    edit('diag-bad-record', f'/data merge entity {EDITED} {{interaction:{{player:[I;1,2,3,4]}}}}')
    # The marker: a non-living entity's /data write that already adopts.
    cmd(f'/data merge entity {MARKER} {{data:{{h042:1}}}}', 'marker-merge', admission=False)
    cmd(f'/data get entity {MARKER} data', 'marker-data', admission=False)
    cmd('/execute if entity @e[type=interaction]', 'interaction-count', admission=False)
    cmd('/execute store result score now h042 run time query gametime', 'diag-clock-now',
        admission=False, diagnostic=True)
    cmd('/scoreboard players operation now h042 -= start h042', 'clock', 0)
    return ops
