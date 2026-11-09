"""Execute release shell steps with optional archives present and absent."""

import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
REQUIRED = {
    "horizon-linux-x86_64.tar.gz", "horizon-windows-x64.zip", "horizon-macos-arm64.zip",
    "horizon-macos-x64.zip", "horizon-server-linux-x86_64.tar.gz", "horizon-source.tar.gz",
}
OPTIONAL = ("horizon-windows-arm64.zip", "horizon-linux-musl-x86_64.tar.gz")


def run_step(containing):
    workflow = (ROOT / ".github/workflows/nightly-build.yml").read_text()
    for match in re.finditer(r"^        run: \|\n((?:          .*\n|\n)+)", workflow, re.MULTILINE):
        script = "\n".join(line[10:] for line in match[1].splitlines())
        if containing in script:
            return script
    raise AssertionError(f"no workflow step containing {containing}")


@unittest.skipUnless(shutil.which("bash") and shutil.which("openssl") and shutil.which("sha256sum"),
                     "release shell checks require bash, OpenSSL and sha256sum")
class OptionalReleaseAssetsTests(unittest.TestCase):
    def test_sign_and_publish_with_each_optional_archive_combination(self):
        for mask in range(4):
            with self.subTest(mask=mask), tempfile.TemporaryDirectory() as folder:
                root = Path(folder)
                dist = root / "dist"
                dist.mkdir()
                present = REQUIRED | {name for i, name in enumerate(OPTIONAL) if mask & (1 << i)}
                for name in present:
                    (dist / name).write_text(name)
                script_dir = root / "tools/ci"
                script_dir.mkdir(parents=True)
                shutil.copy(ROOT / "tools/ci/sign_release.sh", script_dir)
                # The steps' own env from nightly-build.yml: SHORT is the commit the title and
                # SHA256SUMS.v2.sig name (ALSO_FOR, nightly-<date>-<short>).
                env = dict(os.environ, EPHEMERAL="yes", SHA="0123456789abcdef", SHORT="0123456", DATE="2026-11-04",
                           ALSO_FOR="nightly-2026-11-04-0123456", SOURCE_TAG="nightly-test",
                           RELEASE_SIGNING_KEY="", RELEASE_SIGNING_KEY_NEXT="")
                signed = subprocess.run(["bash", "-e", "-c", run_step("sign_release.sh nightly")],
                                        cwd=root, env=env, text=True, capture_output=True)
                self.assertEqual(signed.returncode, 0, signed.stderr)
                # `name` or `*name`: sha256sum on Windows (Git Bash) marks binary mode with `*`.
                sums = {name.removeprefix("*"): digest for digest, name in
                        (line.split() for line in (dist / "SHA256SUMS").read_text().splitlines())}
                self.assertEqual(sums, {name: hashlib.sha256(name.encode()).hexdigest() for name in present})
                self.assertTrue((dist / "SHA256SUMS.sig").stat().st_size)
                self.assertTrue((dist / "SHA256SUMS.v2.sig").stat().st_size)
                (dist / "smoke-test.json").write_text("{}")
                (dist / "nightly-notes.md").write_text("test")
                # Isolate GitHub and tag rotation; validate the exact attachment arguments
                # produced by the real workflow, including paths with display labels.
                stubs = '''
python3() { :; }
gh() {
  for arg in "$@"; do
    case "$arg" in dist/*)
      file=${arg%%#*}
      test -f "$file" || return 1
      printf '%s\\n' "$file" >> published
    esac
  done
}
'''
                published = subprocess.run(["bash", "-e", "-c", stubs + run_step("gh release create nightly")],
                                           cwd=root, env=env, text=True, capture_output=True)
                self.assertEqual(published.returncode, 0, published.stderr)
                files = {Path(name).name for name in (root / "published").read_text().splitlines()}
                self.assertEqual(files, present | {"SHA256SUMS", "SHA256SUMS.sig", "SHA256SUMS.v2.sig", "smoke-test.json",
                                                   "nightly-notes.md"})
                for name in set(OPTIONAL) - present:
                    self.assertIn(f"::warning::{name} is missing", signed.stdout)
                    self.assertIn(f"::warning::{name} is missing", published.stdout)
