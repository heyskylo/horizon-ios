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

    def test_workflow_versions_advance_but_retry_marketing_version_stays(self):
        for run, version in [(1, "0.1.44"), (88, "0.1.44"), (89, "0.1.45"),
                             (90, "0.1.46"), (91, "0.1.47")]:
            self.assertEqual(release.release_version(run, 1), (version, f"{run}.1"))
            self.assertEqual(release.release_version(run, 2), (version, f"{run}.2"))
        for run, attempt in [(0, 1), (88, 0), ("88\nkey=value", 1), ("x", 1)]:
            with self.assertRaises(ValueError):
                release.release_version(run, attempt)

    def test_stamp_updates_app_and_framework_preserving_other_fields(self):
        with tempfile.TemporaryDirectory() as directory:
            app = Path(directory) / "Game.app"
            framework = app / "Frameworks/Launcher.framework"
            framework.mkdir(parents=True)
            for parent in (app, framework):
                (parent / "Info.plist").write_bytes(plistlib.dumps({"CFBundleIdentifier": "keep"}))
            release.stamp_app(app, "91", "2", SHA, "public-client")
            for parent in (app, framework):
                info = plistlib.loads((parent / "Info.plist").read_bytes())
                self.assertEqual(info["CFBundleShortVersionString"], "0.1.47")
                self.assertEqual(info["CFBundleVersion"], "91.2")
                self.assertEqual(info["CFBundleIdentifier"], "keep")
            info = plistlib.loads((app / "Info.plist").read_bytes())
            self.assertEqual(info["HorizonBuildCommit"], SHA)
            self.assertEqual(info["HorizonMSAClientID"], "public-client")

    def test_framework_version_mismatch_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "input.ipa"
            self.archive(ipa, {"Payload/Horizon.app/Frameworks/Launcher.framework/Info.plist":
                              plistlib.dumps({"CFBundleShortVersionString": "0.1.0", "CFBundleVersion": "1"})})
            with self.assertRaisesRegex(ValueError, "Framework version"):
                release.audit(ipa, SHA)

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

    def test_main_must_be_an_executable_not_a_library(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "input.ipa"
            data = bytearray(binary())
            struct.pack_into("<I", data, 12, 6)
            self.archive(ipa, executable=data)
            with self.assertRaisesRegex(ValueError, "executable"):
                release.audit(ipa, SHA)

    def test_rejects_corrupt_payload_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            ipa = Path(directory) / "input.ipa"
            self.archive(ipa, {"Payload/Horizon.app/data.txt": b"A" * (1 << 15) + b"unchanged"})
            data = ipa.read_bytes().replace(b"unchanged", b"Xnchanged", 1)
            ipa.write_bytes(data)
            with self.assertRaisesRegex(ValueError, "corrupt"):
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
    elif args[1].endswith("/releases/latest"): print("ios-build-99.1" if scenario=="older" else "ios-build-41.1")
    else: print(os.environ["TAG_SHA"])
elif args[:2]==["release","view"]:
    if scenario in ["new", "upload-failed", "wrong-tag"]: sys.exit(1)
    names=["game.ipa", "game.ipa.sha256"] if scenario!="incomplete" else []
    if scenario=="legacy": names=["Horizon-iOS-0.1.0-build42.2-unsigned.ipa", "Horizon-iOS-0.1.0-build42.2-unsigned.ipa.sha256", "other.txt", "Horizon-iOS-0.1.0-build41.1-unsigned.ipa"]
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
        self.assertFalse(any(c[:2] == ["release", "upload"] for c in calls))
        edit = next(c for c in calls if c[:2] == ["release", "edit"])
        self.assertIn("--prerelease=false", edit)
        self.assertIn("--latest=true", edit)
        self.assertIn("Horizon iOS 0.1.0 — build 42.2", edit)

    def test_wrong_tag_fails_closed(self):
        for scenario in ["wrong-tag"]:
            result, calls = self.run_flow(scenario, "b" * 40)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(any(c[:2] in [["release", "upload"], ["release", "edit"]] for c in calls))

    def test_legacy_assets_repair_only_this_build_after_upload(self):
        result, calls = self.run_flow("legacy")
        self.assertEqual(result.returncode, 0, result.stderr)
        upload = next(i for i, c in enumerate(calls) if c[:2] == ["release", "upload"])
        deletes = [(i, c) for i, c in enumerate(calls) if c[:2] == ["release", "delete-asset"]]
        self.assertEqual(len(deletes), 2)
        self.assertTrue(all(i > upload and "build42.2-unsigned.ipa" in c[3] for i, c in deletes))
        result, calls = self.run_flow("incomplete")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(any(c[:2] == ["release", "upload"] for c in calls))

    def test_retry_of_older_run_does_not_replace_latest(self):
        result, calls = self.run_flow("older")
        self.assertEqual(result.returncode, 0, result.stderr)
        edit = next(c for c in calls if c[:2] == ["release", "edit"])
        self.assertIn("--latest=false", edit)


if __name__ == "__main__":
    unittest.main()
