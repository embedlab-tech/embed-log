"""Tests for backend.cli.config_resolution — env var / CLI config precedence."""

from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path

from backend.cli.config_resolution import ENV_CONFIG_PATH, resolve_config_path


class ResolveConfigPathTests(unittest.TestCase):
    def setUp(self):
        self._saved = os.environ.get(ENV_CONFIG_PATH)
        os.environ.pop(ENV_CONFIG_PATH, None)

    def tearDown(self):
        os.environ.pop(ENV_CONFIG_PATH, None)
        if self._saved is not None:
            os.environ[ENV_CONFIG_PATH] = self._saved

    def test_cli_path_wins_when_no_env(self):
        self.assertEqual(
            resolve_config_path("/tmp/cli.yml"),
            Path("/tmp/cli.yml"),
        )

    def test_env_var_used_when_no_cli(self):
        os.environ[ENV_CONFIG_PATH] = "/tmp/env.yml"
        self.assertEqual(resolve_config_path(None), Path("/tmp/env.yml"))

    def test_cli_overrides_env(self):
        os.environ[ENV_CONFIG_PATH] = "/tmp/env.yml"
        self.assertEqual(
            resolve_config_path("/tmp/cli.yml"),
            Path("/tmp/cli.yml"),
        )

    def test_empty_cli_does_not_mask_env(self):
        os.environ[ENV_CONFIG_PATH] = "/tmp/env.yml"
        # argparse may hand us "" if the user types --config ""
        self.assertEqual(resolve_config_path(""), Path("/tmp/env.yml"))

    def test_whitespace_cli_does_not_mask_env(self):
        os.environ[ENV_CONFIG_PATH] = "/tmp/env.yml"
        self.assertEqual(resolve_config_path("   "), Path("/tmp/env.yml"))

    def test_none_when_nothing_set(self):
        self.assertIsNone(resolve_config_path(None))

    def test_none_when_only_empty_env(self):
        os.environ[ENV_CONFIG_PATH] = ""
        self.assertIsNone(resolve_config_path(None))

    def test_none_when_only_whitespace_env(self):
        os.environ[ENV_CONFIG_PATH] = "   "
        self.assertIsNone(resolve_config_path(None))

    def test_whitespace_in_env_is_trimmed(self):
        os.environ[ENV_CONFIG_PATH] = "  /tmp/spaced.yml  "
        self.assertEqual(resolve_config_path(None), Path("/tmp/spaced.yml"))

    def test_real_existing_path(self):
        with tempfile.NamedTemporaryFile(suffix=".yml", delete=False) as f:
            f.write(b"x: 1\n")
            p = Path(f.name)
        try:
            os.environ[ENV_CONFIG_PATH] = str(p)
            self.assertEqual(resolve_config_path(None), p)
        finally:
            p.unlink()

    def test_resolver_does_not_touch_filesystem(self):
        """A non-existent path still resolves — callers decide validity."""
        os.environ[ENV_CONFIG_PATH] = "/definitely/does/not/exist.yml"
        self.assertEqual(
            resolve_config_path(None),
            Path("/definitely/does/not/exist.yml"),
        )


class EnvConfigPathConstantTests(unittest.TestCase):
    def test_constant_name(self):
        self.assertEqual(ENV_CONFIG_PATH, "EMBED_LOG_CONFIG_YML_PATH")


if __name__ == "__main__":
    unittest.main()
