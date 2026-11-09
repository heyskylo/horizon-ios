"""The network sound harnesses run with no arguments, as tools/parity.py starts them."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

TOOLS = Path(__file__).resolve().parents[1] / 'tools'

# Runs in its own interpreter: tools/ on sys.path would shadow tests/ modules of the same name.
PROBE = '''
import sys
from pathlib import Path
sys.path.insert(0, sys.argv[1])
harness = __import__(sys.argv[2])
local = Path(sys.argv[3])
harness.net.lab.LOCAL = local
seen = {}
class Reached(Exception):
    pass
def sides(name, binary=None):
    seen.update(name=name, binary=binary)
    raise Reached
harness.net.sides = sides
sys.argv = [sys.argv[2] + '.py', *(a.replace('{local}', str(local)) for a in sys.argv[4:])]
try:
    harness.main()
except Reached:
    pass
build = local / seen['name']
binary = seen['binary'] and seen['binary'].relative_to(local)
print(binary, '/'.join(build.relative_to(local).parts[:1]), build.is_dir())
'''


class SoundHarnessInvocationTest(unittest.TestCase):
    def run_harness(self, name, *arguments):
        with tempfile.TemporaryDirectory() as temporary:
            local = Path(temporary).resolve()
            result = subprocess.run([sys.executable, '-c', PROBE, str(TOOLS), name, str(local), *arguments],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            return result.stdout.split()

    def test_defaults_build_the_game_in_a_scratch_folder(self):
        for name, folder in [('test_mob_local_sound_parity', 'mob-local-sound'),
                             ('test_ominous_warning_sound_parity', 'ominous-warning-sound-parity')]:
            with self.subTest(harness=name):
                self.assertEqual(self.run_harness(name), ['None', folder, 'True'])

    def test_explicit_arguments_still_win(self):
        for name in ['test_mob_local_sound_parity', 'test_ominous_warning_sound_parity']:
            with self.subTest(harness=name):
                self.assertEqual(self.run_harness(name, '--binary', '{local}/game', '--work-dir', '{local}/chosen'),
                                 ['game', 'chosen', 'True'])


if __name__ == '__main__':
    unittest.main()
