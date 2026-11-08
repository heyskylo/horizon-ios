#!/usr/bin/env python3
"""Put Microsoft's DXC (`dxcompiler.dll`) and its licence files into a Windows x64 game folder.

    python tools/ci/fetch_dxc.py dist/horizon-windows-x64

DirectX 12 compiles shaders with DXC on integrated GPUs (crates/horizon-client-render/src/gfx/dx12_compiler.rs,
#8513). The DLL comes from Microsoft's DirectX Shader Compiler release pinned below, checked against its
SHA-256 before and after unpacking; nothing is committed to the repository. Its licence (LICENSE-MS.txt, the
Windows SDK's Distributable Code terms) and the LLVM licence of its open-source part go beside it, with
packaging/windows/DXC-NOTICE.txt. THIRD_PARTY_NOTICES.md ("DirectX Shader Compiler") says why only x64 and why
`dxil.dll` is left out.
"""
import hashlib
import io
import sys
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
VERSION = 'v1.9.2609'
URL = ('https://github.com/microsoft/DirectXShaderCompiler/releases/download/'
       f'{VERSION}/dxc_2026_09_29.zip')
ZIP_SHA256 = 'ad31b1fc8443175d204f77a611fdb3ef2ec42759bdc2f1167368de24a4a7e7f1'
# What goes into the game folder: name there <- (path in the zip, its SHA-256; None = the zip's checksum covers it).
FILES = {
    'dxcompiler.dll': ('bin/x64/dxcompiler.dll',
                       '0689e2fee0a1c9df8662a59680469f24e2a3ad7ce6bbbcc9284f7a83172bfc59'),
    'DXC-LICENSE-MS.txt': ('LICENSE-MS.txt', None),
    'DXC-LICENSE-LLVM.txt': ('LICENSE-LLVM.txt', None),
}
NOTICE = ROOT / 'packaging/windows/DXC-NOTICE.txt'


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def unpack(archive: bytes, out: Path, zip_sha256: str = ZIP_SHA256, files=None) -> None:
    """Writes FILES from `archive` into `out`, after checking the archive's and each DLL's checksum."""
    files = FILES if files is None else files
    if sha256(archive) != zip_sha256:
        raise SystemExit(f'DXC archive: sha256 {sha256(archive)}, expected {zip_sha256}')
    with zipfile.ZipFile(io.BytesIO(archive)) as z:
        # Microsoft's zip names its entries with backslashes.
        entries = {name.replace('\\', '/'): name for name in z.namelist()}
        for target, (member, expected) in files.items():
            if member not in entries:
                raise SystemExit(f'DXC archive: no {member}')
            data = z.read(entries[member])
            if expected is not None and sha256(data) != expected:
                raise SystemExit(f'DXC archive: {member} sha256 {sha256(data)}, expected {expected}')
            (out / target).write_bytes(data)


def main(argv: list[str]) -> None:
    if len(argv) != 1:
        raise SystemExit(__doc__)
    out = Path(argv[0])
    if not out.is_dir():
        raise SystemExit(f'{out} is not a folder')
    with urllib.request.urlopen(URL, timeout=120) as response:
        archive = response.read()
    unpack(archive, out)
    (out / NOTICE.name).write_bytes(NOTICE.read_bytes())
    for name in [*FILES, NOTICE.name]:
        print(f'{out / name}: {(out / name).stat().st_size} bytes')


if __name__ == '__main__':
    main(sys.argv[1:])
