#!/usr/bin/env python3
"""Launch the vanilla 26.3 client offline (no Microsoft account, no auth
servers: --offlineDeveloperMode) and optionally join a server straight away
(--quickPlayMultiplayer). Meant to run on a virtual screen:

  vscreen start python3 tools/vanilla_client.py --name Guest --join localhost:25565

On Linux (x86_64) and on macOS (Apple silicon, from the logged-in screen session), from the files
tools/lab.py prepared on that machine. On macOS vsync is off: with it on, a window nobody can see (a
locked session, or covered) blocks in SDL_GL_SwapWindow and the client stops before it joins.

Sound goes to OpenAL's null backend. The game directory gets an options.txt
that skips the first-run screens, keeps the game running unfocused and
renders few chunks, so a harness can drive it with vscreen keys.
"""
import argparse
import json
import os
import subprocess
import sys
import zipfile
from pathlib import Path

try:
    from . import lab
except ImportError:
    import lab

ROOT = Path(__file__).resolve().parents[1]
GAME = lab.GAME
MAC = sys.platform == "darwin"
# The natives of this machine's platform, and the shared libraries in them.
NATIVES, LIBRARY_SUFFIX = ("natives-macos-arm64.jar", ".dylib") if MAC else ("natives-linux.jar", ".so")

OPTIONS = {
    "onboardAccessibility": "false",
    "skipMultiplayerWarning": "true",
    "joinedFirstServer": "true",
    "tutorialStep": "none",
    "pauseOnLostFocus": "false",
    "renderDistance": "6",
    "simulationDistance": "6",
    "maxFps": "30",
    "narrator": "0",
    "soundCategory_master": "0.0",
    "realmsNotifications": "false",
    "hideServerAddress": "false",
    "autoJump": "false",
    "chatLinksPrompt": "false",
    "onlyShowSecureChat": "false",
    "rawMouseInput": "false",
}
if sys.platform == "darwin":
    OPTIONS["enableVsync"] = "false"


def host_library(path):
    name = Path(path).name
    if "natives-" in name:
        return name.endswith(NATIVES)
    if MAC:
        return "netty-transport-native" not in name or "linux" not in name
    if "netty-transport-native" in name and ("osx" in name or "aarch_64" in name):
        return False
    return "java-objc-bridge" not in name


def host_natives(path):
    """A natives jar for this machine in place of another platform's: a prepared.json made on one machine
    and shared with others (a shared .local) lists that machine's natives only; the jars sit side by side."""
    name = Path(path).name
    if "-natives-" not in name or name.endswith(NATIVES):
        return path
    mine = Path(path).with_name(name[:name.index("-natives-") + 1] + NATIVES)
    return str(mine) if (ROOT / mine).exists() else path


def classpath():
    prepared = json.loads((GAME / "prepared.json").read_text())
    jars = [ROOT / p for p in map(host_natives, prepared["libraries"]["client"]) if host_library(p)]
    return [GAME / "jars/client.jar", *jars]


def extract_natives(jars, target):
    target.mkdir(parents=True, exist_ok=True)
    for jar in jars:
        if not jar.name.endswith(NATIVES):
            continue
        with zipfile.ZipFile(jar) as archive:
            for name in archive.namelist():
                if name.endswith(LIBRARY_SUFFIX) and (MAC or ("arm" not in name and "aarch" not in name)):
                    out = target / Path(name).name
                    if not out.exists():
                        out.write_bytes(archive.read(name))


def write_options(game_dir, extra):
    path = game_dir / "options.txt"
    current = {}
    if path.exists():
        for line in path.read_text().splitlines():
            if ":" in line:
                k, v = line.split(":", 1)
                current[k] = v
    current.update(OPTIONS)
    current.update(extra)
    path.write_text("".join(f"{k}:{v}\n" for k, v in current.items()))


def environment():
    env = dict(os.environ)
    libs = [p for p in (os.environ.get("NIX_LD_LIBRARY_PATH", ""), "/run/opengl-driver/lib",
                        "/run/current-system/sw/share/nix-ld/lib") if p]
    env["LD_LIBRARY_PATH"] = ":".join(libs + [env.get("LD_LIBRARY_PATH", "")]).strip(":")
    env["ALSOFT_DRIVERS"] = "null"
    return env


def command(args):
    jars = classpath()
    natives = args.game_dir / "natives"
    extract_natives(jars, natives)
    assets = args.assets or (args.game_dir / "assets")
    (assets / "indexes").mkdir(parents=True, exist_ok=True)
    cmd = ["java", f"-Xmx{args.heap}", *(["-XstartOnFirstThread"] if MAC else []), "-XX:StackShadowPages=32",
           "--enable-native-access=ALL-UNNAMED",
           "--add-exports", "java.base/jdk.internal.misc=ALL-UNNAMED",
           f"-Djava.library.path={natives}", f"-Dorg.lwjgl.system.SharedLibraryExtractPath={natives}",
           f"-Dio.netty.native.workdir={natives}",
           "-cp", os.pathsep.join(map(str, jars)), "net.minecraft.client.main.Main",
           "--username", args.name, "--uuid", lab.offline_uuid(args.name).hex,
           "--accessToken", "0", "--offlineDeveloperMode",
           "--version", lab.VERSION, "--gameDir", str(args.game_dir),
           "--assetsDir", str(assets), "--assetIndex", json.loads((GAME / "manifest.json").read_text())["assetIndex"]["id"],
           "--width", str(args.width), "--height", str(args.height)]
    if args.join:
        cmd += ["--quickPlayMultiplayer", args.join]
    if args.world:
        cmd += ["--quickPlaySingleplayer", args.world]
    if args.lan_port is not None:
        # The vanilla client as reference LAN host (OfflineLanHost).
        cp = cmd.index("-cp") + 1
        cmd[cp] = os.pathsep.join([str(args.classes), cmd[cp]])
        main = cmd.index("net.minecraft.client.main.Main")
        cmd[main:main + 1] = [f"-Dhorizon.lanPort={args.lan_port}", "dev.horizonlab.parity.OfflineLanHost"]
    if args.backend:
        cmd += ["--graphicsBackend", args.backend]
    return cmd


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--name", default="Guest")
    parser.add_argument("--join", help="HOST:PORT to join on start")
    parser.add_argument("--world", help="open this saves/ folder in singleplayer on start")
    parser.add_argument("--game-dir", type=Path, default=ROOT / ".local/vanilla-client")
    parser.add_argument("--assets", type=Path)
    parser.add_argument("--heap", default="3G")
    parser.add_argument("--width", type=int, default=1280)
    parser.add_argument("--height", type=int, default=800)
    parser.add_argument("--lan-port", type=int,
                        help="open the --world to LAN on this port without authentication (needs --classes)")
    parser.add_argument("--classes", type=Path, default=ROOT / ".local/mp/oracle",
                        help="compiled OfflineLanHost (mods/horizon/src/test/java)")
    parser.add_argument("--backend", help="graphics backend (opengl or vulkan)")
    parser.add_argument("--option", action="append", default=[], help="KEY:VALUE for options.txt")
    parser.add_argument("--print", action="store_true", help="print the command and exit")
    args = parser.parse_args()
    args.game_dir = args.game_dir.resolve()
    args.game_dir.mkdir(parents=True, exist_ok=True)
    write_options(args.game_dir, dict(o.split(":", 1) for o in args.option))
    cmd = command(args)
    if args.print:
        print(" ".join(cmd))
        return
    sys.exit(subprocess.run(cmd, cwd=args.game_dir, env=environment()).returncode)


if __name__ == "__main__":
    main()
