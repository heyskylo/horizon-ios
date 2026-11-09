import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
README = ROOT / "docs" / "README.md"
LINK = re.compile(r"\]\(([^)]+)\)")


class DocsReadmeLinksTests(unittest.TestCase):
    def targets(self):
        text = README.read_text(encoding="utf-8")
        return [target.split("#", 1)[0] for target in LINK.findall(text)]

    def test_relative_links_resolve(self):
        for target in self.targets():
            if not target or "://" in target or target.startswith(("mailto:", "#")):
                continue
            with self.subTest(target=target):
                self.assertTrue((README.parent / target).exists(), target)


if __name__ == "__main__":
    unittest.main()
