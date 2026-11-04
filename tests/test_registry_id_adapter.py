"""Renamed blocks retain every state's properties in both adapter directions."""
import unittest
from tools.generate_registry_id_adapter import states, table, state_alias, item_names, registry_names, reverse_table


class RenamedBlockStates(unittest.TestCase):
    def test_a_reused_projectile_name_does_not_change_the_old_breeze_projectile(self):
        old = {"minecraft:wind_charge": 0, "minecraft:arrow": 1}
        new = {"minecraft:arrow": 0, "minecraft:breeze_wind_charge": 1, "minecraft:wind_charge": 2}
        moved = {"minecraft:wind_charge": "minecraft:breeze_wind_charge"}
        self.assertEqual(table(old, new, reassign=moved), [1, 0])
        self.assertEqual(reverse_table(old, new, reassign=moved), [1, 0, 65535])

    def test_static_registry_names_follow_wire_ids_instead_of_alphabetical_report_order(self):
        self.assertEqual(registry_names({"minecraft:aqua_affinity": 2, "minecraft:protection": 0}),
                         ["minecraft:protection", "", "minecraft:aqua_affinity"])
        self.assertEqual(registry_names({}), [])

    def test_cooldown_names_follow_the_current_item_rename_and_reject_missing_entries(self):
        old = {"minecraft:stone": 0, "minecraft:chain": 1, "minecraft:removed": 2}
        new = {"minecraft:stone": 0, "minecraft:iron_chain": 1}
        self.assertEqual(item_names(old, new, {"minecraft:chain": "minecraft:iron_chain"}),
                         ["minecraft:stone", "minecraft:iron_chain", ""])

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

    def test_a_removed_variant_uses_the_surviving_type_without_changing_its_reverse_id(self):
        old = {"minecraft:creaking": 0, "minecraft:creaking_transient": 1}
        new = {"minecraft:creaking": 0}
        forward = {"minecraft:creaking_transient": "minecraft:creaking"}
        backward = {v: k for k, v in forward.items()}
        self.assertEqual(table(old, new, forward), [0, 0])
        self.assertEqual(table(new, old, backward), [0])

    def test_changed_properties_map_both_new_variants_back_to_the_old_state(self):
        specs = [
            "minecraft:creaking_heart[axis=y,creaking=active]="
            f"minecraft:creaking_heart[natural={natural},creaking_heart_state=awake,axis=y]"
            for natural in ("false", "true")
        ]
        pairs = [state_alias(spec) for spec in specs]
        old = {pairs[0][0]: 0}
        new = {pairs[0][1]: 0, pairs[1][1]: 1}
        forward, backward = {}, {}
        for a, b in pairs:
            forward.setdefault(a, b)
            backward.setdefault(b, a)
        self.assertEqual(table(old, new, forward), [0])
        self.assertEqual(table(new, old, backward), [0, 0])


if __name__ == "__main__":
    unittest.main()
