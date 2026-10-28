#!/usr/bin/env python3
"""Add read-only string-section bytes from the matching ELF to a Zephyr v3 DB.

Requires pyelftools (pip install pyelftools). Does not change the wire format or
firmware. Raw bytes let the decoder resolve pooled empty strings inside binary
constants, which cannot be reconstructed from string_mappings alone.
"""
import argparse
import base64
import json
from pathlib import Path


def enrich(database, elf_path):
    from elftools.elf.elffile import ELFFile

    if database.get("version") != 3:
        raise ValueError("only Zephyr dictionary version 3 is supported")
    with Path(elf_path).open("rb") as stream:
        elf = ELFFile(stream)
        target = database.get("target", {})
        if target.get("bits", 32) != elf.elfclass or target.get("little_endianness", True) != elf.little_endian:
            raise ValueError("ELF and dictionary target differ")
        sections = []
        for section in elf.iter_sections():
            name = section.name.lstrip(".")
            if not (name in ("rodata", "srodata", "log_strings") or name.startswith(("rodata.", "srodata."))):
                continue
            if section["sh_type"] != "SHT_PROGBITS" or section["sh_flags"] & 1:
                continue
            sections.append((section.name, section["sh_addr"], section.data()))
    if not sections:
        raise ValueError("ELF has no read-only string sections")
    checked = 0
    for address, text in database.get("string_mappings", {}).items():
        address = int(address)
        for name, start, data in sections:
            offset = address - start
            if 0 <= offset < len(data):
                expected = text.encode("utf-8") + b"\0"
                if data[offset:offset + len(expected)] != expected:
                    raise ValueError(f"ELF/dictionary string mismatch at 0x{address:x} ({name}); use the matching build")
                checked += 1
                break
    if not checked:
        raise ValueError("no dictionary strings could be verified against the ELF")
    result = dict(database)
    raw = dict(database.get("sections", {}))
    for name, start, data in sections:
        raw[name] = {"start": start, "size": len(data), "data_b64": base64.b64encode(data).decode("ascii")}
    result["sections"] = raw
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--database", required=True, type=Path)
    parser.add_argument("--elf", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path, help="Output JSON (may replace input after successful verification)")
    args = parser.parse_args()
    try:
        result = enrich(json.loads(args.database.read_text(encoding="utf-8")), args.elf)
    except (ValueError, OSError, ImportError) as error:
        parser.exit(1, f"enrich_zephyr_dictionary: {error}\n")
    # No output is changed until ELF/dictionary validation has succeeded.
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
