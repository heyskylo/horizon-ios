#!/usr/bin/env python3
"""Compare a level's saved PORTAL chunk tickets with vanilla's TicketStorage: what loads from
chunk_tickets.dat, how the tickets count down (not while their chunk is still loading), and what
a save and a restart keep."""
import json
import re

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab
    import oracle_harness as harness


def portal(x, z, level=30, left=None, kind='portal'):
    ticks = '' if left is None else f', ticks_left: {left}'
    return f'{{type: "minecraft:{kind}", level: {level}, chunk_pos: [I; {x}, {z}]{ticks}}}'


def saved(*tickets):
    return '{tickets: [' + ', '.join(tickets) + ']}'


TICK = {'op': 'tick'}
RESTART = {'op': 'restart'}


def loading(*chunks):
    return {'op': 'tick', 'loading': [list(c) for c in chunks]}


CASES = [
    # No file contents, then a ticket placed and saved over a restart.
    ('{}', [{'op': 'portal', 'pos': [3, -4]}, TICK, RESTART, TICK]),
    # The cannon's saved Overworld tickets keep their lifetimes and count down.
    (saved(portal(-1, 8, left='284L'), portal(-6, -48, left='274L'), portal(2, 5, left='290L')),
     [TICK, TICK, RESTART, TICK]),
    # The last tick a ticket lives, and one saved at or past it.
    (saved(portal(0, 0, left='1L'), portal(1, 0, left='0L'), portal(2, 0, left='-5L')), [TICK, TICK]),
    # A record without ticks_left reads as 0.
    (saved(portal(5, 5)), [TICK]),
    # A chunk still loading holds its ticket's lifetime; another chunk's doesn't.
    (saved(portal(2, 5, left='3L'), portal(9, 9, left='3L')),
     [loading((2, 5)), loading((2, 5), (40, 40)), TICK, TICK, loading((2, 5)), TICK, TICK, TICK]),
    # A ticket whose chunk loads from the save, over a restart while it loads.
    (saved(portal(-3, 7, left='2L')), [loading((-3, 7)), RESTART, loading((-3, 7)), TICK, TICK, TICK]),
    # Two of one level on a chunk: the second only restarts the first's timeout.
    (saved(portal(4, 4, left='10L'), portal(4, 4, left='20L')), [TICK]),
    # Other levels on one chunk are other tickets.
    (saved(portal(4, 4, 28, '10L'), portal(4, 4, 30, '20L'), portal(4, 4, 33, '5L')), [TICK, RESTART]),
    # A new portal ticket refreshes the saved one of its level only.
    (saved(portal(0, 1, 30, '50L'), portal(7, 7, 28, '50L')),
     [{'op': 'portal', 'pos': [0, 1]}, {'op': 'portal', 'pos': [7, 7]}, TICK, RESTART]),
    # Forced chunks stay as they are beside the portal tickets.
    (saved(portal(1, 1, 31, kind='forced'), portal(1, 1, left='2L'), portal(-9, 3, 31, kind='forced')),
     [TICK, RESTART, TICK, TICK, RESTART]),
    # ticks_left saved as an int, or past an int.
    (saved(portal(0, 0, left='7'), portal(1, 1, left='5000000000L')), [TICK, RESTART]),
    # A long's decrement wraps: this ticket never times out.
    (saved(portal(0, 0, left='-9223372036854775808L')), [TICK, TICK, RESTART]),
    # Levels far from the portal's own.
    (saved(portal(0, 0, 0, '3L'), portal(0, 0, 44, '3L'), portal(8, -8, 31, '3L')), [TICK, TICK]),
    # A ticket that ran out is gone after the restart.
    (saved(portal(6, 6, left='0L'), portal(6, 7, left='9L')), [TICK, RESTART, TICK]),
    # Placed while loading, counted once loaded.
    ('{tickets: []}', [{'op': 'portal', 'pos': [0, 0]}, loading((0, 0)), loading((0, 0)), TICK, RESTART]),
    # Negative chunk coordinates far out.
    (saved(portal(-1875000, 1874999, left='12L')), [TICK, RESTART, loading((-1875000, 1874999)), TICK]),
    # A new ticket whose chunk loads for longer than its timeout is still held, then counts
    # down once loaded (canTicketExpire). Its live hold, not the save, tells the pause apart.
    ('{tickets: []}', [{'op': 'portal', 'pos': [3, 3]}, {**loading((3, 3)), 'times': 301}, TICK,
                       {**loading((3, 3), (0, 0)), 'times': 2}, {'op': 'tick', 'times': 299}, TICK]),
]


def chunks(saved, ops):
    """Every chunk a case names: each snapshot says which of them a PORTAL ticket holds."""
    found = {(int(x), int(z)) for x, z in re.findall(r'\[I; (-?\d+), (-?\d+)\]', saved)}
    for op in ops:
        found.update(tuple(c) for c in op.get('loading', []) + ([op['pos']] if 'pos' in op else []))
    return sorted(list(c) for c in found)


def make_input(build):
    path = build / 'inputs.jsonl'
    path.write_text(''.join(json.dumps({'saved': s, 'ops': ops, 'chunks': chunks(s, ops)}) + '\n'
                            for s, ops in CASES), encoding='utf-8')
    return path, len(CASES)


def main():
    harness.differential(
        'portal-tickets-parity', 'VanillaPortalTicketsOracle', 'portal_tickets_fixture', make_input,
        'Actual TicketStorage.CODEC, activateAllDeactivatedTickets, purgeStaleTickets with real '
        'ChunkMap.getUpdatingChunkIfPresent and ChunkHolder.isReadyForSaving on allocated holders, '
        'addTicketWithRadius(PORTAL, 3) and deactivateTicketsOnClosing, against SaveDir and '
        'ChunkTickets through a real chunk_tickets.dat. Excludes level propagation, streaming '
        'readiness and other ticket types than FORCED beside PORTAL.',
        sources=[lab.ROOT / 'crates/horizon-client/src/save.rs',
                 lab.ROOT / 'crates/horizon-client/src/ticket_levels.rs'],
        expected_lines=len(CASES))


if __name__ == '__main__':
    main()
