import os
from pathlib import Path
import subprocess
import sys
import textwrap
import threading
import unittest

from tools import process_tree

ROOT = Path(__file__).resolve().parents[1]
# A harness: starts a child that sleeps a minute with the harness's stdout, prints the child's pid, then exits or hangs.
HARNESS = textwrap.dedent("""
    import subprocess, sys, time
    child = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"], stdout=sys.stdout)
    print(child.pid, flush=True)
    if sys.argv[1] == "hang":
        time.sleep(60)
""")
# A runner that starts the hanging harness through process_tree and waits, like tools/parity.py.
RUNNER = textwrap.dedent("""
    import sys, time
    sys.path.insert(0, sys.argv[1])
    from tools import process_tree
    process_tree.popen([sys.executable, "-c", sys.argv[2], "hang"], stdout=sys.stdout)
    time.sleep(60)
""")


class Output:
    """A process's stdout, read to its end: the end comes once every process holding it (the sleeping child) exits."""

    def __init__(self, process):
        self.lines, self.thread = [], threading.Thread(target=self._read, args=(process.stdout,), daemon=True)
        self.thread.start()

    def _read(self, stream):
        for line in stream:
            self.lines.append(line.strip())

    def closed(self, seconds):
        self.thread.join(seconds)
        return not self.thread.is_alive()


class ProcessTreeTests(unittest.TestCase):
    def start(self, arguments, tied=True):
        start = process_tree.popen if tied else subprocess.Popen
        process = start(arguments, stdout=subprocess.PIPE, text=True)
        self.addCleanup(process.stdout.close)
        self.addCleanup(self.stop if tied else subprocess.Popen.wait, process)
        return process, Output(process)

    def stop(self, process):
        process_tree.kill(process)
        process.wait()
        process_tree.ended(process)

    def stop_child(self, output):
        """The control case leaves the child running: end it by its pid, nothing else."""
        if output.lines and output.lines[0].isdigit():
            pid = int(output.lines[0])
            if os.name == "nt":
                subprocess.run(["taskkill", "/F", "/PID", str(pid)], capture_output=True)
            else:
                try:
                    os.kill(pid, 9)
                except ProcessLookupError:
                    pass

    @unittest.skipUnless(os.name == "nt" or hasattr(os, "waitid"), "macOS before Python 3.13 has no os.waitid")
    def test_what_an_exited_harness_left_running_ends_with_it(self):
        harness, output = self.start([sys.executable, "-c", HARNESS, "exit"])
        # Polled with ended(), as tools/parity.py does: wait() would reap the harness before its group is killed.
        for _ in range(600):
            if process_tree.ended(harness):
                break
            threading.Event().wait(0.05)
        self.assertIsNotNone(harness.returncode)
        self.assertTrue(output.closed(30), "the harness's child outlived it")

    def test_killing_a_harness_kills_what_it_started(self):
        harness, output = self.start([sys.executable, "-c", HARNESS, "hang"])
        while not output.lines:
            self.assertIsNone(harness.poll())
            threading.Event().wait(0.05)
        process_tree.kill(harness)
        harness.wait(30)
        self.assertTrue(process_tree.ended(harness))
        self.assertTrue(output.closed(30), "the harness's child outlived it")

    def test_other_processes_are_left_alone(self):
        bystander = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"])
        self.addCleanup(bystander.wait)
        self.addCleanup(bystander.kill)
        harness, output = self.start([sys.executable, "-c", HARNESS, "hang"])
        while not output.lines:
            threading.Event().wait(0.05)
        process_tree.kill(harness)
        harness.wait(30)
        process_tree.ended(harness)
        self.assertTrue(output.closed(30))
        self.assertIsNone(bystander.poll())

    def test_without_it_the_child_outlives_the_harness(self):
        """The control: the test does see a child that survives."""
        harness, output = self.start([sys.executable, "-c", HARNESS, "exit"], tied=False)
        self.addCleanup(self.stop_child, output)
        harness.wait(30)
        self.assertFalse(output.closed(2))

    @unittest.skipUnless(os.name == "nt", "a POSIX runner that is killed outright (SIGKILL) leaves its harnesses")
    def test_a_runner_that_dies_takes_its_harnesses_and_their_children(self):
        runner = subprocess.Popen([sys.executable, "-c", RUNNER, str(ROOT), HARNESS], stdout=subprocess.PIPE, text=True)
        self.addCleanup(runner.stdout.close)
        self.addCleanup(runner.wait)
        output = Output(runner)
        while not output.lines:
            self.assertIsNone(runner.poll())
            threading.Event().wait(0.05)
        runner.kill()  # TerminateProcess: no cleanup runs in the runner
        self.assertTrue(output.closed(30), "the runner's harness or its child outlived it")


if __name__ == "__main__":
    unittest.main()
