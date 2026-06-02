from __future__ import annotations

import unittest
from pathlib import Path


class PackagingConfigTests(unittest.TestCase):
    def test_wheel_force_include_only_targets_external_trees(self):
        text = Path("pyproject.toml").read_text(encoding="utf-8")
        start = text.index("[tool.hatch.build.targets.wheel.force-include]")
        end = text.find("\n[", start + 1)
        section = text[start:] if end == -1 else text[start:end]

        self.assertIn('"frontend" = "frontend"', section)
        self.assertIn('"utils" = "utils"', section)
        self.assertNotIn('"backend/skills"', section)
        self.assertNotIn('"config-samples"', section)
        self.assertNotIn('"examples/embed-log.yml"', section)
        self.assertNotIn('"embed-log.demo.yml"', section)


if __name__ == "__main__":
    unittest.main()
