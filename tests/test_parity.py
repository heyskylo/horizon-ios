import re
import unittest
from pathlib import Path

README = Path(__file__).resolve().parents[1] / 'README.md'


class ParityTests(unittest.TestCase):
    def test_readme_percentage_matches_table(self):
        text = README.read_text()
        heading = re.search(r'^### Java parity: (\d+)%$', text, re.MULTILINE)
        if heading is None:
            self.assertNotRegex(text, r'badge/vanilla%20parity-\d+%25-')
            self.skipTest('This README publishes no numeric Java parity claim')
        table = text[heading.end():].split('| Subsystem | Score | Main gaps |', 1)[1].split('\n\n', 1)[0]
        scores = [float(score) for score in re.findall(r'^\| [^|]+ \| (0|0\.25|0\.5|0\.75|1) \| [^|]+ \|$', table, re.MULTILINE)]
        self.assertEqual(len(scores), table.count('\n|') - 1, 'Every row needs a score of 0, 0.25, 0.5, 0.75 or 1')
        self.assertEqual(int(heading.group(1)), round(sum(scores) * 100 / len(scores)))
        badge = re.search(r'badge/vanilla%20parity-(\d+)%25-', text)
        self.assertIsNotNone(badge, 'README parity badge is missing')
        self.assertEqual(badge.group(1), heading.group(1), 'The parity badge must match the table')


if __name__ == '__main__':
    unittest.main()
