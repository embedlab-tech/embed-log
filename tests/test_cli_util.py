"""Tests for backend.cli.util — pure helper functions."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from backend.cli.util import (
    _ms3,
    count_lines,
    file_size_kb,
    format_duration,
    parse_duration,
    parse_log_timestamp,
    read_manifest,
    read_session_dir,
    resolve_session_id,
    short_alias,
)


class Ms3Tests(unittest.TestCase):
    def test_none_returns_zeros(self):
        self.assertEqual(_ms3(None), "000")

    def test_empty_string_returns_zeros(self):
        self.assertEqual(_ms3(""), "000")

    def test_short_fraction_padded(self):
        self.assertEqual(_ms3("1"), "100")
        self.assertEqual(_ms3("12"), "120")

    def test_exact_three_digits(self):
        self.assertEqual(_ms3("123"), "123")

    def test_long_fraction_truncated(self):
        self.assertEqual(_ms3("12345"), "123")


class ParseLogTimestampTests(unittest.TestCase):
    def test_standard_iso_timestamp(self):
        line = "[2026-01-15T10:30:45.123+01:00] some log message"
        result = parse_log_timestamp(line)
        self.assertEqual(result, "2026-01-15T10:30:45.123Z")

    def test_utc_z_suffix(self):
        line = "[2026-01-15T10:30:45.456Z] message"
        result = parse_log_timestamp(line)
        self.assertEqual(result, "2026-01-15T10:30:45.456Z")

    def test_no_timestamp(self):
        self.assertIsNone(parse_log_timestamp("no timestamp here"))

    def test_empty_line(self):
        self.assertIsNone(parse_log_timestamp(""))

    def test_fractional_seconds_with_comma(self):
        line = "[2026-01-15T10:30:45,789Z] message"
        result = parse_log_timestamp(line)
        self.assertEqual(result, "2026-01-15T10:30:45.789Z")

    def test_fractional_seconds_short(self):
        line = "[2026-01-15T10:30:45.1Z] message"
        result = parse_log_timestamp(line)
        self.assertEqual(result, "2026-01-15T10:30:45.100Z")

    def test_no_fractional_seconds(self):
        line = "[2026-01-15T10:30:45+00:00] message"
        result = parse_log_timestamp(line)
        self.assertEqual(result, "2026-01-15T10:30:45.000Z")

    def test_negative_timezone_offset(self):
        line = "[2026-01-15T10:30:45.000-05:00] message"
        result = parse_log_timestamp(line)
        self.assertEqual(result, "2026-01-15T10:30:45.000Z")


class FormatDurationTests(unittest.TestCase):
    def test_zero_seconds(self):
        self.assertEqual(format_duration(0), "0s")

    def test_under_one_minute(self):
        self.assertEqual(format_duration(45), "45s")

    def test_exact_minute(self):
        self.assertEqual(format_duration(60), "1m 0s")

    def test_minutes_and_seconds(self):
        self.assertEqual(format_duration(150), "2m 30s")

    def test_hours(self):
        self.assertEqual(format_duration(3600), "1h 0m 0s")

    def test_hours_and_minutes(self):
        self.assertEqual(format_duration(3750), "1h 2m 30s")

    def test_fractional_seconds_truncated(self):
        self.assertEqual(format_duration(45.9), "45s")


class ParseDurationTests(unittest.TestCase):
    def test_seconds(self):
        self.assertEqual(parse_duration("30s"), 30.0)

    def test_seconds_long_unit(self):
        self.assertEqual(parse_duration("30sec"), 30.0)

    def test_minutes(self):
        self.assertEqual(parse_duration("5m"), 300.0)

    def test_minutes_long_unit(self):
        self.assertEqual(parse_duration("5min"), 300.0)

    def test_hours(self):
        self.assertEqual(parse_duration("2h"), 7200.0)

    def test_hours_long_unit(self):
        self.assertEqual(parse_duration("2hr"), 7200.0)

    def test_days(self):
        self.assertEqual(parse_duration("1d"), 86400.0)

    def test_days_long_unit(self):
        self.assertEqual(parse_duration("1day"), 86400.0)

    def test_case_insensitive(self):
        self.assertEqual(parse_duration("5M"), 300.0)
        self.assertEqual(parse_duration("2H"), 7200.0)

    def test_with_spaces(self):
        self.assertEqual(parse_duration("5 m"), 300.0)

    def test_invalid_format(self):
        self.assertIsNone(parse_duration("abc"))
        self.assertIsNone(parse_duration(""))
        self.assertIsNone(parse_duration("5x"))
        self.assertIsNone(parse_duration("m5"))


class CountLinesTests(unittest.TestCase):
    def test_empty_file(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".log", delete=False) as f:
            path = Path(f.name)
        try:
            self.assertEqual(count_lines(path), 0)
        finally:
            path.unlink()

    def test_single_line(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".log", delete=False) as f:
            f.write("hello\n")
            path = Path(f.name)
        try:
            self.assertEqual(count_lines(path), 1)
        finally:
            path.unlink()

    def test_multiple_lines(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".log", delete=False) as f:
            f.write("line1\nline2\nline3\n")
            path = Path(f.name)
        try:
            self.assertEqual(count_lines(path), 3)
        finally:
            path.unlink()

    def test_missing_file_returns_zero(self):
        self.assertEqual(count_lines(Path("/nonexistent/file.log")), 0)


class FileSizeKbTests(unittest.TestCase):
    def test_small_file(self):
        with tempfile.NamedTemporaryFile(mode="w", suffix=".log", delete=False) as f:
            f.write("x" * 2048)
            path = Path(f.name)
        try:
            self.assertEqual(file_size_kb(path), 2)
        finally:
            path.unlink()

    def test_missing_file_returns_zero(self):
        self.assertEqual(file_size_kb(Path("/nonexistent/file.log")), 0)


class ShortAliasTests(unittest.TestCase):
    def test_deterministic(self):
        a = short_alias("2026-01-15_10-30-45")
        b = short_alias("2026-01-15_10-30-45")
        self.assertEqual(a, b)

    def test_fixed_length(self):
        self.assertEqual(len(short_alias("any-session-id")), 4)

    def test_different_ids_different_aliases(self):
        a = short_alias("session-aaa")
        b = short_alias("session-bbb")
        self.assertNotEqual(a, b)


class ResolveSessionIdTests(unittest.TestCase):
    def test_exact_match(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_dir = Path(tmp)
            sdir = log_dir / "2026-01-15_10-30-45"
            sdir.mkdir()
            result = resolve_session_id(log_dir, "2026-01-15_10-30-45")
            self.assertEqual(result, "2026-01-15_10-30-45")

    def test_alias_match(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_dir = Path(tmp)
            sid = "2026-01-15_10-30-45"
            (log_dir / sid).mkdir()
            alias = short_alias(sid)
            result = resolve_session_id(log_dir, alias)
            self.assertEqual(result, sid)

    def test_not_found(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = resolve_session_id(Path(tmp), "nonexistent")
            self.assertIsNone(result)

    def test_missing_log_dir(self):
        result = resolve_session_id(Path("/nonexistent/dir"), "anything")
        self.assertIsNone(result)


class ReadSessionDirTests(unittest.TestCase):
    def test_valid_session(self):
        with tempfile.TemporaryDirectory() as tmp:
            log_dir = Path(tmp)
            sdir = log_dir / "2026-01-15_10-30-45"
            sdir.mkdir()
            result = read_session_dir(log_dir, "2026-01-15_10-30-45")
            self.assertEqual(result, sdir)

    def test_not_found(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = read_session_dir(Path(tmp), "nonexistent")
            self.assertIsNone(result)


class ReadManifestTests(unittest.TestCase):
    def test_valid_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            sdir = Path(tmp)
            manifest = {"session_id": "test-123", "started_at": "2026-01-15T10:00:00"}
            (sdir / "manifest.json").write_text(json.dumps(manifest), encoding="utf-8")
            result = read_manifest(sdir)
            self.assertEqual(result["session_id"], "test-123")

    def test_missing_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            result = read_manifest(Path(tmp))
            self.assertIsNone(result)

    def test_invalid_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            sdir = Path(tmp)
            (sdir / "manifest.json").write_text("not json {{{", encoding="utf-8")
            result = read_manifest(sdir)
            self.assertIsNone(result)


if __name__ == "__main__":
    unittest.main()
