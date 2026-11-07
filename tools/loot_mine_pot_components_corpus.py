"""Actual pot source -> loot mine -> chest, without a world observation tick."""
from command_exec_corpus import cmd, setup

CHEST = "12 200 4"
RIGHT = "minecraft:angler_pottery_sherd"
RIGHT_NBT = '{sherds:{right:{id:"minecraft:angler_pottery_sherd"}}}'
COMPONENT = 'Items[{Slot:0b}].components."minecraft:pot_decorations"'


def cases():
    ops = setup("/tick freeze", "/forceload add 0 0")
    ops.append({"op": "settle", "required_chunks": [[0, 0]]})
    ops += setup("/fill 0 199 0 15 199 15 stone", "/fill 0 200 0 15 204 15 air",
                 "/scoreboard objectives add h035 dummy")

    def check(command, result=1, admission=True, callback=None, **meta):
        op = cmd(command, expect_result=result if callback is None else None)
        if callback is not None:
            op["expect_callback"] = callback
        op.update(meta)
        op["admission"] = admission
        ops.append(op)

    def stored(command, result, success=True, admission=False):
        ops.extend(setup("/scoreboard players set result h035 31",
                         "/scoreboard players set success h035 17"))
        check("/execute store result score result h035 store success score success h035 run " + command,
              admission=admission, callback=[success, result])
        check("/scoreboard players get result h035", result, admission)
        check("/scoreboard players get success h035", int(success), admission)

    def clock(start=False):
        holder = "start" if start else "now"
        ops.extend(setup("/scoreboard players set clock_success h035 17"))
        check(f"/execute store result score {holder} h035 store success score clock_success h035 run time query gametime",
              result=None, diagnostic="complete absolute clock receipt",
              require_success=True)
        check("/scoreboard players get clock_success h035", 1)
        if not start:
            ops.extend(setup("/scoreboard players operation now h035 -= start h035"))
            check("/scoreboard players get now h035", 0)

    def item(slot, name, count, admission=True):
        check(f"/execute if data block {CHEST} Items[{{Slot:{slot}b,id:\"{name}\",count:{count}}}]",
              admission=admission)
        check(f"/data get block {CHEST} Items[{{Slot:{slot}b}}].count", count, admission)
        check(f"/data get block {CHEST} Items[{{Slot:{slot}b}}].id", len(name), admission)

    def source(pos, decorated, phase, case):
        check(f"/execute if loaded {pos}")
        x, y, z = pos.split()
        check(f"/execute if block {x} 199 {z} stone")
        check(f"/execute if block {pos} decorated_pot[facing=north,cracked=false,waterlogged=false]")
        for field, value in [("x", int(x)), ("y", int(y)), ("z", int(z))]:
            check(f"/data get block {pos} {field}", value)
        check(f"/data get block {pos} id", len("minecraft:decorated_pot"))
        if decorated:
            check(f"/data get block {pos} sherds.right.id", len(RIGHT))
            check(f"/execute if data block {pos} sherds.right{{id:\"{RIGHT}\"}}")
        else:
            check(f"/execute unless data block {pos} sherds")
        check(f"/execute unless data block {pos} item")
        check(f"/execute unless data block {pos} LootTable")
        check(f"/data get block {pos}", result=None, admission=False,
              diagnostic="complete source block entity",
              source_snapshot=case, snapshot_phase=phase)

    check(f"/setblock {CHEST} chest[facing=north,type=single,waterlogged=false] strict")
    check(f"/item replace block {CHEST} container.26 with minecraft:diamond 7")
    check(f"/item replace block {CHEST} container.2 with minecraft:decorated_pot[minecraft:pot_decorations={{}}] 1")
    item(26, "minecraft:diamond", 7)
    item(2, "minecraft:decorated_pot", 1)
    direct = 'Items[{Slot:2b}].components."minecraft:pot_decorations"'
    check(f"/execute if data block {CHEST} {direct}")
    stored(f"data get block {CHEST} {direct}", 0, admission=True)
    clock(True)

    for case, pos, decorated in [("empty", "4 200 4", False), ("right", "7 200 4", True)]:
        nbt = RIGHT_NBT if decorated else ""
        check(f"/setblock {pos} decorated_pot[facing=north,cracked=false,waterlogged=false]{nbt} strict")
        source(pos, decorated, "before", case)
        check(f"/item replace block {CHEST} container.0 with minecraft:stone 3")
        item(0, "minecraft:stone", 3)
        stored(f"loot replace block {CHEST} container.0 1 mine {pos}", 1, admission=True)
        item(0, "minecraft:decorated_pot", 1)
        check(f"/execute if data block {CHEST} {COMPONENT}", admission=False)
        stored(f"data get block {CHEST} {COMPONENT}", 1 if decorated else 0)
        if decorated:
            check(f"/data get block {CHEST} {COMPONENT}.right.id", len(RIGHT), False)
        for side in ["back", "left", "front"] + ([] if decorated else ["right"]):
            check(f"/execute unless data block {CHEST} {COMPONENT}.{side}", admission=False)
        item(26, "minecraft:diamond", 7)
        item(2, "minecraft:decorated_pot", 1)
        check(f"/execute if data block {CHEST} {direct}")
        source(pos, decorated, "after", case)
        clock()

    check("/setblock 10 200 4 minecraft:dirt strict")
    check("/execute if loaded 10 200 4")
    check("/execute if block 10 199 4 minecraft:stone")
    check("/execute if block 10 200 4 minecraft:dirt")
    check(f"/item replace block {CHEST} container.0 with minecraft:stone 3")
    item(0, "minecraft:stone", 3)
    stored(f"loot replace block {CHEST} container.0 1 mine 10 200 4", 1, admission=True)
    item(0, "minecraft:dirt", 1)
    check(f"/execute unless data block {CHEST} {COMPONENT}")
    check("/execute if block 10 200 4 minecraft:dirt")
    item(26, "minecraft:diamond", 7)
    item(2, "minecraft:decorated_pot", 1)
    stored(f"data get block {CHEST} {direct}", 0, admission=True)
    clock()
    ops.extend(setup("/scoreboard objectives remove h035", "/forceload remove 0 0"))
    check("/tick unfreeze", 0)
    return ops
