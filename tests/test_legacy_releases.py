"""tools/ci/legacy_releases.py: the pinned SHA-256s of the releases published before signing began."""
import unittest

from tools.ci import legacy_releases as legacy


def release(tag, assets, draft=False):
    return {"tag_name": tag, "draft": draft, "assets": assets}


def asset(name, digest="sha256:" + "ab" * 32):
    return {"name": name, "digest": digest}


class TagKeyTests(unittest.TestCase):
    def test_the_numbers_order_by_value_not_by_spelling(self):
        self.assertLess(legacy.tag_key("v0.2.9"), legacy.tag_key("v0.2.10"))
        self.assertLess(legacy.tag_key("v0.9.0"), legacy.tag_key("v0.10.0"))

    def test_a_game_version_comes_before_a_launcher_version(self):
        # A game tag starts with "v"; a launcher tag is "launcher@x.y.z".
        self.assertLess(legacy.tag_key("v1.0.0"), legacy.tag_key("launcher@0.2.0"))

    def test_the_launcher_versions_keep_their_own_order(self):
        self.assertLess(legacy.tag_key("launcher@0.2.9"), legacy.tag_key("launcher@0.2.10"))


class EntriesTests(unittest.TestCase):
    def test_a_draft_is_left_out(self):
        lines = legacy.entries([release("v1.0.0", [asset("horizon.tar.gz")], draft=True)])
        self.assertEqual(lines, [])

    def test_a_signed_release_is_left_out(self):
        signed = release("v1.0.0", [asset("horizon.tar.gz"), asset("SHA256SUMS.sig")])
        self.assertEqual(legacy.entries([signed]), [])

    def test_an_unsigned_release_keeps_its_digest(self):
        lines = legacy.entries([release("v1.0.0", [asset("horizon.tar.gz")])])
        self.assertEqual(lines, [f"v1.0.0 horizon.tar.gz {'ab' * 32}"])

    def test_the_digest_is_written_without_its_prefix(self):
        lines = legacy.entries([release("v1.0.0", [asset("horizon.tar.gz", "sha256:" + "CD" * 32)])])
        self.assertEqual(lines, [f"v1.0.0 horizon.tar.gz {'cd' * 32}"])

    def test_an_asset_without_a_digest_is_refused(self):
        with self.assertRaises(SystemExit):
            legacy.entries([release("v1.0.0", [asset("horizon.tar.gz", "")])])

    def test_the_releases_come_out_in_order(self):
        releases = [release("v0.2.10", [asset("a")]), release("v0.2.9", [asset("b")])]
        self.assertEqual([line.split()[0] for line in legacy.entries(releases)], ["v0.2.9", "v0.2.10"])


class RenderTests(unittest.TestCase):
    def test_the_header_names_the_file_and_its_writer(self):
        text = legacy.render([])
        self.assertTrue(text.startswith("# Releases published before signing began"))
        self.assertIn("tools/ci/legacy_releases.py", text)

    def test_the_header_and_the_lines_are_both_written(self):
        text = legacy.render([release("v1.0.0", [asset("horizon.tar.gz")])])
        self.assertTrue(text.startswith(legacy.HEADER))
        self.assertIn("v1.0.0 horizon.tar.gz", text)
        self.assertTrue(text.endswith("\n"))


if __name__ == "__main__":
    unittest.main()
