"""tools/wire_stream.py cuts a capture's serverbound play stream into client ticks: the comparison of a Horizon
client's packets with vanilla's (tools/test_wire_stream_parity.py) is only as good as this cut."""
from pathlib import Path
import importlib.util
import struct
import unittest

# Loaded by path: tools/ on sys.path would shadow tests/ modules that share a name with a tools/ harness.
_spec = importlib.util.spec_from_file_location(
    'wire_stream', Path(__file__).resolve().parents[1] / 'tools' / 'wire_stream.py')
wire_stream = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(wire_stream)


def varint(v):
    v &= 0xFFFFFFFF
    out = b''
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out += bytes([b | 0x80])
        else:
            return out + bytes([b])


def c2s(name, data=b''):
    return {'conn': 1, 'dir': 'c2s', 'state': 'play', 'name': name, 'data': data.hex()}


def mark():
    text = b'wire-mark'
    return c2s('chat_command', varint(len(text)) + text)


LOGIN = {'conn': 1, 'dir': 's2c', 'state': 'play', 'name': 'login', 'data': struct.pack('>i', 42).hex() + '00'}
TICK_END = c2s('client_tick_end')


class WireStreamTest(unittest.TestCase):
    def test_ticks_end_at_tick_end_and_start_at_the_mark(self):
        entries = [LOGIN, c2s('move_player_rot', struct.pack('>ffB', 1, 2, 1)), TICK_END, mark(),
                   c2s('player_input', b'\x41'), c2s('move_player_pos', struct.pack('>dddB', 0.5, -60, 1.5, 1)),
                   TICK_END, c2s('pong', b'\0\0\0\7'), c2s('attack', varint(7)), c2s('punch'), TICK_END]
        [run] = wire_stream.scenarios(entries)
        self.assertEqual(run[0]['packets'], [
            ('player_input', {'keys': ['forward', 'sprint']}),
            ('move_player_pos', {'x': 0.5, 'y': -60.0, 'z': 1.5, 'on_ground': True, 'horizontal_collision': False}),
            ('client_tick_end', {})])
        # The pong is the frame's, before the tick's packets: never a tick's own, never misplaced.
        self.assertEqual(run[1]['frame'], [('pong', {'raw': '00000007'})])
        self.assertEqual(run[1]['misplaced'], [])
        # Other entities are named by first appearance, the player as `self`.
        self.assertEqual(run[1]['packets'][0], ('attack', {'entity': 'e1'}))

    def test_an_answer_between_two_packets_of_a_tick_is_misplaced(self):
        entries = [LOGIN, mark(), c2s('player_command', varint(42) + varint(1) + varint(0)), c2s('pong', b'\0\0\0\1'),
                   c2s('move_player_status_only', b'\x01'), TICK_END]
        [run] = wire_stream.scenarios(entries)
        self.assertEqual(run[0]['packets'][0], ('player_command', {'entity': 'self', 'action': 'start_sprinting',
                                                                   'data': 0}))
        self.assertEqual(run[0]['misplaced'], ['pong'])

    def test_a_block_position_decodes_signed_as_vanilla_packs_it(self):
        # BlockPos.asLong: (x & 2^26-1) << 38 | (z & 2^26-1) << 12 | (y & 2^12-1).
        def as_long(x, y, z):
            v = (x & 0x3FFFFFF) << 38 | (z & 0x3FFFFFF) << 12 | (y & 0xFFF)
            return struct.pack('>q', v - (1 << 64) if v >= 1 << 63 else v)
        data = varint(0) + as_long(-2, -61, 3) + varint(1) + struct.pack('>fff', 0.5, 1.0, 0.25) + b'\0\0' + varint(7)
        fields = wire_stream.decode('use_item_on', data)
        self.assertEqual((fields['pos'], fields['face'], fields['sequence']), ([-2, -61, 3], 1, 7))
        self.assertEqual(wire_stream.decode('player_action', varint(0) + as_long(29999999, 2047, -30000000)
                                            + b'\1' + varint(3))['pos'], [29999999, 2047, -30000000])

    def test_a_click_s_state_id_compares_as_how_far_it_lags_the_server(self):
        def set_slot(state):
            return {'conn': 1, 'dir': 's2c', 'state': 'play', 'name': 'container_set_slot',
                    'data': (varint(0) + varint(state) + b'\0\x24\0').hex()}

        def click(state):
            return c2s('container_click', varint(0) + varint(state) + b'\0\x24\0' + varint(0) + b'\0\0')
        # Two runs whose servers counted differently: both clicks are up to date, the third is one behind.
        a = wire_stream.scenarios([LOGIN, set_slot(53), mark(), click(53), TICK_END])
        b = wire_stream.scenarios([LOGIN, set_slot(55), mark(), click(55), TICK_END, set_slot(56), click(55),
                                   TICK_END])
        self.assertEqual(a[0][0]['packets'], b[0][0]['packets'])
        self.assertEqual(a[0][0]['packets'][0][1]['stale'], 0)
        self.assertEqual(b[0][1]['packets'][0][1]['stale'], 1)

    def test_a_late_answer_compares_with_vanilla_s_answer_and_the_script_keeps_its_clock(self):
        tick = lambda *names: {'packets': [(n, {}) for n in names], 'frame': [], 'misplaced': []}  # noqa
        # The server's answer (an abort) reaches vanilla in tick 1 and Horizon in tick 2; the script acts in tick 3.
        vanilla = [tick('punch'), tick('abort', 'punch'), tick('punch'), tick('pos'), tick()]
        horizon = [tick('punch'), tick('punch'), tick('abort', 'punch'), tick('pos'), tick()]
        self.assertEqual(wire_stream.compare_answered(vanilla, horizon, 4, 1, 2), [])
        # What the answer brings is compared: a new dig instead of the abort.
        restart = [tick('punch'), tick('punch'), tick('start', 'punch'), tick('pos'), tick()]
        self.assertEqual([d[0] for d in wire_stream.compare_answered(vanilla, restart, 4, 1, 2)], [1])
        # So is the script's clock: a packet a tick late anywhere else.
        late = [tick('punch'), tick('punch'), tick('abort', 'punch'), tick(), tick('pos')]
        self.assertEqual([d[0] for d in wire_stream.compare_answered(vanilla, late, 4, 1, 2)], [3, 4])

    def test_compare_reports_the_first_field_that_differs(self):
        tick = lambda y: {'packets': [('move_player_pos', {'x': 0.5, 'y': y})], 'frame': [], 'misplaced': []}  # noqa
        diffs = wire_stream.compare([tick(-60.0)], [tick(-60.0000001)], 0)
        self.assertEqual([d[0] for d in diffs], [0])
        self.assertEqual(wire_stream.field_diff(diffs[0][1], diffs[0][2]),
                         ['    #0 move_player_pos.y: vanilla -60.0, Horizon -60.0000001'])
        self.assertEqual(wire_stream.compare([tick(-60.0)], [tick(-60.0)], 0), [])


if __name__ == '__main__':
    unittest.main()
