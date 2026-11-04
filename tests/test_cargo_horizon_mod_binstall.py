"""`cargo binstall` finds every cargo-horizon-mod archive the release ships (#2120).

`[package.metadata.binstall]` and `.github/workflows/cargo-horizon-mod.yml` are two halves of one contract: the
manifest names a file per target, the workflow uploads it. These check the halves still fit, without a network.
"""

import tempfile
import unittest
from pathlib import Path

import tools.horizon_mod_binstall as binstall

class BinstallTests(unittest.TestCase):
    def test_the_manifest_names_one_archive_per_platform_in_this_release(self):
        for platform in binstall.PLATFORMS:
            with self.subTest(target=platform["target"]):
                url = binstall.url(platform["target"])
                self.assertIn(f"/releases/download/{binstall.tag()}/", url)
                self.assertTrue(url.endswith("/" + binstall.asset(platform["target"])))

    def test_a_placeholder_binstall_cannot_fill_fails_here(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory, "Cargo.toml")
            manifest.write_text(
                '[package]\nname = "cargo-horizon-mod"\nversion = "1.2.3"\n'
                '[package.metadata.binstall]\npkg-url = "https://example.invalid/{ name }-{ arch }.tar.gz"\n'
            )
            with self.assertRaises(KeyError):
                binstall.asset("x86_64-unknown-linux-gnu", manifest)

    def test_a_release_missing_an_archive_or_held_by_another_tag_is_reported(self):
        names = [binstall.asset(platform["target"]) for platform in binstall.PLATFORMS]
        self.assertEqual(len(names), len(set(names)), "two targets would overwrite one asset")
        with tempfile.TemporaryDirectory() as directory:
            for name in names:
                Path(directory, name).touch()
            self.assertEqual(binstall.check(directory, binstall.tag()), [])
            self.assertEqual(binstall.check(directory), [])

            renamed = Path(directory, names[0])
            renamed.rename(renamed.with_name("cargo-horizon-mod-mystery.tar.gz"))
            problems = binstall.check(directory)
            self.assertTrue(any(names[0] in problem for problem in problems), problems)
            self.assertTrue(any("mystery" in problem for problem in problems), problems)

            self.assertTrue(any("not" in problem for problem in binstall.check(directory, "cargo-horizon-mod-v9.9.9")))


if __name__ == "__main__":
    unittest.main()
