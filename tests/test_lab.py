import hashlib
import io
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest import mock
import zipfile

from tools import check_public_tree, lab


def minimal_class(source="Example.java"):
    # A valid class-file structure with no fields/methods and one SourceFile attribute.
    def utf8(value):
        data = value.encode()
        return b"\x01" + struct.pack(">H", len(data)) + data
    return (b"\xca\xfe\xba\xbe" + struct.pack(">HHH", 0, 69, 5)
            + utf8("SourceFile") + utf8(source) + utf8("Example") + b"\x07\x00\x03"
            + struct.pack(">HHHHHHHHIH", 1, 4, 0, 0, 0, 0, 1, 1, 2, 2))


class ArtifactTests(unittest.TestCase):
    def test_download_rejects_corruption_without_overwriting_existing_file(self):
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "game.jar"
            destination.write_bytes(b"existing")
            with mock.patch("urllib.request.urlopen", return_value=io.BytesIO(b"corrupt")):
                with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                    lab.download("https://example.invalid/game.jar", destination, "0" * 40)
            self.assertEqual(destination.read_bytes(), b"existing")
            self.assertEqual([p.name for p in Path(temp).iterdir()], ["game.jar"])

    def test_cached_download_is_verified(self):
        with tempfile.TemporaryDirectory() as temp:
            destination = Path(temp) / "game.jar"
            destination.write_bytes(b"verified")
            with mock.patch("urllib.request.urlopen") as request:
                lab.download("https://example.invalid", destination, hashlib.sha1(b"verified").hexdigest())
                request.assert_not_called()

    def test_import_copies_only_the_pinned_file_from_the_users_install(self):
        with tempfile.TemporaryDirectory() as temp:
            source, destination = Path(temp) / "own.jar", Path(temp) / "cache" / "client.jar"
            source.write_bytes(b"modified")
            with self.assertRaisesRegex(ValueError, "not the pinned file"):
                lab.import_copy(source, destination, hashlib.sha1(b"vanilla").hexdigest())
            self.assertFalse(destination.exists())
            source.write_bytes(b"vanilla")
            lab.import_copy(source, destination, hashlib.sha1(b"vanilla").hexdigest())
            self.assertEqual(destination.read_bytes(), b"vanilla")
            with self.assertRaisesRegex(FileNotFoundError, "official launcher"):
                lab.import_copy(Path(temp) / "absent.jar", Path(temp) / "other.jar", "0" * 40,
                                hint=lab.LAUNCHER_HINT.format(version="26.3"))

    def test_game_files_come_from_the_launcher_install_without_network(self):
        client, server = b"client", b"server"
        lock = {"minecraft": {"version": "26.3", "manifest_sha1": "0" * 40, "java_major": 25,
                              "client_sha1": hashlib.sha1(client).hexdigest(),
                              "server_sha1": hashlib.sha1(server).hexdigest()}}
        manifest = {"id": "26.3", "javaVersion": {"majorVersion": 25},
                    "downloads": {"client": {"sha1": lock["minecraft"]["client_sha1"]},
                                  "server": {"sha1": lock["minecraft"]["server_sha1"]}}}
        with tempfile.TemporaryDirectory() as temp, mock.patch("urllib.request.urlopen") as request, \
                mock.patch.dict("os.environ", {}, clear=False) as environment:
            environment.pop("HORIZON_SERVER_JAR", None)
            launcher, cache = Path(temp) / "minecraft", Path(temp) / "cache"
            version = launcher / "versions" / "26.3"
            version.mkdir(parents=True)
            (version / "26.3.json").write_text(json.dumps(manifest))
            (version / "26.3.jar").write_bytes(client)
            lock["minecraft"]["manifest_sha1"] = lab.digest(version / "26.3.json", "sha1")
            loaded, jar = lab.import_client(lock, cache, launcher)
            self.assertEqual((loaded["id"], jar.read_bytes()), ("26.3", client))
            with self.assertRaisesRegex(FileNotFoundError, "server jar yourself"):
                lab.import_server(lock, cache)
            (Path(temp) / "server.jar").write_bytes(server)
            self.assertEqual(lab.import_server(lock, cache, Path(temp) / "server.jar").read_bytes(), server)
            self.assertEqual(lab.import_server(lock, cache).read_bytes(), server)
            request.assert_not_called()

    def test_launcher_manifest_for_another_release_is_rejected(self):
        lock = {"minecraft": {"version": "26.3", "manifest_sha1": "0" * 40, "java_major": 25,
                              "client_sha1": "1" * 40, "server_sha1": "2" * 40}}
        with tempfile.TemporaryDirectory() as temp:
            version = Path(temp) / "versions" / "26.3"
            version.mkdir(parents=True)
            (version / "26.3.json").write_text(json.dumps({"id": "26.3", "downloads": {
                "client": {"sha1": "3" * 40}, "server": {"sha1": "2" * 40}}}))
            with self.assertRaisesRegex(ValueError, "pinned 26.3 release"):
                lab.import_manifest(lock, Path(temp) / "cache", Path(temp))

    def test_republished_launcher_manifests_are_accepted_and_unknown_ones_rejected(self):
        manifest = {"id": "26.3", "javaVersion": {"majorVersion": 25}, "assetIndex": {"sha1": "a"},
                    "downloads": {"client": {"sha1": "1" * 40}, "server": {"sha1": "2" * 40}}}
        older = json.dumps({**manifest, "assetIndex": {"sha1": "b"}})
        lock = {"minecraft": {"version": "26.3", "java_major": 25, "client_sha1": "1" * 40, "server_sha1": "2" * 40,
                              "manifest_sha1": hashlib.sha1(json.dumps(manifest).encode()).hexdigest(),
                              "accepted_manifest_sha1": [hashlib.sha1(older.encode()).hexdigest()]}}
        with tempfile.TemporaryDirectory() as temp:
            launcher, cache = Path(temp) / "minecraft", Path(temp) / "cache"
            source = launcher / "versions" / "26.3" / "26.3.json"
            source.parent.mkdir(parents=True)
            source.write_text(older)
            self.assertEqual(lab.import_manifest(lock, cache, launcher)["assetIndex"], {"sha1": "b"})
            # The launcher's newer copy replaces the cached one, keeping it in step with its asset index.
            source.write_text(json.dumps(manifest))
            self.assertEqual(lab.import_manifest(lock, cache, launcher)["assetIndex"], {"sha1": "a"})
            source.unlink()
            self.assertEqual(lab.import_manifest(lock, cache, launcher)["assetIndex"], {"sha1": "a"})
            source.write_text(json.dumps({**manifest, "assetIndex": {"sha1": "c"}}))
            with self.assertRaisesRegex(ValueError, "does not know yet"):
                lab.import_manifest(lock, cache, launcher)


    def test_sourcefile_metadata_covers_inner_classes(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "game.jar"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("demo/Example.class", minimal_class())
                archive.writestr("demo/Example$Inner.class", minimal_class())
            result = lab.inventory(path)
            self.assertEqual(result["class_count"], 2)
            self.assertEqual(result["expected_source_files"], ["demo/Example.java"])

    def test_invalid_class_is_rejected(self):
        for data in [b"oops", b"\xca\xfe\xba\xbe"]:
            with self.assertRaises(ValueError):
                lab.ClassReader(data).metadata()

    def test_server_bundler_extracts_actual_game_and_checks_checksum(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            path = directory / "server.jar"
            game = b"inner game jar"
            library = b"dependency"
            with zipfile.ZipFile(path, "w") as archive:
                archive.writestr("META-INF/versions.list", f"{hashlib.sha256(game).hexdigest()}\t26.3\t26.3/game.jar\n")
                archive.writestr("META-INF/versions/26.3/game.jar", game)
                archive.writestr("META-INF/libraries.list", f"{hashlib.sha256(library).hexdigest()}\tdemo:lib:1\tdemo/lib.jar\n")
                archive.writestr("META-INF/libraries/demo/lib.jar", library)
            extracted, libraries = lab.unpack_server(path, directory, "26.3")
            self.assertEqual(extracted.read_bytes(), game)
            self.assertEqual(libraries[0].read_bytes(), library)
            with self.assertRaises(ValueError):
                lab.unpack_server(path, directory, "wrong-version")

    def test_server_bundler_rejects_path_traversal(self):
        for relative in ["../../escape.jar", "/escape.jar", "..\\escape.jar"]:
            with zipfile.ZipFile(io.BytesIO(), "w") as archive:
                archive.writestr("entries", f"{'0' * 64}\tdemo\t{relative}")
                with self.assertRaisesRegex(ValueError, "Unsafe bundled path"):
                    list(lab.bundle_entries(archive, "entries"))

    def test_coverage_reports_missing_files_and_failed_method_markers(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            lab.write_json(directory / "reports" / "client-inventory.json", {
                "class_count": 3, "expected_source_files": ["demo/Example.java", "demo/Missing.java"]
            })
            sources = directory / "sources" / "client" / "demo"
            sources.mkdir(parents=True)
            (sources / "Example.java").write_text("// $VF: Couldn't be decompiled\n")
            report = lab.coverage(directory, "client")
            self.assertEqual(report["missing_source_files"], ["demo/Missing.java"])
            self.assertEqual(report["files_with_decompiler_error_markers"], ["demo/Example.java"])


class PublicationTests(unittest.TestCase):
    def test_private_paths_rejected(self):
        for path in [".local/minecraft/source.java", "decompiled/Foo.txt", "game.JAR", "capture.jfr", "source/Foo.java",
                     "Horizon.ipa", "signing/account.p12", "profile.mobileprovision", "project/xcuserdata/user.xcuserdatad/state",
                     "export/Game.xcarchive/Info.plist", ".env", "config/.env.production"]:
            self.assertTrue(check_public_tree.prohibited(path), path)
        for path in [".env.example", "config/.env.sample", ".env.template"]:
            self.assertFalse(check_public_tree.prohibited(path), path)
        for path in ["tools/lab.py", "crates/horizon-core/src/lib.rs", "Cargo.lock", "docs/analysis.md",
                     "mods/horizon/src/main/java/dev/horizonlab/HorizonLab.java",
                     "mods/horizon-lab/src/main/java/dev/horizonlab/HorizonLab.java"]:
            self.assertFalse(check_public_tree.prohibited(path), path)
        self.assertTrue(check_public_tree.prohibited("mods/horizon/src/main/java/net/minecraft/Client.java"))
        for path in ["tools/JfrSummary.java", "tests/java/JfrFixture.java",
                     "tools/regex_reference/JavaBlockRanges.java",
                     "tools/regex_reference/JavaCaseMappings.java",
                     "tools/regex_reference/JavaPropertyRanges.java"]:
            self.assertFalse(check_public_tree.prohibited(path), path)
        for path in ["tools/Minecraft.java", "tests/java/Client.java", ".local/tools/JfrSummary.java",
                     "tools/regex_reference/RecoveredGame.java"]:
            self.assertTrue(check_public_tree.prohibited(path), path)

    def test_force_added_ignored_file_is_detected_in_index_and_history(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            real_check_output = subprocess.check_output
            def git(*args):
                return real_check_output(["git", *args], cwd=directory, stderr=subprocess.DEVNULL)
            git("init", "-b", "main")
            (directory / ".gitignore").write_text("/.local/\n")
            (directory / ".local").mkdir()
            (directory / ".local" / "artifact.txt").write_text("private fixture")
            git("add", ".gitignore")
            git("add", "-f", ".local/artifact.txt")
            with mock.patch("tools.check_public_tree.subprocess.check_output",
                            side_effect=lambda command: git(*command[1:])):
                self.assertIn(".local/artifact.txt", check_public_tree.paths_in_repository())
                git("-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-m", "fixture")
                self.assertIn(".local/artifact.txt", check_public_tree.paths_in_repository(history=True))


def library(name, os_name=None):
    entry = {"name": name, "downloads": {"artifact": {"path": name.replace(":", "/") + ".jar",
                                                      "sha1": hashlib.sha1(name.encode()).hexdigest()}}}
    if os_name:
        entry["rules"] = [{"action": "allow", "os": {"name": os_name}}]
    return entry


# The shapes of 26.3's libraries: OS-only rules, ARM and x64 natives told apart by classifier.
PLATFORM_MANIFEST = {"libraries": [
    library("com.mojang:brigadier:1.3.10"),
    library("ca.weblite:java-objc-bridge:1.1", "osx"),
    library("org.lwjgl:lwjgl:3.4.3"),
    library("org.lwjgl:lwjgl:3.4.3:natives-linux", "linux"),
    library("org.lwjgl:lwjgl:3.4.3:natives-macos", "osx"),
    library("org.lwjgl:lwjgl:3.4.3:natives-macos-arm64", "osx"),
    library("org.lwjgl:lwjgl:3.4.3:natives-windows", "windows"),
    library("org.lwjgl:lwjgl:3.4.3:natives-windows-arm64", "windows"),
    library("io.netty:netty-transport-native-epoll:4.2.16.Final:linux-aarch_64", "linux"),
    library("io.netty:netty-transport-native-epoll:4.2.16.Final:linux-x86_64", "linux"),
    library("io.netty:netty-transport-native-kqueue:4.2.16.Final:osx-aarch_64", "osx"),
    library("io.netty:netty-transport-native-kqueue:4.2.16.Final:osx-x86_64", "osx"),
    {"name": "org.example:no-artifact:1.0", "downloads": {}},
]}


class PlatformLibraryTests(unittest.TestCase):
    def selected(self, system, architecture):
        names = {entry["downloads"]["artifact"]["path"]: entry["name"]
                 for entry in PLATFORM_MANIFEST["libraries"] if entry["downloads"]}
        return {names[artifact["path"]]
                for artifact in lab.client_library_artifacts(PLATFORM_MANIFEST, system, architecture)}

    def test_each_platform_requires_only_what_the_official_launcher_installs(self):
        common = {"com.mojang:brigadier:1.3.10", "org.lwjgl:lwjgl:3.4.3"}
        expected = {
            ("linux", "x86_64"): {"org.lwjgl:lwjgl:3.4.3:natives-linux",
                                  "io.netty:netty-transport-native-epoll:4.2.16.Final:linux-x86_64"},
            # No ARM build of natives-linux is listed, so the plain one is the only one.
            ("linux", "aarch64"): {"org.lwjgl:lwjgl:3.4.3:natives-linux",
                                   "io.netty:netty-transport-native-epoll:4.2.16.Final:linux-aarch_64"},
            ("osx", "arm64"): {"ca.weblite:java-objc-bridge:1.1", "org.lwjgl:lwjgl:3.4.3:natives-macos-arm64",
                               "io.netty:netty-transport-native-kqueue:4.2.16.Final:osx-aarch_64"},
            ("osx", "x86_64"): {"ca.weblite:java-objc-bridge:1.1", "org.lwjgl:lwjgl:3.4.3:natives-macos",
                                "io.netty:netty-transport-native-kqueue:4.2.16.Final:osx-x86_64"},
            ("windows", "AMD64"): {"org.lwjgl:lwjgl:3.4.3:natives-windows"},
            ("windows", "ARM64"): {"org.lwjgl:lwjgl:3.4.3:natives-windows-arm64"},
        }
        for (system, architecture), specific in expected.items():
            with self.subTest(system=system, architecture=architecture):
                self.assertEqual(self.selected(system, architecture), common | specific)

    def test_rules_follow_the_launcher_last_match_and_arch_aliases(self):
        rules = [{"action": "allow"}, {"action": "disallow", "os": {"name": "osx", "arch": "aarch64"}}]
        self.assertTrue(lab.library_allowed({"rules": rules}, "osx", "x86_64"))
        self.assertFalse(lab.library_allowed({"rules": rules}, "osx", "arm64"))
        self.assertFalse(lab.library_allowed({"rules": rules}, "osx", "aarch64"))
        self.assertTrue(lab.library_allowed({"rules": rules}, "linux", "aarch64"))
        self.assertTrue(lab.library_allowed({"rules": [{"action": "allow", "os": {"arch": "x86_64"}}]}, "windows", "AMD64"))
        self.assertFalse(lab.library_allowed({"rules": [{"action": "allow", "features": {"is_demo_user": True}}]}, "linux"))

    def test_prepare_on_linux_skips_macos_libraries_and_still_checks_required_ones(self):
        with tempfile.TemporaryDirectory() as temp:
            launcher, cache = Path(temp) / "minecraft", Path(temp) / "cache"
            artifacts = lab.client_library_artifacts(PLATFORM_MANIFEST, "linux", "x86_64")
            # A Linux install: the launcher never fetched the macOS bridge. Each fixture
            # jar's content is its Maven name, which is what its sha1 covers.
            for entry in PLATFORM_MANIFEST["libraries"]:
                artifact = entry["downloads"].get("artifact")
                if artifact in artifacts:
                    path = launcher / "libraries" / artifact["path"]
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(entry["name"].encode())
            imported = [lab.import_library(a, cache, launcher) for a in artifacts]
            self.assertFalse(any("objc" in str(path) for path in imported))
            required = next(a for a in artifacts if "natives-linux" in a["path"])
            (launcher / "libraries" / required["path"]).write_bytes(b"tampered")
            with self.assertRaisesRegex(ValueError, "not the pinned file"):
                lab.import_library(required, Path(temp) / "other", launcher)
            (launcher / "libraries" / required["path"]).unlink()
            with self.assertRaisesRegex(FileNotFoundError, "official launcher"):
                lab.import_library(required, Path(temp) / "other", launcher)


if __name__ == "__main__":
    unittest.main()
