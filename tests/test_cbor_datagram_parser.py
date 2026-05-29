"""Unit tests for the CBOR datagram parser.

These tests define the expected contract before the parser is implemented.
"""

import unittest

from backend.parsers.cbor_datagram import CborDatagramParser

try:
    import cbor2
except ImportError:
    cbor2 = None  # Tests fail here rather than silently skip


class CborDatagramParserTests(unittest.TestCase):
    """Valid CBOR map decodes into one expected text line."""

    def setUp(self):
        self.parser = CborDatagramParser()

    def _encode(self, obj) -> bytes:
        return cbor2.dumps(obj)

    # ---- decoding tests ----

    def test_valid_cbor_map_decodes(self):
        data = self._encode({"level": "INFO", "event": "temp", "value": 23.4,
                             "unit": "C", "tick": 17})
        lines = self.parser.feed(data)
        self.assertEqual(len(lines), 1)
        line = lines[0]
        self.assertIn("level=INFO", line)
        self.assertIn("event=temp", line)
        self.assertIn("value=23.4", line)
        self.assertIn("unit=C", line)
        self.assertIn("tick=17", line)

    def test_valid_cbor_map_deterministic_ordering(self):
        """Keys are sorted for deterministic output regardless of CBOR insertion order."""
        data_a = self._encode({"z": "last", "a": "first"})
        data_b = self._encode({"a": "first", "z": "last"})
        self.assertEqual(self.parser.feed(data_a), self.parser.feed(data_b))

    def test_missing_optional_field_does_not_crash(self):
        data = self._encode({"event": "test"})
        lines = self.parser.feed(data)
        self.assertEqual(len(lines), 1)
        self.assertIn("event=test", lines[0])

    def test_empty_map_produces_empty_line(self):
        data = self._encode({})
        lines = self.parser.feed(data)
        self.assertEqual(len(lines), 1)
        self.assertEqual(lines[0].strip(), "")

    # ---- type rejection tests ----

    def test_unsupported_top_level_type_list_raises(self):
        data = self._encode([1, 2, 3])
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_unsupported_top_level_type_text_raises(self):
        data = self._encode("hello")
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_unsupported_top_level_type_int_raises(self):
        data = self._encode(42)
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_unsupported_top_level_type_float_raises(self):
        data = self._encode(3.14)
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_unsupported_top_level_type_bytes_raises(self):
        data = self._encode(b"raw")
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_unsupported_top_level_type_bool_raises(self):
        data = self._encode(True)
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_unsupported_top_level_type_none_raises(self):
        data = self._encode(None)
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    # ---- malformed data tests ----

    def test_malformed_cbor_bytes_returns_empty(self):
        lines = self.parser.feed(b"\x81\x82\xff\xff\xff")
        self.assertEqual(lines, [])

    def test_empty_bytes_returns_empty(self):
        lines = self.parser.feed(b"")
        self.assertEqual(lines, [])

    def test_truncated_cbor_returns_empty(self):
        """Incomplete CBOR payload does not crash."""
        full = self._encode({"event": "test"})
        lines = self.parser.feed(full[:3])
        self.assertEqual(lines, [])

    def test_extra_bytes_after_cbor_returns_empty(self):
        """Trailing garbage after a valid CBOR object is rejected."""
        data = self._encode({"event": "test"}) + b"\xff\xff"
        lines = self.parser.feed(data)
        self.assertEqual(lines, [])

    def test_nested_map_still_works(self):
        """Nested dict values are formatted recursively."""
        data = self._encode({"event": "test", "nested": {"inner": "val"}})
        lines = self.parser.feed(data)
        self.assertEqual(len(lines), 1)
        self.assertIn("event=test", lines[0])
        self.assertIn("nested={\"inner\": \"val\"}", lines[0])

    # ---- formatting tests ----

    def test_string_values_are_not_quoted(self):
        data = self._encode({"msg": "hello"})
        lines = self.parser.feed(data)
        self.assertIn("msg=hello", lines[0])

    def test_integer_values_are_not_quoted(self):
        data = self._encode({"count": -5})
        lines = self.parser.feed(data)
        self.assertIn("count=-5", lines[0])

    def test_float_values_use_decimal_format(self):
        data = self._encode({"val": 3.14000})
        lines = self.parser.feed(data)
        self.assertIn("val=3.14", lines[0])

    def test_bool_values_are_lowercase(self):
        data = self._encode({"flag": True, "no": False})
        lines = self.parser.feed(data)
        self.assertIn("flag=True", lines[0])
        self.assertIn("no=False", lines[0])

    def test_null_values_rendered_as_none(self):
        data = self._encode({"field": None})
        lines = self.parser.feed(data)
        self.assertIn("field=None", lines[0])

    def test_list_values_are_json_formatted(self):
        data = self._encode({"tags": [1, "a", 3.0]})
        lines = self.parser.feed(data)
        self.assertIn("tags=[1, \"a\", 3.0]", lines[0])

    # ---- lifecycle tests ----

    def test_feed_returns_one_line_per_valid_datagram(self):
        data1 = self._encode({"event": "first"})
        data2 = self._encode({"event": "second"})
        lines1 = self.parser.feed(data1)
        lines2 = self.parser.feed(data2)
        self.assertEqual(len(lines1), 1)
        self.assertEqual(len(lines2), 1)
        self.assertIn("event=first", lines1[0])
        self.assertIn("event=second", lines2[0])

    def test_feed_does_not_buffer_across_calls(self):
        """No cross-datagram buffering — each feed call is independent."""
        data = self._encode({"event": "standalone"})
        self.parser.feed(data)
        lines = self.parser.feed(self._encode({"event": "other"}))
        self.assertEqual(len(lines), 1)
        self.assertIn("event=other", lines[0])

    def test_flush_returns_empty(self):
        """flush() returns [] for CborDatagramParser."""
        self.assertEqual(self.parser.flush(), [])

    def test_flush_after_feed_returns_empty(self):
        data = self._encode({"event": "test"})
        self.parser.feed(data)
        self.assertEqual(self.parser.flush(), [])
