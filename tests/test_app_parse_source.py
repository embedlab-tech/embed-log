import unittest

from backend.app import parse_source, parse_source_config
from backend.sources import UartSource, UdpSource


class ParseSourceTests(unittest.TestCase):
    def test_uart_default_baud(self):
        src = parse_source("DUT", "uart:/dev/ttyUSB0", 115200)
        self.assertIsInstance(src, UartSource)
        self.assertEqual(src.port, "/dev/ttyUSB0")
        self.assertEqual(src.baudrate, 115200)

    def test_uart_explicit_baud(self):
        src = parse_source("DUT", "uart:/dev/ttyUSB0@921600", 115200)
        self.assertIsInstance(src, UartSource)
        self.assertEqual(src.baudrate, 921600)

    def test_uart_socket_url(self):
        src = parse_source("DUT", "uart:socket://127.0.0.1:12345@921600", 115200)
        self.assertIsInstance(src, UartSource)
        self.assertEqual(src.port, "socket://127.0.0.1:12345")
        self.assertEqual(src.baudrate, 921600)

    def test_udp(self):
        src = parse_source("DUT", "udp:6000", 115200)
        self.assertIsInstance(src, UdpSource)
        self.assertEqual(src.port, 6000)

    def test_cli_source_config_defaults_to_text_parser(self):
        cfg = parse_source_config("DUT", "uart:/dev/ttyUSB0", 115200)
        self.assertEqual(cfg["parser"], {"type": "text"})
    def test_invalid_spec_raises(self):
        with self.assertRaises(ValueError):
            parse_source("DUT", "bad-spec", 115200)

    def test_invalid_udp_port_raises(self):
        with self.assertRaises(ValueError):
            parse_source("DUT", "udp:not-a-port", 115200)


if __name__ == "__main__":
    unittest.main()
