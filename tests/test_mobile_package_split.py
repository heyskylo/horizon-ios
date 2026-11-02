"""The shared client-library split must preserve native mobile build roots."""
from pathlib import Path
import tomllib
import unittest

ROOT = Path(__file__).resolve().parents[1]


def manifest(name):
    return tomllib.loads((ROOT / 'crates' / name / 'Cargo.toml').read_text())


class MobilePackageSplitTests(unittest.TestCase):
    def test_android_compiles_gameplay_root_with_native_activity(self):
        android = manifest('horizon-android')
        root = (ROOT / 'crates/horizon-android' / android['lib']['path']).resolve()
        self.assertEqual(root, ROOT / 'crates/horizon-client/src/lib.rs')
        self.assertEqual(android['lib']['crate-type'], ['cdylib'])
        self.assertNotIn('horizon-client', android['dependencies'])
        self.assertFalse(android['dependencies']['horizon-client-lib']['default-features'])
        self.assertIn('android-native-activity', android['dependencies']['winit']['features'])
        self.assertIn('horizon-client-lib/audio', android['features']['audio'])
        self.assertIn('horizon-client-render/audio', android['features']['audio'])
        for patch in ['winit', 'naga', 'wgpu-hal', 'java_regex']:
            self.assertIn(patch, android['patch']['crates-io'])

    def test_reusable_mobile_dependencies_and_apple_backend_gate(self):
        library = manifest('horizon-client-lib')
        deps = library['dependencies']
        self.assertIn('stack-growth', deps['java_regex']['features'])
        self.assertEqual(deps['unicode_names2'], '=2.0.0')
        self.assertIn('horizon-ios-launcher', deps)
        self.assertIn('cfg(not(target_vendor = "apple"))', library['target'])
        self.assertNotIn('cfg(not(target_os = "macos"))', library['target'])
        game = manifest('horizon-client')
        self.assertIn('horizon-client-lib/audio', game['features']['audio'])
        self.assertIn('horizon-client-lib/controllers', game['features']['controllers'])
        binaries = {target['name'] for target in library['bin']}
        self.assertIn('horizon-ctl', binaries)
