import hashlib
import io
from pathlib import Path
import tempfile
import unittest
import zipfile

from tools.ci.fetch_dxc import unpack

FILES = {
    'dxcompiler.dll': ('bin/x64/dxcompiler.dll', hashlib.sha256(b'x64 dll').hexdigest()),
    'DXC-LICENSE-MS.txt': ('LICENSE-MS.txt', None),
}


def archive(entries: dict[str, bytes]) -> bytes:
    out = io.BytesIO()
    with zipfile.ZipFile(out, 'w') as z:
        for name, data in entries.items():
            z.writestr(name, data)
    return out.getvalue()


class FetchDxcTests(unittest.TestCase):
    def unpack(self, data: bytes, zip_sha256: str | None = None) -> Path:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        out = Path(tmp.name)
        unpack(data, out, zip_sha256 or hashlib.sha256(data).hexdigest(), FILES)
        return out

    def test_the_x64_dll_and_licence_land_in_the_game_folder(self):
        # Microsoft's zip names entries with backslashes; the arm64 DLL stays out.
        data = archive({'bin\\x64\\dxcompiler.dll': b'x64 dll', 'bin\\arm64\\dxcompiler.dll': b'arm',
                        'LICENSE-MS.txt': b'terms'})
        out = self.unpack(data)
        self.assertEqual(sorted(p.name for p in out.iterdir()), ['DXC-LICENSE-MS.txt', 'dxcompiler.dll'])
        self.assertEqual((out / 'dxcompiler.dll').read_bytes(), b'x64 dll')

    def test_a_different_archive_is_refused(self):
        data = archive({'bin/x64/dxcompiler.dll': b'x64 dll', 'LICENSE-MS.txt': b'terms'})
        with self.assertRaises(SystemExit):
            self.unpack(data, zip_sha256='0' * 64)

    def test_a_different_dll_is_refused_even_in_a_matching_archive(self):
        data = archive({'bin/x64/dxcompiler.dll': b'another dll', 'LICENSE-MS.txt': b'terms'})
        with self.assertRaises(SystemExit):
            self.unpack(data)

    def test_a_missing_licence_is_refused(self):
        with self.assertRaises(SystemExit):
            self.unpack(archive({'bin/x64/dxcompiler.dll': b'x64 dll'}))


if __name__ == '__main__':
    unittest.main()
