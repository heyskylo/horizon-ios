import hashlib
import importlib.util
import io
import plistlib
import struct
import tempfile
import unittest
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ios_release", ROOT / "tools/ci/ios_release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)
SHA = "a" * 40


def binary(signed=False):
    commands = struct.pack("<IIII", 0x1D, 16, 48, 8) if signed else b""
    return struct.pack("<IIIIIIII", 0xFEEDFACF, 0x0100000C, 0, 2,
                       int(signed), len(commands), 0, 0) + commands


class IOSReleaseTests(unittest.TestCase):
    def archive(self, path, extra=None, info=None, executable=None):
        metadata = {"CFBundleExecutable": "Horizon", "CFBundleShortVersionString": "0.1.0",
                    "CFBundleVersion": "42.2", "HorizonBuildCommit": SHA,
                    "CFBundleSupportedPlatforms": ["iPhoneOS"]}
        metadata.update(info or {})
        with zipfile.ZipFile(path, "w") as z:
            z.writestr("Payload/Horizon.app/Info.plist", plistlib.dumps(metadata))
            z.writestr("Payload/Horizon.app/Horizon", binary() if executable is None else executable)
            for name, data in (extra or {}).items():
                z.writestr(name, data)

    def test_real_metadata_names_and_checksum_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            ipa = root / "input.ipa"
            self.archive(ipa)
            result = release.prepare(ipa, root / "out", SHA)
            target = Path(result["ipa"])
            self.assertEqual(target.name, "Horizon-iOS-0.1.0-build42.2-unsigned.ipa")
            self.assertEqual(target.read_bytes(), ipa.read_bytes())
            self.assertEqual(Path(result["checksum"]).read_text(),
                             hashlib.sha256(ipa.read_bytes()).hexdigest() + "  " + target.name + "\n")

    def test_rejects_private_assets_profiles_and_paths(self):
        for name in ["horizon-resources.json", "horizon-data.bin", "embedded.mobileprovision",
                     "account.p12", "key.pem", ".env", "_CodeSignature/CodeResources",
                     "saves/level.dat", "servers.dat", "../outside", "/outside"]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                ipa = Path(directory) / "input.ipa"
                entry = name if name.startswith("/") else "Payload/Horizon.app/" + name
                self.archive(ipa, {entry: b"fixture"})
                with self.assertRaises(ValueError):
                    release.audit(ipa, SHA)

    def test_rejects_signed_main_and_nested_framework(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "input.ipa"
            self.archive(ipa, executable=binary(True))
            with self.assertRaisesRegex(ValueError, "signed binary"):
                release.audit(ipa, SHA)
            self.archive(ipa, {"Payload/Horizon.app/Frameworks/Kit.framework/Kit": binary(True)})
            with self.assertRaisesRegex(ValueError, "signed binary"):
                release.audit(ipa, SHA)

    def test_rejects_wrong_commit_and_unsafe_version(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "input.ipa"
            self.archive(ipa)
            with self.assertRaisesRegex(ValueError, "commit"):
                release.audit(ipa, "b" * 40)
            self.archive(ipa, info={"CFBundleVersion": "42\noutput=bad"})
            with self.assertRaisesRegex(ValueError, "version"):
                release.audit(ipa, SHA)

    def test_rejects_simulator_and_symlink_payloads(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "input.ipa"
            self.archive(ipa, info={"CFBundleSupportedPlatforms": ["iPhoneSimulator"]})
            with self.assertRaisesRegex(ValueError, "simulator"):
                release.audit(ipa, SHA)
            self.archive(ipa)
            with zipfile.ZipFile(ipa, "a") as z:
                link = zipfile.ZipInfo("Payload/Horizon.app/link")
                link.create_system = 3
                link.external_attr = 0o120777 << 16
                z.writestr(link, b"/outside")
            with self.assertRaisesRegex(ValueError, "symbolic link"):
                release.audit(ipa, SHA)

    def test_rejects_malformed_binary_instead_of_assuming_unsigned(self):
        for data in [binary()[:10], binary()[:16] + struct.pack("<II", 1, 0) + binary()[24:],
                     struct.pack("<IIIIIIII", 0xFEEDFACF, 0x0100000C, 0, 2, 1, 8, 0, 0)
                     + struct.pack("<II", 1, 0)]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                release.unsigned_macho(io.BytesIO(data))

class PublishFlowTests(unittest.TestCase):
    def run_flow(self, scenario, tag_sha=SHA):
        import json
        import os
        import subprocess
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            mock = root / "gh"
            mock.write_text('''#!/usr/bin/env python3
import json, os, sys
args=sys.argv[1:]
with open(os.environ["CALL_LOG"], "a") as f: f.write(json.dumps(args)+"\\n")
scenario=os.environ["SCENARIO"]
if args[0]=="api":
    if "matching-refs" in args[1]:
        if scenario=="wrong-tag": print("refs/tags/ios-build-42.2")
    else: print(os.environ["TAG_SHA"])
elif args[:2]==["release","view"]:
    if scenario in ["new", "upload-failed", "wrong-tag"]: sys.exit(1)
    names=["game.ipa", "game.ipa.sha256"] if scenario!="incomplete" else []
    print(json.dumps({"isDraft":scenario=="draft", "targetCommitish":os.environ["GITHUB_SHA"], "assets":[{"name":n} for n in names]}))
elif args[:2]==["release","upload"] and scenario=="upload-failed": sys.exit(1)
''')
            mock.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ["PATH"],
                       GH_REPO="example/repository", GITHUB_SHA=SHA, IPA="game.ipa",
                       CHECKSUM="game.ipa.sha256", VERSION="0.1.0", BUILD="42.2",
                       SCENARIO=scenario, TAG_SHA=tag_sha, CALL_LOG=str(root / "calls"))
            result = subprocess.run(["bash", str(ROOT / "tools/ci/publish-ios-release.sh")],
                                    cwd=root, env=env, capture_output=True, text=True)
            calls = [json.loads(line) for line in (root / "calls").read_text().splitlines()]
            return result, calls

    def test_new_release_publishes_only_after_upload(self):
        result, calls = self.run_flow("new")
        self.assertEqual(result.returncode, 0, result.stderr)
        actions = [c[1] for c in calls if c[0] == "release"]
        self.assertEqual(actions, ["view", "create", "upload", "edit"])
        create = next(c for c in calls if c[:2] == ["release", "create"])
        self.assertIn("--draft", create)
        result, calls = self.run_flow("upload-failed")
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse(any(c[:2] == ["release", "edit"] for c in calls))

    def test_draft_resume_and_published_assets_are_not_overwritten(self):
        result, calls = self.run_flow("draft")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(c[:2] == ["release", "create"] for c in calls))
        self.assertTrue(any(c[:2] == ["release", "edit"] for c in calls))
        result, calls = self.run_flow("published")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(c[:2] in [["release", "upload"], ["release", "edit"]] for c in calls))

    def test_wrong_tag_and_incomplete_published_release_fail_closed(self):
        for scenario in ["wrong-tag", "incomplete"]:
            result, calls = self.run_flow(scenario, "b" * 40)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(any(c[:2] in [["release", "upload"], ["release", "edit"]] for c in calls))


if __name__ == "__main__":
    unittest.main()
