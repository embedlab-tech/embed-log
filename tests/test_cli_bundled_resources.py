import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from backend import cli
from backend.cli import demo as demo_cli


class BundledCliResourceTests(unittest.TestCase):
    def test_resolve_bundled_file_prefers_packaged_resource(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            packaged = root / "resources" / "examples" / "embed-log.yml"
            packaged.parent.mkdir(parents=True, exist_ok=True)
            packaged.write_text("version: 1\n", encoding="utf-8")

            with patch("backend.cli._bundled_resource_root", return_value=root / "resources"):
                with patch("backend.cli._repo_root", return_value=root / "missing-repo"):
                    resolved = cli._resolve_bundled_file(
                        "examples/embed-log.yml",
                        packaged_relative="examples/embed-log.yml",
                    )

        self.assertEqual(packaged, resolved)

    def test_main_init_writes_default_config_outside_repo(self):
        expected = (cli._repo_root() / "config-samples" / "double_uart_udp_two_tabs.yml").read_text(
            encoding="utf-8"
        )

        with tempfile.TemporaryDirectory() as td:
            cwd = Path(td)
            old = Path.cwd()
            try:
                os.chdir(cwd)
                rc = cli.main(["init"])
            finally:
                os.chdir(old)

            output_path = cwd / "embed-log.yml"
            self.assertEqual(rc, 0)
            self.assertTrue(output_path.is_file())
            self.assertEqual(expected, output_path.read_text(encoding="utf-8"))

    def test_resolve_demo_config_prefers_packaged_resource(self):
        demo_text = """
version: 1
sources:
  - name: SRC
    type: udp
    port: 6000
tabs:
  - label: Demo
    panes: [SRC]
""".strip()

        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            packaged = root / "resources" / "embed-log.demo.yml"
            packaged.parent.mkdir(parents=True, exist_ok=True)
            packaged.write_text(demo_text, encoding="utf-8")

            with patch("backend.cli._bundled_resource_root", return_value=root / "resources"):
                with patch("backend.cli._repo_root", return_value=root / "missing-repo"):
                    resolved = demo_cli._resolve_demo_config()

        self.assertEqual(packaged, resolved)


if __name__ == "__main__":
    unittest.main()
