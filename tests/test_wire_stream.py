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

    def test_compare_reports_the_first_field_that_differs(self):
        tick = lambda y: {'packets': [('move_player_pos', {'x': 0.5, 'y': y})], 'frame': [], 'misplaced': []}  # noqa
        diffs = wire_stream.compare([tick(-60.0)], [tick(-60.0000001)], 0)
        self.assertEqual([d[0] for d in diffs], [0])
        self.assertEqual(wire_stream.field_diff(diffs[0][1], diffs[0][2]),
                         ['    #0 move_player_pos.y: vanilla -60.0, Horizon -60.0000001'])
        self.assertEqual(wire_stream.compare([tick(-60.0)], [tick(-60.0)], 0), [])


if __name__ == '__main__':
    unittest.main()
