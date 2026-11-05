import unittest

from tools import registry_backfill_owner_ids as bf

ENTRY = 'id = "coords_hud"\nkind = "mod"\nname = "Coordinates HUD"\nowners = ["alice", "Bob"]\nlicense = "MIT"\n'
USERS = {"alice": {"id": 7, "login": "alice", "type": "User"}, "bob": {"id": 8, "login": "Bob", "type": "User"}}


def resolve(login):
    return USERS.get(login.lower())


class BackfillTests(unittest.TestCase):
    def test_every_owner_resolved_is_written_after_owners(self):
        ids, problems = bf.plan(ENTRY, "", resolve)
        self.assertEqual((ids, problems), ([7, 8], []))
        self.assertIn('owners = ["alice", "Bob"]\nowner_ids = [7, 8]\nlicense', bf.with_owner_ids(ENTRY, ids))

    def test_an_entry_with_ids_is_left_alone(self):
        self.assertEqual(bf.plan(bf.with_owner_ids(ENTRY, [7, 8]), "", resolve), (None, []))

    def test_an_unknown_login_leaves_the_whole_entry_alone(self):
        ids, problems = bf.plan(ENTRY.replace('"Bob"', '"gone"'), "", resolve)
        self.assertIsNone(ids)
        self.assertIn("no such user", problems[0])

    def test_a_login_that_changed_hands_is_reported_not_written(self):
        log = ("Update coords_hud 0.2.0\n\nSubmitted by @alice through the form.\n\n"
               "Co-authored-by: alice <99+alice@users.noreply.github.com>\n")
        ids, problems = bf.plan(ENTRY, log, resolve)
        self.assertIsNone(ids)
        self.assertIn("form recorded [99]", problems[0])
        # The same account the form recorded: written.
        ok = log.replace("99+alice", "7+alice")
        self.assertEqual(bf.plan(ENTRY, ok, resolve), ([7, 8], []))

    def test_owners_not_on_one_line_are_left_for_a_person(self):
        ids, problems = bf.plan(ENTRY.replace('owners = ["alice", "Bob"]', 'owners = [\n  "alice",\n]'), "", resolve)
        self.assertIsNone(ids)
        self.assertIn("by hand", problems[0])


if __name__ == "__main__":
    unittest.main()
