"""H189 storage literal design; no production parser or formatter changes."""


def cases():
    return [
        ('hex', '0x10', '16', 16),
        ('underscore', '1_000', '1000', 1000),
        ('unicode4', r'"\u0041"', '"A"', 1),
        ('hex2', r'"\x41"', '"A"', 1),
        ('unicode8', r'"\U00000041"', '"A"', 1),
        ('trailing', '{n:3,}', '{n:3}', 1),
        ('plain', 'plain', '"plain"', 5),
    ]


def storage_literals(cmd):
    ops = [cmd('/tick freeze'),
           cmd('/scoreboard objectives add h189 dummy', expect_result=1),
           cmd('/scoreboard players set #before h189 -31', expect_result=-31),
           cmd('/scoreboard players set #after h189 -17', expect_result=-17),
           cmd('/execute store result score #before h189 store success score #clock_ok h189 run time query gametime', False),
           cmd('/scoreboard players get #clock_ok h189', expect_result=1),
           cmd('/data merge storage h189:admission {v:21}', expect_result=1),
           cmd('/data get storage h189:admission v', expect_result=21)]

    def stored(command, result, success):
        return [cmd('/scoreboard players set #r h189 31', expect_result=31),
                cmd('/scoreboard players set #s h189 17', expect_result=17),
                cmd('/execute store result score #r h189 store success score #s h189 run ' + command),
                cmd('/scoreboard players get #r h189', expect_result=result),
                cmd('/scoreboard players get #s h189', expect_result=success)]

    for name, modern, plain, value in cases():
        reference = 'h189:ref_' + name
        ops += [cmd('/data merge storage ' + reference + ' {v:' + plain + '}', expect_result=1),
                cmd('/data get storage ' + reference + ' v', expect_result=value)]
        for route in ['merge', 'modify']:
            target = 'h189:' + route + '_' + name
            ops += [cmd('/data merge storage ' + target + ' {v:"seed"}', expect_result=1),
                    cmd('/data get storage ' + target + ' v', expect_result=4)]
            write = ('data merge storage ' + target + ' {v:' + modern + '}' if route == 'merge'
                     else 'data modify storage ' + target + ' v set value ' + modern)
            ops += stored(write, 1, 1)
            ops += [cmd('/data get storage ' + target + ' v', expect_result=value)]
            # The real stored Tag must equal the canonical one: copying it
            # back must be a no-change failure, independently of pretty text.
            ops += stored('data modify storage ' + target + ' v set from storage ' + reference + ' v', 0, 0)
            ops += [cmd('/data get storage ' + target + ' v', expect_result=value),
                    cmd('/data remove storage ' + target + ' v', expect_result=1)]
        ops += [cmd('/data remove storage ' + reference + ' v', expect_result=1)]
    ops += [cmd('/data remove storage h189:admission v', expect_result=1),
            cmd('/execute store result score #after h189 store success score #clock_ok h189 run time query gametime', False),
            cmd('/scoreboard players get #clock_ok h189', expect_result=1),
            cmd('/execute if score #before h189 = #after h189', expect_result=1),
            cmd('/scoreboard objectives remove h189'), cmd('/tick unfreeze')]
    return ops
