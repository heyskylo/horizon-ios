"""Writes RGBA rows as a PNG with the standard library alone, so the glyphs need no image package."""
import struct
import zlib


def write_png(path, rows):
    h = len(rows); w = len(rows[0])
    raw = bytearray()
    for r in rows:
        raw.append(0)
        for px in r: raw += bytes(px)
    def ch(t, b):
        return struct.pack('>I', len(b)) + t + b + struct.pack('>I', zlib.crc32(t+b) & 0xffffffff)
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n' + ch(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) + ch(b'IDAT', zlib.compress(bytes(raw), 9)) + ch(b'IEND', b''))
