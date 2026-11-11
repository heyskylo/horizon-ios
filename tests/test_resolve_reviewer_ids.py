"""tools/ci/resolve_reviewer_ids.py: the numeric GitHub IDs the comment commands decide by."""
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools.ci import resolve_reviewer_ids as resolve


class ReviewerListTests(unittest.TestCase):
    """The check reads the list and the owner's ID; it never writes them."""

    def _in(self, list_text, nightly_text, accounts=None):
        accounts = accounts or {}
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.multiple(resolve, LIST=Path(folder, "reviewers.txt"),
                                    NIGHTLY_SET=Path(folder, "nightly_set.py")), \
                mock.patch.object(resolve, "user", side_effect=lambda ref: accounts.get(ref)):
            Path(folder, "reviewers.txt").write_text(list_text, encoding="utf-8")
            Path(folder, "nightly_set.py").write_text(nightly_text, encoding="utf-8")
            errors = []
            with mock.patch.object(resolve, "print", create=True):
                code = resolve.check()
            return code

    OWNER = 'OWNER = "ludovic111"\nOWNER_ID: int | None = 136255825\n'

    def test_a_list_of_ids_and_logins_is_accepted(self):
        code = self._in("136255825 ludovic111\n", self.OWNER,
                        {"user/136255825": {"id": 136255825, "login": "ludovic111", "type": "User"}})
        self.assertEqual(code, 0)

    def test_a_line_without_an_id_is_reported(self):
        code = self._in("ludovic111\n", self.OWNER,
                        {"user/136255825": {"id": 136255825, "login": "ludovic111", "type": "User"}})
        self.assertEqual(code, 1)

    def test_an_id_that_belongs_to_another_login_is_reported(self):
        code = self._in("136255825 ludovic111\n", self.OWNER,
                        {"user/136255825": {"id": 136255825, "login": "someone-else", "type": "User"}})
        self.assertEqual(code, 1)

    def test_an_id_github_does_not_know_is_reported(self):
        code = self._in("999999999 ludovic111\n", self.OWNER,
                        {"user/136255825": {"id": 136255825, "login": "ludovic111", "type": "User"}})
        self.assertEqual(code, 1)

    def test_the_paused_line_is_not_a_problem(self):
        code = self._in("paused\n", self.OWNER,
                        {"user/136255825": {"id": 136255825, "login": "ludovic111", "type": "User"}})
        self.assertEqual(code, 0)

    def test_an_owner_id_that_is_not_set_is_reported(self):
        code = self._in("136255825 ludovic111\n", 'OWNER = "ludovic111"\n',
                        {"user/136255825": {"id": 136255825, "login": "ludovic111", "type": "User"}})
        self.assertEqual(code, 1)


class OwnerLoginTests(unittest.TestCase):
    def test_the_owner_is_read_from_the_nightly_set(self):
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.object(resolve, "NIGHTLY_SET", Path(folder, "nightly_set.py")):
            Path(folder, "nightly_set.py").write_text('OWNER = "ludovic111"\n', encoding="utf-8")
            self.assertEqual(resolve.owner_login(), "ludovic111")

    def test_a_nightly_set_without_an_owner_is_refused(self):
        with tempfile.TemporaryDirectory() as folder, \
                mock.patch.object(resolve, "NIGHTLY_SET", Path(folder, "nightly_set.py")):
            Path(folder, "nightly_set.py").write_text("# nothing here\n", encoding="utf-8")
            with self.assertRaises(SystemExit):
                resolve.owner_login()


class UserTests(unittest.TestCase):
    """The lookup asks GitHub for a login or for a numeric ID."""

    def test_a_login_is_asked_for_by_name(self):
        with mock.patch.object(resolve.subprocess, "run") as run:
            run.return_value = mock.Mock(returncode=0, stdout='{"id": 1, "login": "ludovic111", "type": "User"}')
            found = resolve.user("ludovic111")
        self.assertEqual(found["login"], "ludovic111")
        self.assertIn("users/ludovic111", run.call_args.args[0])

    def test_an_id_is_asked_for_by_number(self):
        with mock.patch.object(resolve.subprocess, "run") as run:
            run.return_value = mock.Mock(returncode=0, stdout='{"id": 1, "login": "ludovic111", "type": "User"}')
            resolve.user("user/1")
        self.assertIn("user/1", run.call_args.args[0])

    def test_an_account_github_does_not_know_is_none(self):
        with mock.patch.object(resolve.subprocess, "run") as run:
            run.return_value = mock.Mock(returncode=1, stdout="")
            self.assertIsNone(resolve.user("nobody"))


if __name__ == "__main__":
    unittest.main()
