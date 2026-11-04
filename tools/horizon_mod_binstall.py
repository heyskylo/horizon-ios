"""Where `cargo binstall` finds `cargo-horizon-mod`'s prebuilt archives (#2120).

`[package.metadata.binstall]` in `crates/cargo-horizon-mod/Cargo.toml` names one release asset per Rust
target, and `.github/workflows/cargo-horizon-mod.yml` builds [`PLATFORMS`] and attaches them to the tag
[`tag`]. Both sides read this file, so renaming an archive here cannot leave `cargo binstall` asking for a
file no release holds.
"""

import re
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / "crates/cargo-horizon-mod/Cargo.toml"

# One entry per archive the release holds: the Rust target, the runner that builds it, and whether that
# runner can start the binary (a cross build cannot).
PLATFORMS = [
    {"target": "x86_64-unknown-linux-gnu", "os": "ubuntu-22.04", "run": True},
    {"target": "aarch64-unknown-linux-gnu", "os": "ubuntu-22.04", "run": False},
    {"target": "x86_64-pc-windows-msvc", "os": "windows-latest", "run": True},
    {"target": "x86_64-apple-darwin", "os": "macos-latest", "run": True},
    {"target": "aarch64-apple-darwin", "os": "macos-latest", "run": True},
]

PLACEHOLDER = re.compile(r"\{\s*([a-z-]+)\s*\}")


def _package(manifest=MANIFEST):
    with open(manifest, "rb") as handle:
        return tomllib.load(handle)["package"]


def version(manifest=MANIFEST):
    return _package(manifest)["version"]


def tag(manifest=MANIFEST):
    """The GitHub release holding the archives: `cargo-horizon-mod-v<version>`."""
    return f"cargo-horizon-mod-v{version(manifest)}"


def url(target, manifest=MANIFEST):
    """The download URL `binstall` builds for `target`, from the manifest's `pkg-url`.

    Raises `KeyError` for a placeholder `binstall` would not fill in, so a typo fails here instead of at
    every mod developer's download.
    """
    template = _package(manifest)["metadata"]["binstall"]["pkg-url"]
    fields = {
        "name": "cargo-horizon-mod",
        "version": version(manifest),
        "target": target,
        "binary-ext": ".exe" if "windows" in target else "",
        "archive-format": "tar.gz",
    }

    def fill(match):
        return fields[match.group(1)]

    return PLACEHOLDER.sub(fill, template)


def asset(target, manifest=MANIFEST):
    """The archive `binstall` downloads for `target`: the file name of its download URL."""
    return url(target, manifest).rsplit("/", 1)[-1]


def check(directory, expected_tag=None):
    """What is wrong with the archives in `directory` for this manifest: `[]` when binstall finds them all."""
    problems = []
    if expected_tag is not None and expected_tag != tag():
        problems.append(f"the tag is {expected_tag}, not {tag()}")
    wanted = {asset(platform["target"]) for platform in PLATFORMS}
    found = {path.name for path in Path(directory).glob("*.tar.gz")}
    problems += [f"the release has no {name}" for name in sorted(wanted - found)]
    problems += [f"{name} is not an archive binstall asks for" for name in sorted(found - wanted)]
    return problems


def main(argv):
    directory = argv[0] if argv else "dist"
    problems = check(directory, argv[1] if len(argv) > 1 else None)
    for problem in problems:
        print(f"::error::{problem}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
