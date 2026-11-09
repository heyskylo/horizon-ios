"""The narrow hopper proof retains diagnostics and rejects incomplete capture."""
import unittest
from tools.test_pot_hopper_parity import compare, recorded
from tools.pot_hopper_corpus import cases
from tools.command_exec_corpus import cmd, groups, ISOLATED_GROUPS


class PotHopperContract(unittest.TestCase):
    def test_full_raw_records_and_admission_failures_are_retained(self):
        ops = [dict(cmd('/execute if block 5 200 4 decorated_pot'), h282_admission=True),
               dict(cmd('/data get block 4 200 4'), diagnostic='saved cooldown')]
        reference = [{'results': [[True, 1]], 'styles': ['green']}, {'whole': 'java'}]
        native = [{'results': [[False, 0]], 'styles': ['red']}, {'whole': 'native'}]
        report = compare(ops, reference, native)
        self.assertEqual(report['strict_records'], 1)
        self.assertEqual(report['strict_matches'], 0)
        self.assertEqual(len(report['differences']), 2)
        self.assertEqual(report['failed_admissions'][0]['java'], reference[0])
        self.assertEqual(report['differences'][1]['native'], native[1])

    def test_incomplete_capture_is_rejected(self):
        with self.assertRaisesRegex(AssertionError, 'incomplete capture'):
            compare([cmd('/data get block 5 200 4 item')], [{}], [])

    def test_fixture_is_isolated_and_keeps_all_declared_rows(self):
        self.assertIn('hopper pot saved slot', ISOLATED_GROUPS)
        self.assertEqual(dict(groups())['hopper pot saved slot'], cases(cmd))
        ops = recorded(cases(cmd))
        self.assertEqual(len(ops), 200)
        self.assertEqual(sum(not op.get('diagnostic') for op in ops), 158)
        self.assertEqual(sum(bool(op.get('h282_admission')) for op in ops), 63)


if __name__ == '__main__':
    unittest.main()
