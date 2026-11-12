#!/usr/bin/env python3
"""Reuse a vanilla oracle's last results when nothing it reads has changed.

tools/parity.py puts `java` and `javac` links to this script first on PATH.
A run is identified by its command line, stdin when it is a file, and the
real program. After a successful run, the files it created or changed under
the paths its arguments name (classpath and sourcepath entries included) are
recorded as its outputs, with their size and mtime and its stdout and stderr;
every other file under those paths is recorded as an input with its content
hash. The next run of the same command replays the recorded output instead
of running the oracle if every input has the same content, no file was added
beside them, and the outputs are exactly as it left them. On a Rust-only
change a harness then pays only for its Rust side.

Files an oracle writes outside its named paths are not tracked. A harness
that depends on them fails on reuse; tools/parity.py then reruns it fresh and
stops reusing its oracle results.

A run that reads stdin from a pipe, or fails, is never cached.
PARITY_JAVA_CACHE=0 disables reuse (tools/parity.py --fresh).
"""

import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import threading

CACHE = Path(os.environ.get("PARITY_SHIM_CACHE", ".local/parity-runner/oracle-cache"))
PATH_OPTIONS = {"-cp", "-classpath", "--class-path", "-sourcepath", "--source-path", "-p", "--module-path"}


# How deep the shim may run inside itself before it stops: it never needs to, since the real program
# gets PARITY_SHIM_REAL_PATH as its PATH, but a wrapper that puts the shim back would start one
# process after another until the machine runs out of memory.
MAX_DEPTH = 4


def real(name):
    """The first `name` on PARITY_SHIM_REAL_PATH that is not this script (a nested run's PATH holds the shim)."""
    me = Path(__file__).resolve()
    for folder in os.environ.get("PARITY_SHIM_REAL_PATH", "").split(os.pathsep):
        found = folder and shutil.which(name, path=folder)
        if found and Path(found).resolve() != me:
            return found
    sys.exit(f"parity shim: no real {name} on PARITY_SHIM_REAL_PATH")


def enter():
    """Count this run and give what it starts the real PATH, so a wrapper looking `java` up on PATH
    (a JDK selector, a build wrapper) finds the real one and not the shim again."""
    depth = int(os.environ.get("PARITY_SHIM_DEPTH") or 0)
    if depth >= MAX_DEPTH:
        sys.exit(f"parity shim: started inside itself {depth} times: what {os.path.basename(sys.argv[0])} runs "
                 "leads back to the shim (a wrapper that puts it back on PATH?)")
    os.environ["PARITY_SHIM_DEPTH"] = str(depth + 1)
    os.environ["PATH"] = os.environ.get("PARITY_SHIM_REAL_PATH", "")


class Hashes:
    """Content hashes keyed by path, size and mtime, shared by parallel harnesses."""

    def __init__(self):
        CACHE.mkdir(parents=True, exist_ok=True)
        self.db = sqlite3.connect(CACHE / "hashes.sqlite", timeout=60)
        self.db.execute("pragma journal_mode=wal")
        self.db.execute("create table if not exists hashes (path text primary key, size int, mtime int, sha text)")

    def file(self, path):
        stat = os.stat(path)
        row = self.db.execute("select size, mtime, sha from hashes where path = ?", (path,)).fetchone()
        if row and row[0] == stat.st_size and row[1] == stat.st_mtime_ns:
            return row[2]
        with open(path, "rb") as stream:
            sha = hashlib.file_digest(stream, "sha256").hexdigest()
        self.db.execute("insert or replace into hashes values (?, ?, ?, ?)", (path, stat.st_size, stat.st_mtime_ns, sha))
        self.db.commit()
        return sha


def tree(root):
    for directory, _, files in os.walk(root):
        for name in files:
            yield os.path.join(directory, name)


def named_paths(arguments):
    """Every filesystem path the arguments name, with classpath lists split."""
    paths = []
    for index, argument in enumerate(arguments):
        if index and arguments[index - 1] in PATH_OPTIONS:
            paths += [entry for entry in argument.split(os.pathsep) if entry]
        elif argument.startswith("-D") and "=" in argument:
            paths.append(argument.split("=", 1)[1])
        elif argument.startswith("-javaagent:"):
            # The jar is an input; its options, after "=", are in the command line already.
            paths.append(argument.removeprefix("-javaagent:").split("=", 1)[0])
        elif not argument.startswith("-"):
            paths.append(argument)
    return [os.path.abspath(path) for path in paths if path]


def snapshot(paths):
    state = {}
    for path in paths:
        files = tree(path) if os.path.isdir(path) else [path]
        for file in files:
            try:
                stat = os.stat(file)
                state[file] = [stat.st_size, stat.st_mtime_ns]
            except FileNotFoundError:
                state[file] = None
    return state


def stdin_identity(hashes):
    try:
        stat = os.fstat(0)
    except OSError:
        return "closed"
    if os.path.samestat(stat, os.stat(os.devnull)) if os.path.exists(os.devnull) else False:
        return "null"
    import stat as modes
    if modes.S_ISREG(stat.st_mode):
        # Hash the content without moving the shared offset (no /proc on macOS).
        digest, offset = hashlib.sha256(), 0
        while chunk := os.pread(0, 1 << 20, offset):
            digest.update(chunk)
            offset += len(chunk)
        return "f:" + digest.hexdigest()
    if modes.S_ISCHR(stat.st_mode):
        return "tty"
    return None  # a pipe or socket: its content is unknown, so never cache


def run_and_tee(command):
    """Run the real program, forwarding and recording its stdout and stderr."""
    process = subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    captured = {"stdout": bytearray(), "stderr": bytearray()}

    def pump(source, sink, buffer):
        for chunk in iter(lambda: source.read1(65536), b""):
            buffer += chunk
            sink.write(chunk)
            sink.flush()

    threads = [threading.Thread(target=pump, args=(process.stdout, sys.stdout.buffer, captured["stdout"])),
               threading.Thread(target=pump, args=(process.stderr, sys.stderr.buffer, captured["stderr"]))]
    for thread in threads:
        thread.start()
    status = process.wait()
    for thread in threads:
        thread.join()
    return status, captured


def files_under(paths):
    found = set()
    for path in paths:
        if os.path.isdir(path):
            found.update(tree(path))
        elif os.path.isfile(path):
            found.add(path)
    return found


def note_outputs(outputs):
    """Tell tools/parity.py which files are oracle outputs, which it keeps when clearing large outputs."""
    listing = os.environ.get("PARITY_SHIM_OUTPUTS")
    if listing:
        with open(listing, "a") as stream:
            stream.writelines(f"{path}\n" for path in outputs)


def reusable(saved, paths, hashes):
    """The recorded inputs are unchanged, nothing was added beside them, and the outputs are as left."""
    outputs = saved["outputs"]
    if files_under(paths) - set(outputs) != set(saved["inputs"]):
        return False
    if any(snapshot([path]).get(path) != stamp for path, stamp in outputs.items()):
        return False
    return all(hashes.file(path) == sha for path, sha in saved["inputs"].items())


def main():
    name = os.path.basename(sys.argv[0])
    arguments = sys.argv[1:]
    program = real(name)
    enter()
    if os.environ.get("PARITY_JAVA_CACHE") == "0" or "-version" in arguments or "--version" in arguments:
        os.execv(program, [program, *arguments])
    hashes = Hashes()
    stdin = stdin_identity(hashes)
    if stdin is None:
        os.execv(program, [program, *arguments])
    paths = named_paths(arguments)
    command = {"program": os.path.realpath(program), "arguments": arguments, "cwd": os.getcwd(), "stdin": stdin}
    key = hashlib.sha256(json.dumps(command, sort_keys=True).encode()).hexdigest()
    record = CACHE / f"{key}.json"
    saved = json.loads(record.read_text()) if record.exists() else None
    if saved and reusable(saved, paths, hashes):
        note_outputs(saved["outputs"])
        sys.stdout.buffer.write((CACHE / f"{key}.stdout").read_bytes())
        sys.stderr.buffer.write((CACHE / f"{key}.stderr").read_bytes())
        return 0
    before = snapshot(paths)
    inputs = {path: hashes.file(path) for path in files_under(paths)}
    status, captured = run_and_tee([program, *arguments])
    if status != 0:
        return status
    after = snapshot(paths)
    outputs = {path: stamp for path, stamp in after.items() if stamp is not None and before.get(path) != stamp}
    for path in outputs:
        inputs.pop(path, None)
    (CACHE / f"{key}.stdout").write_bytes(captured["stdout"])
    (CACHE / f"{key}.stderr").write_bytes(captured["stderr"])
    temporary = record.with_suffix(f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps({"command": command, "inputs": inputs, "outputs": outputs}))
    temporary.replace(record)
    note_outputs(outputs)
    return 0


if __name__ == "__main__":
    sys.exit(main())
