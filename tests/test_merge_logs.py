import tempfile
import unittest
from pathlib import Path

from utils.merge_logs import generate_html, parse_log_file


class MergeLogsParseTests(unittest.TestCase):
    def test_parse_strips_leading_system_timestamp(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "a.log"
            path.write_text(
                "[2026-04-22T10:11:12.123+02:00] boot ok\n"
                "[2026-04-22T10:11:13.456+02:00] [TX::UI] ping\n"
                "[2026-04-22T10:11:14.000+02:00] [CONTROLLER] [SERIAL] payload\n",
                encoding="utf-8",
            )

            rows = parse_log_file(str(path), "CONTROLLER")

            self.assertEqual(3, len(rows))
            self.assertEqual("04-22 10:11:12.123", rows[0]["ts"])
            self.assertEqual("boot ok", rows[0]["text"])
            self.assertEqual("[TX::UI] ping", rows[1]["text"])
            self.assertTrue(rows[1]["isTx"])
            self.assertEqual("payload", rows[2]["text"])

    def test_parse_relative_timestamps(self):
        with tempfile.TemporaryDirectory() as td:
            path = Path(td) / "relative.log"
            path.write_text(
                "[T+00:00:00.000] boot ok\n"
                "[T+00:00:01.250] [TX::UI] ping\n"
                "[T+00:00:02.000] [CONTROLLER] [SERIAL] payload\n",
                encoding="utf-8",
            )

            rows = parse_log_file(str(path), "CONTROLLER")

            self.assertEqual(3, len(rows))
            self.assertEqual("T+00:00:00.000", rows[0]["ts"])
            self.assertEqual("boot ok", rows[0]["text"])
            self.assertEqual("T+00:00:01.250", rows[1]["ts"])
            self.assertTrue(rows[1]["isTx"])
            self.assertEqual("payload", rows[2]["text"])

    def test_generate_html_embeds_frontend_plugins(self):
        with tempfile.TemporaryDirectory() as td:
            log_path = Path(td) / "a.log"
            log_path.write_text(
                "[2026-04-22T10:11:12.123+02:00] prefix AABBCC 40011234B3666F6F03626172 suffix\n",
                encoding="utf-8",
            )

            html = generate_html(
                [{"label": "UART", "panes": [("A", "READER", str(log_path))]}],
                frontend_plugins={"hex-coap": {"kind": "line", "sha256": "abc"}},
                pane_plugins={"A": [{"name": "hex-coap", "options": {}}]},
                plugin_scripts={
                    "hex-coap": "window.EmbedLogPlugins.register({apiVersion:1,kind:'line',name:'hex-coap',analyzeLine(){return null;}});"
                },
            )

            self.assertIn("window.__embedLogFrontendPlugins", html)
            self.assertIn("window.__embedLogPanePlugins", html)
            self.assertIn("window.__embedLogPluginScripts", html)
            self.assertIn("pluginRuntime", html)
            self.assertIn("hex-coap", html)


if __name__ == "__main__":
    unittest.main()
