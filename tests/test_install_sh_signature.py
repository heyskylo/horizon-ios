"""Runs tests/install_sh_signature.sh: packaging/server/install.sh checks the release signature.

PURPOSE: keep install.sh's Ed25519 check from regressing (docs/release-flow.md, "Signed checksums").
REFS: tests/install_sh_signature.sh, tools/ci/sign_release.sh.
Skips quietly without bash or OpenSSL 3. Headless, no network.
"""

import shutil
import subprocess
import unittest
from pathlib import Path

SCRIPT = Path(__file__).with_name("install_sh_signature.sh")


class InstallShSignature(unittest.TestCase):
    @unittest.skipUnless(shutil.which("bash") and shutil.which("openssl"), "bash and openssl needed")
    def test_signature_cases(self):
        run = subprocess.run(["bash", str(SCRIPT)], capture_output=True, text=True, timeout=300)
        if run.returncode == 77:
            self.skipTest(run.stdout.strip())
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)


if __name__ == "__main__":
    unittest.main()
