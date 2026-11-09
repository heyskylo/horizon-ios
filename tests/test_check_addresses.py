import unittest

from tools import check_addresses


def strings(source):
    """The string literals a source file keeps once its #[cfg(test)] items are dropped."""
    literals, mask = check_addresses.tokenize(source)
    ranges, files = check_addresses.test_items(source, mask)
    return [s for offset, s in literals if not any(a <= offset < b for a, b in ranges)], files


CONFIG = {
    "homes": ["crates/horizon-addresses/src/lib.rs"],
    "vanilla": {"textures.minecraft.net": "skins"},
    "not_addresses": {"ddns.net": "a suffix"},
    "allowed": ["crates/horizon-telemetry/src/lib.rs telemetry.playhorizon.dev"],
}


class HostsTests(unittest.TestCase):
    def test_urls_and_whole_host_strings(self):
        self.assertEqual(check_addresses.hosts("https://cosmetics.playhorizon.dev/v1/presence"),
                         ["cosmetics.playhorizon.dev"])
        self.assertEqual(check_addresses.hosts("stun.l.google.com:19302"), ["stun.l.google.com"])
        self.assertEqual(check_addresses.hosts("wss://user:pw@relay.playhorizon.dev:443/x"),
                         ["relay.playhorizon.dev"])

    def test_text_translation_keys_and_placeholders_are_not_hosts(self):
        for text in ["Download it at playhorizon.dev, then press Play.", "options.online", "mco.info",
                     "https://example.com/a", "https://foo.test", "http://127.0.0.1:8080", "http://192.168.1.1/",
                     "https://map.example.net", "http://192.0.2.7:8100", "vertex.xyz * 2.0"]:
            self.assertEqual(check_addresses.hosts(text), [], text)

    def test_public_ip_in_a_url_counts(self):
        self.assertEqual(check_addresses.hosts("http://8.8.8.8/"), ["8.8.8.8"])


class TokenizeTests(unittest.TestCase):
    def test_comments_are_not_strings(self):
        found, _ = strings('// "https://a.com"\n/* "https://b.com" */ let x = "https://c.com";')
        self.assertEqual(found, ["https://c.com"])

    def test_raw_strings_char_literals_and_lifetimes(self):
        found, _ = strings("fn f<'a>(c: char) { if c == '\"' {} let r = r#\"x \"q\" y\"#; let s = \"z\"; }")
        self.assertEqual(found, ['x "q" y', "z"])

    def test_cfg_test_modules_are_skipped_whatever_their_strings_hold(self):
        source = ('const A: &str = "https://a.com";\n#[cfg(test)]\nmod tests {\n    const R: &str = r#"\n}\n"#;\n'
                  '    const B: &str = "https://b.com";\n}\nconst C: &str = "https://c.com";\n')
        found, _ = strings(source)
        self.assertEqual(found, ["https://a.com", "https://c.com"])

    def test_cfg_test_files_are_named(self):
        _, files = strings('#[cfg(test)]\nmod tests;\n#[cfg(test)]\n#[path = "fetch_tests.rs"]\nmod fetch_tests;\n')
        self.assertEqual(files, ["tests", "fetch_tests.rs"])

    def test_other_cfg_test_items_are_skipped(self):
        found, _ = strings('#[cfg(test)]\nconst T: [&str; 1] = ["https://t.com"];\nconst U: &str = "https://u.com";')
        self.assertEqual(found, ["https://u.com"])


class CheckTests(unittest.TestCase):
    def run_check(self, found, base=None):
        return check_addresses.check(CONFIG, set(found), base)

    def test_home_vanilla_not_addresses_and_allowed_pass(self):
        found = [("crates/horizon-addresses/src/lib.rs", "cosmetics.playhorizon.dev"),
                 ("crates/horizon-client-render/src/skins.rs", "textures.minecraft.net"),
                 ("crates/horizon-client-render/src/bluemap_detect.rs", "ddns.net"),
                 ("crates/horizon-telemetry/src/lib.rs", "telemetry.playhorizon.dev")]
        self.assertEqual(self.run_check(found), ([], []))

    def test_a_new_address_outside_the_home_fails_and_names_the_doc(self):
        problems, _ = self.run_check([("crates/horizon-client/src/net.rs", "relay.playhorizon.dev")])
        self.assertIn("relay.playhorizon.dev", problems[0])
        self.assertIn("docs/layers.md#built-in-addresses", problems[0])

    def test_an_address_that_left_its_file_is_a_note(self):
        problems, notes = self.run_check([])
        self.assertEqual(problems, [])
        self.assertIn("telemetry.playhorizon.dev", notes[0])

    def test_the_list_may_not_take_new_hosts_or_grow(self):
        config = dict(CONFIG, allowed=CONFIG["allowed"] + ["crates/x/src/lib.rs relay.playhorizon.dev"])
        problems, _ = check_addresses.check(config, set(), {"allowed": CONFIG["allowed"]})
        self.assertTrue(any("relay.playhorizon.dev: added" in p for p in problems))
        self.assertTrue(any("grew from 1 to 2" in p for p in problems))

    def test_an_entry_may_move_with_its_code(self):
        moved = {"allowed": ["crates/horizon-telemetry/src/upload.rs telemetry.playhorizon.dev"]}
        config = dict(CONFIG, allowed=moved["allowed"])
        found = {("crates/horizon-telemetry/src/upload.rs", "telemetry.playhorizon.dev")}
        self.assertEqual(check_addresses.check(config, found, {"allowed": CONFIG["allowed"]}), ([], []))


if __name__ == "__main__":
    unittest.main()
