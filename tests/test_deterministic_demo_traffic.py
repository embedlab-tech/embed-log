import unittest

from utils.deterministic_demo_traffic import COAP_DEMO_HEX, CURATED_DEVICE_A, build_udp_lines


class DeterministicDemoTrafficTests(unittest.TestCase):
    def test_sensor_a_test_profile_emits_coap_demo_line(self):
        lines, next_seq = build_udp_lines("SENSOR_A", 4, 1)

        self.assertEqual(3, next_seq)
        self.assertEqual(2, len(lines))
        self.assertIn('kind=coap-demo', lines[1])
        self.assertIn('AA 55', lines[1])
        self.assertIn(COAP_DEMO_HEX, lines[1])

    def test_other_sources_do_not_emit_coap_demo_line(self):
        lines, _ = build_udp_lines("SENSOR_B", 4, 1)

        self.assertEqual(1, len(lines))
        self.assertNotIn('kind=coap-demo', lines[0])

    def test_curated_device_a_contains_readable_coap_payload(self):
        tick_four = CURATED_DEVICE_A[4]

        self.assertEqual(2, len(tick_four))
        self.assertIn('coap rx: frame AA 55 payload', tick_four[1])
        self.assertIn(COAP_DEMO_HEX, tick_four[1])


if __name__ == "__main__":
    unittest.main()
