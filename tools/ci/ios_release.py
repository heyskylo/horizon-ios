#!/usr/bin/env python3
"""Audit an unsigned iOS build and prepare its versioned release assets."""
import argparse
import hashlib
import plistlib
import re
import shutil
import stat
import struct
import zipfile
from pathlib import Path, PurePosixPath

VERSION = re.compile(r"[0-9]+(?:\.[0-9]+){0,2}\Z")
PRIVATE_NAMES = {"horizon-resources.json", "horizon-data.bin", "horizon-auth.json",
                 "horizon-player.txt", "servers.dat", "options.txt"}
PRIVATE_SUFFIXES = {".p12", ".pfx", ".mobileprovision", ".pem", ".key", ".cer",
                    ".crt", ".jar", ".nbt", ".mca", ".mcr"}
MACHO64 = b"\xcf\xfa\xed\xfe"
OTHER_MACHO = {b"\xfe\xed\xfa\xcf", b"\xce\xfa\xed\xfe", b"\xfe\xed\xfa\xce",
               b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca", b"\xca\xfe\xba\xbf", b"\xbf\xba\xfe\xca"}


# Run 88 was the first release in this sequence. Retries change only the build.
BASE_RUN = 88
BASE_PATCH = 44


def release_version(run, attempt):
    if not re.fullmatch(r"[1-9][0-9]*", str(run)) or not re.fullmatch(r"[1-9][0-9]*", str(attempt)):
        raise ValueError("Invalid workflow run or attempt")
    # Fresh forks have independent counters; keep their preview builds at the base.
    patch = BASE_PATCH + max(0, int(run) - BASE_RUN)
    return f"0.1.{patch}", f"{run}.{attempt}"


def stamp_app(app, run, attempt, commit, client_id):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("Invalid source commit")
    version, build = release_version(run, attempt)
    paths = [app / "Info.plist", *sorted((app / "Frameworks").glob("*.framework/Info.plist"))]
    # Read all metadata before changing any file.
    records = [(path, plistlib.loads(path.read_bytes())) for path in paths]
    for path, info in records:
        info["CFBundleShortVersionString"] = version
        info["CFBundleVersion"] = build
        if path == app / "Info.plist":
            info["HorizonBuildCommit"] = commit
            info["HorizonMSAClientID"] = client_id
        path.write_bytes(plistlib.dumps(info))
    return version, build


def unsigned_macho(stream):
    magic = stream.read(4)
    if magic in OTHER_MACHO:
        raise ValueError("Expected a thin ARM64 iOS binary")
    if magic != MACHO64:
        return False
    header = magic + stream.read(28)
    if len(header) != 32:
        raise ValueError("Truncated Mach-O header")
    cpu, _, file_type, count, size = struct.unpack_from("<IIIII", header, 4)
    if cpu != 0x0100000C or size > 16 << 20:
        raise ValueError("Invalid ARM64 Mach-O load commands")
    commands = stream.read(size)
    if len(commands) != size:
        raise ValueError("Truncated Mach-O load commands")
    offset = 0
    for _ in range(count):
        if offset + 8 > size:
            raise ValueError("Invalid Mach-O command count")
        command, length = struct.unpack_from("<II", commands, offset)
        if length < 8 or offset + length > size:
            raise ValueError("Invalid Mach-O command size")
        if command == 0x1D:  # LC_CODE_SIGNATURE, including ad-hoc signatures.
            raise ValueError("The IPA contains a signed binary")
        offset += length
    if offset != size:
        raise ValueError("Unexpected Mach-O load-command data")
    return file_type


def audit(ipa, expected_commit=None):
    with zipfile.ZipFile(ipa) as archive:
        names = archive.namelist()
        if len(names) != len(set(names)):
            raise ValueError("Duplicate IPA entries")
        binaries = {}
        infos = []
        frameworks = []
        for entry in archive.infolist():
            path = PurePosixPath(entry.filename)
            parts = {p.lower() for p in path.parts}
            name = path.name.lower()
            if (path.is_absolute() or ".." in parts or "\\" in entry.filename
                    or not path.parts or path.parts[0] != "Payload"):
                raise ValueError("Unexpected IPA path")
            if (name in PRIVATE_NAMES or path.suffix.lower() in PRIVATE_SUFFIXES
                    or name.startswith(".env")
                    or parts & {"_codesignature", "xcuserdata", "saves", "logs"}):
                raise ValueError("The IPA contains signing material, private files or game assets")
            if stat.S_ISLNK(entry.external_attr >> 16):
                raise ValueError("The IPA contains a symbolic link")
            if entry.is_dir():
                continue
            with archive.open(entry) as stream:
                kind = unsigned_macho(stream)
                if kind:
                    binaries[entry.filename] = kind
            if len(path.parts) == 3 and path.parts[1].endswith(".app") and name == "info.plist":
                if entry.file_size > 1 << 20:
                    raise ValueError("Oversized app Info.plist")
                infos.append((path.parent, plistlib.loads(archive.read(entry))))
            if name == "info.plist" and path.parent.suffix == ".framework":
                if entry.file_size > 1 << 20:
                    raise ValueError("Oversized framework Info.plist")
                frameworks.append(plistlib.loads(archive.read(entry)))
        if archive.testzip() is not None:
            raise ValueError("The IPA contains a corrupt ZIP entry")
        if len(infos) != 1:
            raise ValueError("Expected exactly one application")
        app, info = infos[0]
        if info.get("CFBundleSupportedPlatforms") != ["iPhoneOS"]:
            raise ValueError("Expected a device IPA, not a simulator app")
        executable = info.get("CFBundleExecutable", "")
        if not isinstance(executable, str) or "/" in executable or binaries.get(str(app / executable)) != 2:
            raise ValueError("Missing unsigned ARM64 app executable")
        version, build = info.get("CFBundleShortVersionString"), info.get("CFBundleVersion")
        if not all(isinstance(v, str) and VERSION.fullmatch(v) for v in (version, build)):
            raise ValueError("Invalid app version or build number")
        for framework in frameworks:
            if (framework.get("CFBundleShortVersionString"), framework.get("CFBundleVersion")) != (version, build):
                raise ValueError("Framework version does not match the app")
        if expected_commit is not None:
            if not re.fullmatch(r"[0-9a-f]{40}", expected_commit):
                raise ValueError("Invalid source commit")
            if info.get("HorizonBuildCommit") != expected_commit:
                raise ValueError("IPA source commit does not match this workflow")
        return version, build


def prepare(ipa, output, expected_commit):
    version, build = audit(ipa, expected_commit)
    output.mkdir(parents=True, exist_ok=True)
    target = output / f"Horizon-iOS-{version}-build{build}-unsigned.ipa"
    shutil.copyfile(ipa, target)
    digest_state = hashlib.sha256()
    with target.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1 << 20), b""):
            digest_state.update(chunk)
    digest = digest_state.hexdigest()
    checksum = target.with_suffix(target.suffix + ".sha256")
    checksum.write_text(f"{digest}  {target.name}\n", encoding="utf-8")
    return {"ipa": str(target), "checksum": str(checksum), "version": version, "build": build}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ipa", type=Path, nargs="?")
    parser.add_argument("--stamp-app", type=Path)
    parser.add_argument("--run-number")
    parser.add_argument("--run-attempt")
    parser.add_argument("--client-id", default="")
    parser.add_argument("--expect-commit", required=True)
    parser.add_argument("--output-dir", type=Path)
    args = parser.parse_args()
    if args.stamp_app:
        if args.ipa or args.output_dir or not args.run_number or not args.run_attempt:
            parser.error("--stamp-app requires run number and attempt, without an IPA/output directory")
        stamp_app(args.stamp_app, args.run_number, args.run_attempt, args.expect_commit, args.client_id)
        return
    if not args.ipa:
        parser.error("an IPA or --stamp-app is required")
    if args.run_number or args.run_attempt:
        expected = release_version(args.run_number, args.run_attempt)
        if audit(args.ipa, args.expect_commit) != expected:
            raise ValueError("IPA version does not match this workflow run")
    result = (prepare(args.ipa, args.output_dir, args.expect_commit) if args.output_dir
              else dict(zip(("version", "build"), audit(args.ipa, args.expect_commit))))
    for key, value in result.items():
        print(f"{key}={value}")


if __name__ == "__main__":
    main()
