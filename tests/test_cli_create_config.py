import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import yaml

from backend import cli


class CreateConfigWizardTests(unittest.TestCase):
    def test_detected_serial_ports_prefer_cu_and_dedupe_tty(self):
        class Port:
            def __init__(self, device, description):
                self.device = device
                self.description = description

        ports = [
            Port('/dev/tty.usbmodem1101', 'tty duplicate'),
            Port('/dev/cu.usbmodem1101', 'USB Modem'),
            Port('COM4', 'USB Serial'),
        ]

        with patch('backend.cli.list_ports.comports', return_value=ports):
            detected = cli._detected_serial_ports()

        self.assertEqual(
            detected,
            [
                {'device': 'COM4', 'label': 'USB Serial'},
                {'device': '/dev/cu.usbmodem1101', 'label': 'USB Modem'},
            ],
        )

    def test_create_config_wizard_writes_tab_first_yaml(self):
        with tempfile.TemporaryDirectory() as td:
            out = Path(td) / 'demo.yml'
            answers = iter([
                str(out),          # output path
                'demo-app',        # app name
                'y',               # open browser
                'demo-logs/',      # log dir
                '2',               # tab count
                'LOCK',            # tab 1 label
                '2',               # tab 1 panes
                'LOCK_MAIN',       # pane 1 name
                'uart',            # pane 1 type
                '1',               # choose detected uart port
                '230400',          # baudrate
                'LOCK_AUX',        # pane 2 name
                'udp',             # pane 2 type
                '7000',            # udp port
                'RADIO',           # tab 2 label
                '1',               # tab 2 panes
                'RADIO_RX',        # pane 1 name
                'uart',            # pane 1 type
                '/dev/ttyUSB9',    # manual port
                '115200',          # baudrate
            ])

            class Port:
                def __init__(self, device, description):
                    self.device = device
                    self.description = description

            detected = [Port('/dev/cu.usbmodem1101', 'USB Modem')]

            with patch('backend.cli.list_ports.comports', return_value=detected):
                rc = cli._run_create_config([], input_fn=lambda _prompt: next(answers))

            self.assertEqual(rc, 0)
            cfg = yaml.safe_load(out.read_text(encoding='utf-8'))
            self.assertEqual(cfg['server']['app_name'], 'demo-app')
            self.assertTrue(cfg['server']['open_browser'])
            self.assertEqual(cfg['logs']['dir'], 'demo-logs/')
            self.assertEqual(
                cfg['tabs'],
                [
                    {'label': 'LOCK', 'panes': ['LOCK_MAIN', 'LOCK_AUX']},
                    {'label': 'RADIO', 'panes': ['RADIO_RX']},
                ],
            )
            self.assertEqual(
                cfg['sources'],
                [
                    {
                        'name': 'LOCK_MAIN',
                        'type': 'uart',
                        'port': '/dev/cu.usbmodem1101',
                        'baudrate': 230400,
                    },
                    {
                        'name': 'LOCK_AUX',
                        'type': 'udp',
                        'port': 7000,
                    },
                    {
                        'name': 'RADIO_RX',
                        'type': 'uart',
                        'port': '/dev/ttyUSB9',
                        'baudrate': 115200,
                    },
                ],
            )


if __name__ == '__main__':
    unittest.main()
