"""Six real UUID-bound pig loads, plus actual tag commands and a live rewrite."""
import json
from command_exec_corpus import cmd, setup

UNIQUE = [f"h252_{i:04d}" for i in range(1025)]
DUPLICATES = ["h252_repeat"] * 1024 + ["h252_late"]
SMALL = ["h252_beta", "h252_alpha", "h252_beta"]


def cases():
    ops = setup("/tick freeze", "/gamerule spawn_mobs false",
                "/gamerule advance_time false", "/gamerule random_tick_speed 0",
                "/forceload add 0 0")
    ops.append({"op": "settle", "required_chunks": [[0, 0]]})
    ops += setup("/fill 0 199 0 15 199 15 stone",
                 "/scoreboard objectives add h252 dummy",
                 "/execute store result score start h252 run time query gametime")

    def guard(command, expect_result=1):
        op = cmd(command, expect_result=expect_result)
        op["admission"] = True
        return op

    def stored(command, result, success=True, saved_count=None):
        ops.extend(setup("/scoreboard players set result h252 31",
                         "/scoreboard players set success h252 17"))
        op = cmd("/execute store result score result h252 store success score success h252 run " + command)
        op["expect_callback"] = [success, result]
        if saved_count is not None:
            op.update(diagnostic="complete saved Tags; ordering remains raw",
                      saved_tags_count=saved_count)
        ops.append(op)
        ops.extend([cmd("/scoreboard players get result h252", expect_result=result),
                    cmd("/scoreboard players get success h252", expect_result=int(success))])

    def member(uuid, tag, present=True):
        ops.append(cmd(f"/execute as {uuid} {'if' if present else 'unless'} entity @s[tag={tag}]",
                       expect_result=1))

    def clock():
        ops.extend(setup("/execute store result score now h252 run time query gametime",
                         "/scoreboard players operation now h252 -= start h252"))
        ops.append(guard("/scoreboard players get now h252", expect_result=0))

    def saved(uuid, count):
        stored(f"data get entity {uuid} Tags", count, bool(count), count)
        ops.append(cmd(f"/execute {'if' if count else 'unless'} data entity {uuid} Tags",
                       expect_result=1))
        # /tag list reads the live getter, independently of the saved-state read.
        stored(f"tag {uuid} list", count)

    actors = [
        ("unique", UNIQUE, 1024),
        ("duplicates", DUPLICATES, 1),
        ("small", SMALL, 2),
        ("scalar", "h252_scalar", 0),
        ("absent", None, 0),
        ("empty", [], 0),
    ]
    for index, (name, tags, count) in enumerate(actors, 1):
        uuid = f"000000fc-0000-0000-0000-{index:012x}"
        x = 2.5 + index * 2
        field = "" if tags is None else ",Tags:" + json.dumps(tags, separators=(",", ":"))
        ops.append(guard(f"/summon pig {x} 200 4.5 {{UUID:[I;252,0,0,{index}],"
                       f"Health:10f,NoAI:1b,NoGravity:1b,Motion:[0d,0d,0d],PortalCooldown:17{field}}}",
                       expect_result=1))
        identity = (f"/execute as {uuid} if entity @s[type=minecraft:pig,nbt={{"
                    f"UUID:[I;252,0,0,{index}],Health:10f,NoAI:1b,NoGravity:1b,"
                    f"Pos:[{x}d,200d,4.5d],Motion:[0d,0d,0d],PortalCooldown:17}}]")
        ops.extend([guard(f"/execute if loaded {int(x)} 200 4", expect_result=1),
                    guard(f"/execute if block {int(x)} 199 4 stone", expect_result=1),
                    guard(identity, expect_result=1)])
        body = cmd(f"/data get entity {uuid}")
        body["diagnostic"] = "complete victim body"
        ops.append(body)
        saved(uuid, count)
        member(uuid, "h252_never", False)
        if name == "unique":
            for tag in [UNIQUE[0], UNIQUE[1023]]:
                member(uuid, tag)
            member(uuid, UNIQUE[1024], False)
            stored(f"tag {uuid} add h252_extra", 0, False)
            stored(f"tag {uuid} remove {UNIQUE[0]}", 1)
            stored(f"tag {uuid} add h252_extra", 1)
            member(uuid, UNIQUE[0], False)
            member(uuid, "h252_extra")
            saved(uuid, 1024)
        elif name == "duplicates":
            member(uuid, "h252_repeat")
            member(uuid, "h252_late", False)
            stored(f"tag {uuid} add h252_added", 1)
            member(uuid, "h252_added")
            stored(f"tag {uuid} remove h252_added", 1)
            member(uuid, "h252_added", False)
            saved(uuid, 1)
        elif name == "small":
            member(uuid, "h252_alpha")
            member(uuid, "h252_beta")
            stored(f"tag {uuid} add h252_alpha", 0, False)
            stored(f"data modify entity {uuid} Tags set value "
                   + json.dumps(DUPLICATES, separators=(",", ":")), 1)
            member(uuid, "h252_repeat")
            member(uuid, "h252_late", False)
            member(uuid, "h252_alpha", False)
            saved(uuid, 1)
        else:
            member(uuid, "h252_scalar", False)
        if name in ("unique", "duplicates", "small"):
            ops.append(guard(identity, expect_result=1))
        clock()
    for index in range(1, 7):
        ops.extend(setup(f"/kill 000000fc-0000-0000-0000-{index:012x}"))
    ops.extend(setup("/scoreboard objectives remove h252", "/forceload remove 0 0"))
    ops.append(guard("/tick unfreeze", expect_result=0))
    return ops
