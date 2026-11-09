import re
import unittest
from pathlib import Path

README = Path(__file__).resolve().parents[1] / 'README.md'


class ParityTableTests(unittest.TestCase):
    def test_every_row_has_a_score(self):
        text = README.read_text()
        heading = re.search(r'^### What the harnesses compare$', text, re.MULTILINE)
        self.assertIsNotNone(heading, 'README harness table heading is missing')
        table = text[heading.end():].split('| Subsystem | Score | Main gaps |', 1)[1].split('\n\n', 1)[0]
        scores = re.findall(r'^\| [^|]+ \| (0|0\.25|0\.5|0\.75|1) \| [^|]+ \|$', table, re.MULTILINE)
        self.assertGreater(len(scores), 0)
        self.assertEqual(len(scores), table.count('\n|') - 1, 'Every row needs a score of 0, 0.25, 0.5, 0.75 or 1')

    def test_no_parity_percentage(self):
        # Decided 2026-11-09: the README describes what the harnesses compare, never a parity percentage.
        text = README.read_text()
        self.assertNotIn('badge/vanilla%20parity-', text)
        self.assertNotRegex(text, r'(?i)parity:? \d+%')


if __name__ == '__main__':
    unittest.main()
