import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
README = ROOT / "docs" / "README.md"
LINK = re.compile(r"\]\(([^)]+)\)")

BENCHMARK_PAGES = [
    "animated-sprite-mips-benchmark.md",
    "generation-scan-benchmark.md",
    "noise-aquifer-benchmark.md",
    "portal-scan-benchmark.md",
    "random-tick-eligibility-benchmark.md",
    "shared-model-library-benchmark.md",
    "surface-generation-benchmark.md",
    "visibility-benchmark.md",
    "media/entity-culling-benchmark.md",
    "performance/lighting-worker-results.md",
    "performance/paletted-container-access-results.md",
    "performance/startup-stage-3.md",
]


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

    def test_benchmark_pages_are_indexed(self):
        linked = set(self.targets())
        for page in BENCHMARK_PAGES:
            with self.subTest(page=page):
                self.assertIn(page, linked)


if __name__ == "__main__":
    unittest.main()
