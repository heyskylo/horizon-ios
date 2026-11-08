#!/usr/bin/env python3
"""Compare two saved 26.x worlds of one seed: spawn, biomes, structure starts and blocks.

  world_compare.py WORLD_A WORLD_B [--noise WORLD_C] [--dimension overworld]

WORLD_A is the reference (a world vanilla 26.3 saved), WORLD_B the world Horizon saved from the same
seed; only the chunks both saved are compared. What a generator must match and what it need not:

- The spawn, the biome of every cell and every structure start must be identical (a chunk that has
  not got its biomes yet, vanilla's `structure_starts` status, is left out of the biomes).
- Terrain blocks (stone, deepslate, water, lava, the grass on top, the ores of blobs and veins) match
  to about 99.5%: what differs is decoration spilling across chunk borders, which is written in the
  order chunks happen to finish.
- Trees, sculk, moss and grass follow decoration order and match far less, even between two runs of
  vanilla itself. `--noise WORLD_C`, a second world one game saved from the same seed, shows that run-to-run
  floor beside the real comparison: a generator is as accurate as vanilla is with itself when
  A-vs-B is about as close as A-vs-C.

Make the worlds with `tools/perf_bench.py --seed N --scenes village --repeats 1 --seconds 3` for each
game (the world is .local/perf-bench/<game>/saves/PerfBench; copy it before the next run, which
deletes it), or with `tools/vanilla_world_check.py` for vanilla's.

Exit status 1 when the spawn, a biome or a structure start differs, or stone, deepslate or water
overlap under 99%. Standard library only.
"""
import argparse
import collections
import gzip
import os
import struct
import sys
import zlib

# The blocks reported: terrain first, then what decoration places.
WATCH = [
    'minecraft:stone', 'minecraft:deepslate', 'minecraft:water', 'minecraft:lava', 'minecraft:grass_block',
    'minecraft:sand', 'minecraft:gravel', 'minecraft:clay', 'minecraft:tuff', 'minecraft:granite',
    'minecraft:diorite', 'minecraft:coal_ore', 'minecraft:iron_ore', 'minecraft:copper_ore',
    'minecraft:oak_leaves', 'minecraft:oak_log', 'minecraft:dark_oak_leaves', 'minecraft:sculk',
    'minecraft:moss_block', 'minecraft:short_grass',
]
# What must overlap to pass.
TERRAIN = ['minecraft:stone', 'minecraft:deepslate', 'minecraft:water']
TERRAIN_FLOOR = 0.99
# Before these a chunk has no biomes of its own.
NO_BIOMES = {'minecraft:empty', 'minecraft:structure_starts', 'minecraft:structure_references'}
AIR = ('minecraft:air', ())


class Nbt:
    def __init__(self, data):
        self.d, self.p = data, 0

    def take(self, n):
        v = self.d[self.p:self.p + n]
        self.p += n
        return v

    def unpack(self, fmt):
        return struct.unpack(fmt, self.take(struct.calcsize(fmt)))[0]

    def string(self):
        return self.take(self.unpack('>H')).decode('utf-8', 'replace')

    def payload(self, t):
        if t in (1, 2, 3, 4, 5, 6):
            return self.unpack('>' + 'bhiqfd'[t - 1])
        if t == 7:
            return list(self.take(self.unpack('>i')))
        if t == 8:
            return self.string()
        if t == 9:
            item = self.unpack('>B')
            return [self.payload(item) for _ in range(self.unpack('>i'))]
        if t == 10:
            out = {}
            while True:
                item = self.unpack('>B')
                if item == 0:
                    return out
                name = self.string()
                out[name] = self.payload(item)
        if t == 11:
            return [self.unpack('>i') for _ in range(self.unpack('>i'))]
        if t == 12:
            return [self.unpack('>q') for _ in range(self.unpack('>i'))]
        raise ValueError('NBT tag %d' % t)


def parse_nbt(data):
    r = Nbt(data)
    t = r.unpack('>B')
    r.string()
    return r.payload(t)


def read_dat(path):
    return parse_nbt(gzip.decompress(open(path, 'rb').read()))


def world_info(world):
    level = read_dat(os.path.join(world, 'level.dat'))['Data']
    settings = os.path.join(world, 'data', 'minecraft', 'world_gen_settings.dat')
    seed = read_dat(settings)['data'].get('seed') if os.path.exists(settings) else None
    return seed, (level.get('spawn') or {}).get('pos')


def region_folder(world, dimension):
    base = os.path.join(world, 'dimensions', 'minecraft', dimension, 'region')
    return base if os.path.isdir(base) else os.path.join(world, 'region')


def read_chunks(folder):
    """{(chunk x, chunk z): chunk NBT} of every chunk saved in the region files of `folder`."""
    out = {}
    if not os.path.isdir(folder):
        return out
    for name in sorted(os.listdir(folder)):
        if not name.endswith('.mca'):
            continue
        _, rx, rz, _ = name.split('.')
        data = open(os.path.join(folder, name), 'rb').read()
        for i in range(1024):
            sector = int.from_bytes(data[4 * i:4 * i + 3], 'big')
            if sector == 0:
                continue
            at = sector * 4096
            length = struct.unpack('>I', data[at:at + 4])[0]
            kind, body = data[at + 4], data[at + 5:at + 4 + length]
            if kind == 2:
                body = zlib.decompress(body)
            elif kind != 3:
                continue
            out[(int(rx) * 32 + i % 32, int(rz) * 32 + i // 32)] = parse_nbt(body)
    return out


def entry(p):
    """(name, properties) of a palette entry, as 26.x writes it: a bare name in a compound, or an id."""
    if isinstance(p, str):
        return (p, ())
    props = p.get('properties') or p.get('Properties') or {}
    return (p.get('id') or p.get('') or p.get('Name'), tuple(sorted(props.items())))


def unpack_bits(longs, bits, count):
    per = 64 // bits
    mask = (1 << bits) - 1
    return [((longs[i // per] & 0xFFFFFFFFFFFFFFFF) >> ((i % per) * bits)) & mask for i in range(count)]


def container(c, count, min_bits):
    """The values of a paletted container (block states or biomes) as entries."""
    if not c:
        return None
    palette = [entry(p) for p in c['palette']]
    data = c.get('data')
    if len(palette) == 1 or data is None:
        return [palette[0]] * count
    bits = max(min_bits, (len(palette) - 1).bit_length())
    return [palette[i] for i in unpack_bits(data, bits, count)]


def blocks(chunk):
    out = {}
    for sec in chunk.get('sections', []):
        values = container(sec.get('block_states'), 4096, 4)
        if values is not None:
            out[sec['Y']] = [v[0] for v in values]
    return out


def biomes(chunk):
    out = {}
    for sec in chunk.get('sections', []):
        values = container(sec.get('biomes'), 64, 1)
        if values is not None:
            out[sec['Y']] = [v[0] for v in values]
    return out


def structure_starts(chunk):
    starts = (chunk.get('structures') or {}).get('starts') or {}
    return {(n, s.get('ChunkX'), s.get('ChunkZ')) for n, s in starts.items() if isinstance(s, dict) and s.get('id') != 'INVALID'}


def full_pairs(a, b):
    return [k for k in sorted(set(a) & set(b)) if a[k].get('Status') == 'minecraft:full' == b[k].get('Status')]


def overlap(a, b):
    """{block: (both, only in a, only in b)} over the chunks both saved as full."""
    count = {w: [0, 0, 0] for w in WATCH}
    for key in full_pairs(a, b):
        sa, sb = blocks(a[key]), blocks(b[key])
        for y in set(sa) & set(sb):
            for x, z in zip(sa[y], sb[y]):
                if x == z:
                    if x in count:
                        count[x][0] += 1
                else:
                    if x in count:
                        count[x][1] += 1
                    if z in count:
                        count[z][2] += 1
    return {w: tuple(v) for w, v in count.items()}


def share(v):
    both, a, b = v
    return both / (both + a + b) if both + a + b else None


def compare_biomes(a, b):
    chunks = same = cells = wrong = 0
    for key in sorted(set(a) & set(b)):
        if a[key].get('Status') in NO_BIOMES or b[key].get('Status') in NO_BIOMES:
            continue
        ba, bb = biomes(a[key]), biomes(b[key])
        if not ba or not bb:
            continue
        chunks += 1
        d = sum(x != z for y in set(ba) & set(bb) for x, z in zip(ba[y], bb[y]))
        cells += 64 * len(set(ba) & set(bb))
        wrong += d
        same += d == 0
    return chunks, same, cells, wrong


def compare_structures(a, b):
    sa, sb = set(), set()
    for key in set(a) & set(b):
        sa |= structure_starts(a[key])
        sb |= structure_starts(b[key])
    return sa, sb


def percent(x):
    return '   n/a' if x is None else '%5.1f%%' % (100 * x)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('a', help='the reference world (vanilla)')
    p.add_argument('b', help='the world to check (Horizon)')
    p.add_argument('--noise', help="a second world of A's game and seed: the floor between two runs of one game")
    p.add_argument('--dimension', default='overworld')
    args = p.parse_args()
    ok = True

    worlds = {'A': args.a, 'B': args.b}
    if args.noise:
        worlds['C'] = args.noise
    info = {k: world_info(w) for k, w in worlds.items()}
    for k, (seed, spawn) in info.items():
        print('%s: seed %s, spawn %s  (%s)' % (k, seed, spawn, worlds[k]))
    if info['A'][0] != info['B'][0] or info['A'][1] != info['B'][1]:
        print('MISMATCH: the seed or the spawn differs between A and B')
        ok = False
    chunks = {k: read_chunks(region_folder(w, args.dimension)) for k, w in worlds.items()}
    print('chunks saved: ' + ', '.join('%s %d' % (k, len(c)) for k, c in chunks.items()))
    print('chunks A and B both saved: %d, as full: %d' % (len(set(chunks['A']) & set(chunks['B'])), len(full_pairs(chunks['A'], chunks['B']))))

    ab = overlap(chunks['A'], chunks['B'])
    ac = overlap(chunks['A'], chunks['C']) if 'C' in chunks else None
    print()
    print('Blocks in the chunks both saved as full, share of the positions holding the block in either that hold it in both:')
    print('  %-26s %8s%s' % ('', 'A vs B', '   A vs C (the floor)' if ac else ''))
    for w in WATCH:
        print('  %-26s %8s%s' % (w, percent(share(ab[w])), '   %8s' % percent(share(ac[w])) if ac else ''))
    for w in TERRAIN:
        s = share(ab[w])
        if s is not None and s < TERRAIN_FLOOR:
            print('MISMATCH: %s overlaps %.1f%%, under %.0f%%' % (w, 100 * s, 100 * TERRAIN_FLOOR))
            ok = False

    n, same, cells, wrong = compare_biomes(chunks['A'], chunks['B'])
    print()
    print('Biomes: %d chunks with biomes in both, %d identical, %d of %d cells differ' % (n, same, wrong, cells))
    if wrong:
        ok = False
    sa, sb = compare_structures(chunks['A'], chunks['B'])
    print('Structure starts in the common chunks: A %d, B %d, in both %d' % (len(sa), len(sb), len(sa & sb)))
    kinds = collections.Counter(s[0] for s in sa | sb)
    for name, count in kinds.most_common():
        print('  %-36s in both %d' % (name, sum(1 for s in sa & sb if s[0] == name)))
    if sa != sb:
        print('MISMATCH: only in A %s, only in B %s' % (sorted(sa - sb)[:4], sorted(sb - sa)[:4]))
        ok = False
    print()
    print('RESULT:', 'matches' if ok else 'DIFFERS')
    return 0 if ok else 1


if __name__ == '__main__':
    sys.exit(main())
