#!/usr/bin/env python3
"""Dump Minecraft NBT files (level.dat, data/*.dat, players/*.dat) and Anvil
region chunks (region/, entities/, poi/) as readable SNBT-like text.

Usage:
  anvil_dump.py FILE.dat [PATH...]
  anvil_dump.py r.0.0.mca [CX CZ] [PATH...]

PATH selects nested keys (for example `sections 3 block_states`).

Options:
  --full      print long arrays whole instead of summarising them
  --typed     suffix numbers with their NBT type (b, s, l, f, d; plain ints are TAG_Int)
  -h, --help  print this help and exit

Used to compare Horizon saves with worlds written by the vanilla server.
"""
import gzip
import io
import json
import struct
import sys
import zlib

USAGE = __doc__


def read_tag(f, tag_id):
    if tag_id == 1:
        return ('b', struct.unpack('>b', f.read(1))[0])
    if tag_id == 2:
        return ('s', struct.unpack('>h', f.read(2))[0])
    if tag_id == 3:
        return struct.unpack('>i', f.read(4))[0]
    if tag_id == 4:
        return ('l', struct.unpack('>q', f.read(8))[0])
    if tag_id == 5:
        return ('f', struct.unpack('>f', f.read(4))[0])
    if tag_id == 6:
        return ('d', struct.unpack('>d', f.read(8))[0])
    if tag_id == 7:
        n = struct.unpack('>i', f.read(4))[0]
        return ('B', list(struct.unpack(f'>{n}b', f.read(n))))
    if tag_id == 8:
        n = struct.unpack('>H', f.read(2))[0]
        return f.read(n).decode('utf-8', 'replace')
    if tag_id == 9:
        inner, n = struct.unpack('>bi', f.read(5))
        return [read_tag(f, inner) for _ in range(max(n, 0))]
    if tag_id == 10:
        out = {}
        while True:
            t = f.read(1)[0]
            if t == 0:
                return out
            n = struct.unpack('>H', f.read(2))[0]
            name = f.read(n).decode('utf-8', 'replace')
            out[name] = read_tag(f, t)
    if tag_id == 11:
        n = struct.unpack('>i', f.read(4))[0]
        return ('I', list(struct.unpack(f'>{n}i', f.read(4 * n))))
    if tag_id == 12:
        n = struct.unpack('>i', f.read(4))[0]
        return ('L', list(struct.unpack(f'>{n}q', f.read(8 * n))))
    raise ValueError(f'bad tag id {tag_id}')


def read_named(data):
    f = io.BytesIO(data)
    t = f.read(1)[0]
    n = struct.unpack('>H', f.read(2))[0]
    f.read(n)
    return read_tag(f, t)


def read_dat(path):
    raw = open(path, 'rb').read()
    if raw[:2] == b'\x1f\x8b':
        raw = gzip.decompress(raw)
    return read_named(raw)


def region_chunks(path):
    data = open(path, 'rb').read()
    out = {}
    for i in range(1024):
        off = int.from_bytes(data[i * 4:i * 4 + 3], 'big')
        if off == 0:
            continue
        start = off * 4096
        length = struct.unpack('>i', data[start:start + 4])[0]
        codec = data[start + 4] & 0x7F
        payload = data[start + 5:start + 4 + length]
        if codec == 1:
            payload = gzip.decompress(payload)
        elif codec == 2:
            payload = zlib.decompress(payload)
        out[(i & 31, i >> 5)] = read_named(payload)
    return out


def plain(v, full, typed=False):
    if isinstance(v, tuple):
        kind, val = v
        if kind in 'bsldf':
            return f'{val}{kind}' if typed else val
        if not full and len(val) > 8:
            return f'{kind}[{len(val)}: {val[:4]}...]'
        return val
    if isinstance(v, dict):
        return {k: plain(x, full, typed) for k, x in v.items()}
    if isinstance(v, list):
        return [plain(x, full, typed) for x in v]
    return v


def select(v, path):
    for p in path:
        if isinstance(v, list):
            v = v[int(p)]
        elif isinstance(v, tuple):
            v = v[1][int(p)]
        else:
            v = v[p]
    return v


def main(argv):
    if '--help' in argv or '-h' in argv:
        print(USAGE)
        return 0
    full = '--full' in argv
    typed = '--typed' in argv
    argv = [a for a in argv if a not in ('--full', '--typed')]
    if not argv:
        print(USAGE)
        return 1
    path = argv[0]
    rest = argv[1:]
    if path.endswith('.mca'):
        chunks = region_chunks(path)
        if len(rest) >= 2 and rest[0].lstrip('-').isdigit():
            cx, cz = int(rest[0]) & 31, int(rest[1]) & 31
            value = chunks[(cx, cz)]
            rest = rest[2:]
        else:
            print(json.dumps(sorted(chunks)))
            return 0
    else:
        value = read_dat(path)
    print(json.dumps(plain(select(value, rest), full, typed), indent=1))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
