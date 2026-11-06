import gzip
import struct
import tempfile
import unittest
from pathlib import Path
from tools import lra_schem


def name(text):
    raw = text.encode()
    return struct.pack('>H', len(raw)) + raw


def tag(kind, key, payload):
    return bytes([kind]) + name(key) + payload


def compound(*tags):
    return b''.join(tags) + b'\0'


def short(key, value):
    return tag(2, key, struct.pack('>h', value))


def schematic(version, palette, indices, width, height, length, entities=()):
    """A Sponge schematic written by hand from the specification, not by the reader under test."""
    data = bytearray()
    for index in indices:
        while True:
            byte = index & 0x7F
            index >>= 7
            data.append(byte | (0x80 if index else 0))
            if not index:
                break
    palette_tag = compound(*(tag(3, state, struct.pack('>i', i)) for state, i in palette.items()))
    entity_tags = b''.join(compound(tag(11, 'Pos', struct.pack('>4i', 3, *pos)), tag(8, 'Id', name(kind)), *extra)
                           for pos, kind, extra in entities)
    entity_list = struct.pack('>bi', 10, len(entities)) + entity_tags
    data_tag = tag(7, 'Data' if version == 3 else 'BlockData', struct.pack('>i', len(data)) + bytes(data))
    sizes = [short('Width', width), short('Height', height), short('Length', length), tag(3, 'Version', struct.pack('>i', version))]
    if version == 3:
        blocks = tag(10, 'Blocks', compound(tag(10, 'Palette', palette_tag), data_tag, tag(9, 'BlockEntities', entity_list)))
        root = compound(tag(10, 'Schematic', compound(*sizes, blocks)))
    else:
        root = compound(*sizes, tag(10, 'Palette', palette_tag), data_tag, tag(9, 'BlockEntities', entity_list))
    return gzip.compress(tag(10, '', root))


class SpongeSchematicTests(unittest.TestCase):
    def read(self, raw):
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / 'a.schem'
            path.write_bytes(raw)
            return lra_schem.read(path)

    def test_version_2_cells_follow_x_then_z_then_y(self):
        palette = {'minecraft:air': 0, 'minecraft:stone': 1, 'minecraft:lever[powered=true]': 2}
        # 2 wide, 2 tall, 1 long: (0,0,0) stone, (1,0,0) air, (0,1,0) air, (1,1,0) lever.
        out = self.read(schematic(2, palette, [1, 0, 0, 2], 2, 2, 1))
        self.assertEqual(out['size'], [2, 2, 1])
        self.assertEqual(out['cells'], [(0, 0, 0, 'minecraft:stone'), (1, 1, 0, 'minecraft:lever[powered=true]')])

    def test_version_3_reads_multi_byte_indices_and_block_entities(self):
        palette = {f'minecraft:block_{i}': i for i in range(300)}
        signal = tag(3, 'OutputSignal', struct.pack('>i', 7))
        out = self.read(schematic(3, palette, [299, 128], 1, 1, 2, [((0, 0, 1), 'minecraft:comparator', [signal])]))
        self.assertEqual(out['cells'], [(0, 0, 0, 'minecraft:block_299'), (0, 0, 1, 'minecraft:block_128')])
        self.assertEqual(out['block_entities'][0]['pos'], [0, 0, 1])
        self.assertEqual(out['block_entities'][0]['data']['OutputSignal'], 7)

    def test_wrong_index_count_is_refused(self):
        with self.assertRaises(ValueError):
            self.read(schematic(2, {'minecraft:stone': 0}, [0, 0, 0], 2, 2, 1))

    def test_state_strings_split_into_name_and_properties(self):
        self.assertEqual(lra_schem.parse_state('repeater[delay=2,facing=east]'),
                         ('minecraft:repeater', {'delay': '2', 'facing': 'east'}))
        self.assertEqual(lra_schem.parse_state('minecraft:stone'), ('minecraft:stone', {}))


if __name__ == '__main__':
    unittest.main()
