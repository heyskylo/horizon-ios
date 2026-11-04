import struct
import unittest

from tools import check_pe_arch


def pe(machine):
    image = bytearray(0x86)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, 0x80)
    image[0x80:0x84] = b"PE\0\0"
    struct.pack_into("<H", image, 0x84, machine)
    return image


class CheckPEArchTests(unittest.TestCase):
    def test_reads_x64_and_arm64_machine_values(self):
        self.assertEqual(check_pe_arch.pe_machine(pe(0x8664)), 0x8664)
        self.assertEqual(check_pe_arch.pe_machine(pe(0xAA64)), 0xAA64)

    def test_rejects_non_pe_and_truncated_headers(self):
        with self.assertRaisesRegex(ValueError, "MZ"):
            check_pe_arch.pe_machine(b"not PE")
        image = pe(0xAA64)
        struct.pack_into("<I", image, 0x3C, 0xFFFFFFF0)
        with self.assertRaisesRegex(ValueError, "PE signature"):
            check_pe_arch.pe_machine(image)


if __name__ == "__main__":
    unittest.main()
