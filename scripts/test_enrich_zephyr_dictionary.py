"""Small portable ELF fixtures; run: python -m unittest discover -s scripts -p 'test_enrich_*.py'."""
import base64
import json
from pathlib import Path
import struct
import subprocess
import sys
import tempfile
import unittest

from enrich_zephyr_dictionary import enrich


def elf_fixture(bits):
    names = b"\0.rodata\0.shstrtab\0"
    data = b"hello\0\x2a\x86\x00\xce\x3d"
    size = 64 if bits == 64 else 52
    ro_offset, names_offset = size, size + len(data)
    table_offset = names_offset + len(names)
    ident = b"\x7fELF" + bytes([2 if bits == 64 else 1, 1, 1]) + bytes(9)
    if bits == 64:
        header = struct.pack("<16sHHIQQQIHHHHHH",ident,2,62,1,0,0,table_offset,0,size,0,0,64,3,2)
        sh = lambda n,t,f,a,o,s: struct.pack("<IIQQQQIIQQ",n,t,f,a,o,s,0,0,1,0)
    else:
        header = struct.pack("<16sHHIIIIIHHHHHH",ident,2,40,1,0,0,table_offset,0,size,0,0,40,3,2)
        sh = lambda n,t,f,a,o,s: struct.pack("<IIIIIIIIII",n,t,f,a,o,s,0,0,1,0)
    return header + data + names + sh(0,0,0,0,0,0) + sh(1,1,2,0x2000,ro_offset,len(data)) + sh(9,3,0,0,names_offset,len(names))


class EnrichmentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.elf = self.root / "firmware.elf"
        self.elf.write_bytes(elf_fixture(32))
        self.db = {"version":3,"target":{"bits":32,"little_endianness":True},"string_mappings":{"8192":"hello"}}

    def test_32_and_64_bit_elf_bytes_preserve_binary_nul(self):
        for bits in (32,64):
            self.elf.write_bytes(elf_fixture(bits))
            self.db["target"]["bits"] = bits
            result = enrich(self.db,self.elf)
            section = result["sections"][".rodata"]
            self.assertEqual(section["start"],0x2000)
            self.assertEqual(base64.b64decode(section["data_b64"]),b"hello\0\x2a\x86\x00\xce\x3d")
            self.assertNotIn("sections",self.db)

    def test_wrong_target_is_rejected(self):
        self.db["target"]["bits"] = 64
        with self.assertRaisesRegex(ValueError,"target differ"):
            enrich(self.db,self.elf)

    def test_wrong_firmware_is_rejected(self):
        self.db["string_mappings"]["8192"] = "other build"
        with self.assertRaisesRegex(ValueError,"mismatch"):
            enrich(self.db,self.elf)

    def test_no_matching_strings_is_rejected(self):
        self.db["string_mappings"] = {"4096":"outside section"}
        with self.assertRaisesRegex(ValueError,"no dictionary strings"):
            enrich(self.db,self.elf)

    def test_failure_does_not_overwrite_existing_output(self):
        source = self.root / "dictionary.json"
        output = self.root / "enriched.json"
        self.db["string_mappings"]["8192"] = "wrong"
        source.write_text(json.dumps(self.db))
        output.write_text("keep existing output")
        completed = subprocess.run([sys.executable,str(Path(__file__).with_name("enrich_zephyr_dictionary.py")),"--database",str(source),"--elf",str(self.elf),"--output",str(output)],capture_output=True)
        self.assertNotEqual(completed.returncode,0)
        self.assertEqual(output.read_text(),"keep existing output")


if __name__ == "__main__":
    unittest.main()
