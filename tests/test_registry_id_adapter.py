"""Renamed blocks retain every state's properties in both adapter directions."""
import unittest
from tools.generate_registry_id_adapter import states, table


class RenamedBlockStates(unittest.TestCase):
    def test_rename_matches_properties_instead_of_state_order(self):
        old = {"minecraft:chain": {"states": [
            {"id": 0, "properties": {"axis": "x", "waterlogged": "false"}},
            {"id": 1, "properties": {"axis": "y", "waterlogged": "false"}},
        ]}}
        new = {"minecraft:iron_chain": {"states": [
            {"id": 0, "properties": {"waterlogged": "false", "axis": "y"}},
            {"id": 1, "properties": {"waterlogged": "false", "axis": "x"}},
        ]}}
        source = states(old, {"minecraft:chain": "minecraft:iron_chain"})
        target = states(new)
        self.assertEqual(table(source, target), [1, 0])
        self.assertEqual(table(target, source), [1, 0])
        self.assertEqual(table(states(old), target), [65535, 65535])


if __name__ == "__main__":
    unittest.main()
