"""Read Sponge schematics (`.schem`, versions 1 to 3) without third-party code.

Written from the format's public specification (SpongePowered/Schematic-Specification): a gzipped NBT
compound with the block palette (`Palette` or `Blocks.Palette`: state string -> index), the indices as
unsigned LEB128 varints in x-fastest order (`index = x + z * Width + y * Width * Length`), and the block
entities with their position relative to the schematic's minimum corner.
"""
import gzip
import struct


def _nbt(data):
    pos = 0

    def take(n):
        nonlocal pos
        chunk = data[pos:pos + n]
        if len(chunk) != n:
            raise ValueError('truncated NBT')
        pos += n
        return chunk

    def payload(kind):
        if kind == 1:
            return struct.unpack('>b', take(1))[0]
        if kind == 2:
            return struct.unpack('>h', take(2))[0]
        if kind == 3:
            return struct.unpack('>i', take(4))[0]
        if kind == 4:
            return struct.unpack('>q', take(8))[0]
        if kind == 5:
            return struct.unpack('>f', take(4))[0]
        if kind == 6:
            return struct.unpack('>d', take(8))[0]
        if kind == 7:
            return take(struct.unpack('>i', take(4))[0])
        if kind == 8:
            return take(struct.unpack('>H', take(2))[0]).decode('utf-8', 'replace')
        if kind == 9:
            inner, count = struct.unpack('>bi', take(5))
            return [payload(inner) for _ in range(max(count, 0))]
        if kind == 10:
            out = {}
            while True:
                inner = take(1)[0]
                if inner == 0:
                    return out
                name = payload(8)
                out[name] = payload(inner)
        if kind == 11:
            count = struct.unpack('>i', take(4))[0]
            return list(struct.unpack(f'>{count}i', take(4 * count)))
        if kind == 12:
            count = struct.unpack('>i', take(4))[0]
            return list(struct.unpack(f'>{count}q', take(8 * count)))
        raise ValueError(f'unknown NBT tag {kind}')

    if take(1)[0] != 10:
        raise ValueError('NBT root is not a compound')
    payload(8)
    return payload(10)


def _varints(raw, count):
    out, value, shift = [], 0, 0
    for byte in raw:
        value |= (byte & 0x7F) << shift
        if byte & 0x80:
            shift += 7
            continue
        out.append(value)
        value, shift = 0, 0
    if len(out) != count:
        raise ValueError(f'{len(out)} block indices for {count} cells')
    return out


def parse_state(text):
    """`minecraft:repeater[delay=1,facing=east]` -> ('minecraft:repeater', {'delay': '1', 'facing': 'east'})."""
    name, _, rest = text.partition('[')
    props = dict(part.split('=', 1) for part in rest.rstrip(']').split(',') if part)
    if ':' not in name:
        name = 'minecraft:' + name
    return name, props


def read(path):
    """The schematic at `path`: size, data version, cells [(x, y, z, state string)] without air, block entities."""
    root = _nbt(gzip.decompress(path.read_bytes()))
    if 'Schematic' in root and isinstance(root['Schematic'], dict):
        root = root['Schematic']
    version = root.get('Version', 1)
    width, height, length = (root[k] & 0xFFFF for k in ('Width', 'Height', 'Length'))
    blocks = root.get('Blocks') if version >= 3 else root
    palette = {index: state for state, index in blocks['Palette'].items()}
    raw = blocks['Data'] if version >= 3 else root['BlockData']
    indices = _varints(raw, width * height * length)
    cells = []
    for i, index in enumerate(indices):
        state = palette[index]
        if state in ('minecraft:air', 'minecraft:cave_air', 'minecraft:void_air'):
            continue
        y, rest = divmod(i, width * length)
        z, x = divmod(rest, width)
        cells.append((x, y, z, state))
    entities = []
    for entity in blocks.get('BlockEntities', root.get('TileEntities', [])):
        data = entity.get('Data', entity) if version >= 3 else entity
        entities.append({'pos': list(entity['Pos']), 'id': entity.get('Id', data.get('id', '')), 'data': data})
    return {'size': [width, height, length], 'data_version': root.get('DataVersion', 0), 'cells': cells, 'block_entities': entities}
