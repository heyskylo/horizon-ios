#!/usr/bin/env python3
"""Pauses the Horizon CI runner (tools/ci/linux-runner.sh) while the maintainer uses this machine or plays on it.

The runner works while nobody uses the machine: its screen locked, or untouched for IDLE_AFTER seconds (900).
It pauses when any of these holds:
  - keyboard, mouse or gamepad input in the last INPUT_PAUSE seconds (60), or an unlocked session on seat0 with
    input in the last IDLE_AFTER seconds;
  - the CPU package at TEMP_LIMIT °C (90) or more (the median of 5 readings);
  - fleet-load says a game runs (~/fleet/load: Steam, gamescope, Wine or Proton processes);
  - a process of his matches GAME below (the same, plus Minecraft and a Horizon game he started himself);
  - a process outside horizon-ci.slice keeps the GPU busy (nvidia-smi: SM use >= GPU_BUSY %, default 50, in two
    samples in a row; the browser, Discord and the compositor stay under 20 %);
  - ~/.actions-runners/horizon/pause exists (pause by hand: `touch` it; `rm` it to let the watcher decide).

Paused, the runner takes no new job: while it is idle its service is stopped, so GitHub sends the jobs to the
Mac mini, and the parity workers other machines started here stop (their harnesses go back to those machines). A
job already running is throttled, not stopped: horizon-ci.slice drops to CPU weight 1, I/O weight 1 and a quota of
THROTTLE_CPUS threads, and the service stops once the job ends. Freezing it (SIGSTOP or the cgroup freezer)
would stop the runner's heartbeat, so GitHub would drop the job as lost, and the harnesses' wall-clock timeouts
would fail runs that were only waiting. The runner comes back RESUME_AFTER seconds after the last of these signs.
Every change is written to ~/.actions-runners/horizon/pause.log.
"""
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

DIR = Path.home() / ".actions-runners" / "horizon"
SERVICE, SLICE = "horizon-runner.service", "horizon-ci.slice"
MANUAL, STATE, LOG = DIR / "pause", DIR / "paused", DIR / "pause.log"
GAME = re.compile(r"(SteamLaunch|gamescope|wine64-preloader|wineserver|wine-preloader|\.exe(\s|$)|proton\b"
                  r"|net\.minecraft\.client\.main\.Main|minecraft-launcher|prismlauncher)", re.I)
GPU_BUSY = int(os.environ.get("GPU_BUSY", 50))
INTERVAL = int(os.environ.get("INTERVAL", 15))
RESUME_AFTER = int(os.environ.get("RESUME_AFTER", 300))
THROTTLE_CPUS = int(os.environ.get("THROTTLE_CPUS", 2))
# Working, the slice gets half the threads: on all of them ludodesktop's CPU sits at 92 °C through a cargo build.
NORMAL_CPUS = int(os.environ.get("NORMAL_CPUS", max(1, (os.cpu_count() or 2) // 2)))
INPUT_PAUSE = int(os.environ.get("INPUT_PAUSE", 60))
IDLE_AFTER = int(os.environ.get("IDLE_AFTER", 900))
TEMP_LIMIT = int(os.environ.get("TEMP_LIMIT", 90))


def log(text):
    with LOG.open("a") as handle:
        handle.write(f"{time.strftime('%Y-%m-%dT%H:%M:%S')} {text}\n")


def systemctl(*arguments):
    return subprocess.run(["systemctl", "--user", *arguments], capture_output=True, text=True)


def ci_process(pid):
    """A process of the runner's slice (its jobs, the parity workers other machines start here)."""
    try:
        return SLICE in Path(f"/proc/{pid}/cgroup").read_text()
    except OSError:
        return True  # gone


class Input:
    """Keyboard, mouse and gamepad activity, from /dev/input (the input group): only whether bytes came, never what."""

    def __init__(self):
        self.devices = {}  # path -> file descriptor
        self.last = 0.0  # the boot (the monotonic clock's zero on Linux) counts as input

    def poll(self):
        try:
            text = Path("/proc/bus/input/devices").read_text()
        except OSError:
            text = ""
        wanted = {f"/dev/input/{handler}" for block in text.split("\n\n") for line in block.splitlines()
                  if line.startswith("H: Handlers=") and re.search(r"\b(kbd|mouse\d*|js\d*)\b", line)
                  for handler in line.split("=", 1)[1].split() if handler.startswith("event")}
        for path in set(self.devices) - wanted:
            os.close(self.devices.pop(path))
        for path in wanted - set(self.devices):
            try:
                self.devices[path] = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
            except OSError:
                pass
        for path, descriptor in list(self.devices.items()):
            try:
                while os.read(descriptor, 4096):
                    self.last = time.monotonic()
            except BlockingIOError:
                pass
            except OSError:  # unplugged
                os.close(self.devices.pop(path))
        return time.monotonic() - self.last


def seat_session():
    """The graphical session on seat0 and whether its screen is locked, or None."""
    sessions = subprocess.run(["loginctl", "list-sessions", "--no-legend"], capture_output=True, text=True).stdout
    for line in sessions.splitlines():
        fields = line.split()
        if "seat0" in fields:
            locked = subprocess.run(["loginctl", "show-session", fields[0], "-p", "LockedHint", "--value"],
                                    capture_output=True, text=True).stdout.strip() == "yes"
            return fields[0], locked
    return None


def at_the_screen(input_idle):
    """Someone is at the machine: input in the last INPUT_PAUSE seconds, or an unlocked session with input in the last
    IDLE_AFTER seconds. A locked screen, or one nobody touched for IDLE_AFTER, leaves the machine to CI."""
    if input_idle < INPUT_PAUSE:
        return f"input {input_idle:.0f} s ago"
    session = seat_session()
    if session and not session[1] and input_idle < IDLE_AFTER:
        return f"session {session[0]} unlocked, input {input_idle:.0f} s ago"
    return None


def too_hot():
    """The CPU package at TEMP_LIMIT °C or more (coretemp or k10temp), as the median of 5 readings over 2 s: under
    load it jumps for a second at a time (to 96 °C on ludodesktop at a median of 68 °C), which is no reason to stop."""
    for monitor in Path("/sys/class/hwmon").glob("hwmon*"):
        try:
            if (monitor / "name").read_text().strip() not in ("coretemp", "k10temp"):
                continue
            readings = []
            for _ in range(5):
                readings.append(int((monitor / "temp1_input").read_text()) / 1000)
                time.sleep(0.5)
        except (OSError, ValueError):
            continue
        celsius = sorted(readings)[2]
        return f"CPU at {celsius:.0f} °C" if celsius >= TEMP_LIMIT else None
    return None


def fleet_load_gaming():
    status = Path(os.environ.get("XDG_RUNTIME_DIR", f"/run/user/{os.getuid()}")) / "fleet-load" / "status.json"
    try:
        if time.time() - status.stat().st_mtime > 120:
            return None
        return "fleet-load: playing a game" if json.loads(status.read_text())["human"]["gaming"] else None
    except (OSError, ValueError, KeyError, TypeError):
        return None


def game_process():
    uid = os.getuid()
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            if entry.stat().st_uid != uid:
                continue
            command = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except OSError:
            continue
        # A Horizon game counts when he started it (its parent is his launcher), not when a harness or agent did.
        if GAME.search(command) or (" --offscreen" not in command and "/horizon-launcher/" in command
                                    and "horizon-client" in command.split(" ")[0]):
            if not ci_process(int(entry.name)):
                return f"process {entry.name}: {command[:120]}"
    return None


def gpu_users():
    """Processes outside the CI slice using the GPU's shader cores, as {pid: SM %}."""
    try:
        # Two samples: the first one nvidia-smi prints has no utilisation yet.
        output = subprocess.run(["nvidia-smi", "pmon", "-c", "2", "-s", "u"], capture_output=True, text=True,
                                timeout=20).stdout
    except (OSError, subprocess.TimeoutExpired):
        return {}
    users = {}
    for line in output.splitlines():
        fields = line.split()
        if line.startswith("#") or len(fields) < 4 or not fields[1].isdigit() or not fields[3].isdigit():
            continue
        if not ci_process(int(fields[1])):
            users[int(fields[1])] = max(users.get(int(fields[1]), 0), int(fields[3]))
    return users


def runner_busy():
    """The runner is in a job: a Runner.Worker runs in its service."""
    group = systemctl("show", "-p", "ControlGroup", "--value", SERVICE).stdout.strip()
    try:
        pids = Path(f"/sys/fs/cgroup{group}/cgroup.procs").read_text().split() if group else []
    except OSError:
        return False
    for pid in pids:
        try:
            if Path(f"/proc/{pid}/comm").read_text().strip() == "Runner.Worker":
                return True
        except OSError:
            pass
    return False


def throttle(on):
    if on:
        systemctl("set-property", "--runtime", SLICE, "CPUWeight=1", "IOWeight=1", f"CPUQuota={THROTTLE_CPUS * 100}%")
    else:
        systemctl("set-property", "--runtime", SLICE, "CPUWeight=20", "IOWeight=20", f"CPUQuota={NORMAL_CPUS * 100}%")


def pause_reason(hot, keys):
    """Why the runner may not work now, or None. hot: the GPU users seen busy at the last sample (updated);
    keys: the Input watcher."""
    if MANUAL.exists():
        return f"paused by hand ({MANUAL})"
    reason = at_the_screen(keys.poll()) or too_hot() or fleet_load_gaming() or game_process()
    if reason:
        return reason
    busy = {pid for pid, sm in gpu_users().items() if sm >= GPU_BUSY}
    twice = sorted(busy & set(hot))
    hot.clear()
    hot.update(dict.fromkeys(busy))
    if twice:
        try:
            name = Path(f"/proc/{twice[0]}/comm").read_text().strip()
        except OSError:
            name = "?"
        return f"GPU busy: process {twice[0]} ({name})"
    return None


def main():
    hot, keys = {}, Input()
    # The boot counts as a sign: the runner starts RESUME_AFTER seconds after it at the earliest, which leaves time
    # to log in after switching the machine on. Restarting this watcher later changes nothing for a running job.
    last_sign = 0.0
    paused = None
    while True:
        reason = pause_reason(hot, keys)
        now = time.monotonic()
        if reason:
            last_sign = now
        active = systemctl("is-active", "--quiet", SERVICE).returncode == 0
        if reason or now - last_sign < RESUME_AFTER:
            if paused is not True:
                paused = True
                # The runner's folder may not exist yet on a fresh machine.
                STATE.parent.mkdir(parents=True, exist_ok=True)
                STATE.write_text((reason or "starting") + "\n")
                throttle(True)
                # Parity workers other machines started here (tools/parity_hosts.json): they stop, and those
                # machines run the harnesses themselves.
                systemctl("stop", "horizon-parity-worker-*.scope")
                log(f"pause: {reason or 'waiting ' + str(RESUME_AFTER) + ' s after starting'}"
                    + (f"; a job is running: throttled to {THROTTLE_CPUS} threads, stops after it" if active else ""))
            if active and not runner_busy():
                systemctl("stop", SERVICE)
                log("runner stopped")
        else:
            if paused is not False:
                paused = False
                STATE.unlink(missing_ok=True)
                throttle(False)
                log(f"resume: nobody at the screen and no game for {RESUME_AFTER} s")
            if not active:
                systemctl("start", SERVICE)
                log("runner started")
        time.sleep(INTERVAL)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        # The watcher takes no arguments: the service runs it with none. It used to start
        # watching (and crash on a missing folder) whatever it was given.
        sys.exit(__doc__.strip() + '\nUsage: python3 tools/ci/runner_pause.py')
    main()
