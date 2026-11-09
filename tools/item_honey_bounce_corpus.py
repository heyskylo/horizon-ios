"""Dropped-item motion from a honey wall onto its slime-block base."""


def item_honey_bounce(cmd, setup):
    """#7467: the item's box lands on slime even with its center outside it."""
    ops = setup(
        '/fill 3 220 4 4 223 4 air',
        '/setblock 4 220 4 slime_block',
        '/fill 4 221 4 4 223 4 honey_block',
        '/setblock 11 220 4 slime_block',
    )
    cases = [
        ('honey_edge', '3.75 223.7 4.5', '[0.25d,-0.2d,0d]', 746703),
        ('direct_slime', '11.5 222.7 4.5', '[0d,-0.2d,0d]', 746708),
    ]
    ground_guards = {
        ('honey_edge', 16): 1,
        ('honey_edge', 17): 0,
        ('direct_slime', 6): 1,
        ('direct_slime', 8): 0,
    }
    for tag, position, motion, entity_id in cases:
        # Control the four-tick schedule once the item is on the ground.
        ops.append({
            'op': 'sync_randoms',
            'uniquifier': entity_id,
            'entity_id': entity_id - 1,
            'level_seed': 7467,
        })
        ops.append(cmd(
            f'/summon item {position} {{Tags:["{tag}"],PickupDelay:32767s,'
            f'Item:{{id:"minecraft:stone",count:1}},Motion:{motion}}}',
            expect_result=1,
        ))
    for tick in range(1, 19):
        ops.append({'op': 'tick', 'n': 1})
        for tag, _, _, _ in cases:
            item = f'@e[type=item,tag={tag},limit=1]'
            for field in ('Pos[0]', 'Pos[1]', 'Motion[1]'):
                # Confirm the honey slider's center is outside the block at
                # contact. All motion values still come from the Java run.
                expected = 39375 if (tag, tick, field) == ('honey_edge', 16, 'Pos[0]') else None
                ops.append(cmd(f'/data get entity {item} {field} 10000', expect_result=expected))
            ops.append(cmd(f'/data get entity {item} OnGround', expect_result=ground_guards.get((tag, tick))))
    return ops + setup(
        '/kill @e[type=item,tag=honey_edge]',
        '/kill @e[type=item,tag=direct_slime]',
        '/fill 4 220 4 4 223 4 air',
        '/setblock 11 220 4 air',
    )
