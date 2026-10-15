"""Run assembled player instructions with py65 and a byte-accurate REU stub.

Build with tools/build_player.py, then set MUPIC_PLAYER_BUILD to its output
directory. Requires py65; these checks do not qualify physical VIC timing.
"""
import os
from pathlib import Path
import re
import struct
import unittest

try:
    from py65.devices.mpu6502 import MPU
except ImportError:
    MPU = None

BUILD = Path(os.environ.get("MUPIC_PLAYER_BUILD", "build/player"))


class ReuMemory(bytearray):
    """Length-table DMA plus the sampler's write-only MMIO register file."""

    def __getitem__(self, key):
        if isinstance(key, int) and 0xDF20 <= key <= 0xDFFF:
            # Hardware reads report IRQ status/version, never written values.
            return 0x10 if key & 1 else 0
        return super().__getitem__(key)

    def written(self, address):
        return super().__getitem__(address)

    def __setitem__(self, key, value):
        super().__setitem__(key, value)
        if key == 0xDF01 and value == 0x91:
            source = int.from_bytes(self[0xDF04:0xDF07], "little")
            destination = int.from_bytes(self[0xDF02:0xDF04], "little")
            count = int.from_bytes(self[0xDF07:0xDF09], "little")
            assert source == 0x20000 + 3 * self.block
            assert count == 3
            super().__setitem__(slice(destination, destination + count),
                                self.length.to_bytes(3, "little"))


@unittest.skipUnless(MPU and (BUILD / "mupic-core.vs").is_file(),
                     "requires py65 and an assembled MUPIC_PLAYER_BUILD")
class PcmPlayer(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.symbols = {
            name: int(address, 16)
            for address, name in re.findall(
                r"al C:([0-9a-fA-F]+) \.([^\s]+)",
                (BUILD / "mupic-core.vs").read_text())
        }
        cls.core = (BUILD / "mupic-core.prg").read_bytes()
        cls.origin = int.from_bytes(cls.core[:2], "little")

    def machine(self, flags=1, block=0, length=35252, profile=0):
        memory = ReuMemory(65536)
        memory[self.origin:self.origin + len(self.core) - 2] = self.core[2:]
        memory.block, memory.length = block, length
        memory[0x500:0x600] = self.header(flags, profile)
        self.put(memory, "cache_read_block", block, 2)
        return MPU(memory=memory), memory

    @staticmethod
    def header(flags=1, profile=0):
        ticks = 4 if profile in (3, 4) else 3
        offset = 221184 if flags == 3 else 184320 if profile else 225280
        header = bytearray(256)
        header[:4] = b"MUP9"
        values = [(4, 1, 9), (5, 1, 9), (6, 1, 13), (7, 1, flags),
                  (8, 2, 256), (10, 2, 360), (12, 2, 200), (14, 2, 360 if profile else 356),
                  (16, 2, 200), (20, 1, ticks), (22, 2, 13), (24, 4, ticks),
                  (28, 4, 1), (32, 2, 140 if flags == 3 else 281),
                  (34, 2, 20), (36, 4, offset),
                  (40, 4, 262144), (44, 4, 256), (48, 4, 512),
                  (52, 4, 46080), (56, 4, 65536), (60, 4, 262144),
                  (64, 4, 327680), (68, 4, 1), (72, 2, ticks),
                  (76, 4, 44100 if flags == 3 else 22085), (80, 1, profile)]
        for offset, width, value in values:
            header[offset:offset + width] = value.to_bytes(width, "little")
        return header

    def put(self, memory, name, value, width=1):
        address = self.symbols[name]
        memory[address:address + width] = value.to_bytes(width, "little")

    def get(self, memory, name, width=1):
        address = self.symbols[name]
        return int.from_bytes(memory[address:address + width], "little")

    def call(self, cpu, name):
        cpu.pc = self.symbols[name]
        cpu.sp = 0xFD
        cpu.memory[0x1FE] = 0xFE
        cpu.memory[0x1FF] = 0x0F
        for _ in range(200000):
            cpu.step()
            if cpu.pc == 0x0FFF:
                return
        self.fail(f"{name} failed to return: ${cpu.pc:04x}")

    def validate(self, header):
        cpu, memory = self.machine()
        memory[0x500:0x600] = header
        self.call(cpu, "browser_validate_header")
        if not cpu.p & 1:
            self.call(cpu, "browser_validate_counts")
        return not cpu.p & 1

    def test_headers_require_matching_format_offsets(self):
        for flags in range(256):
            with self.subTest(flags=flags):
                self.assertEqual(self.validate(self.header(flags)), flags in (0, 1, 3))
        for flags, offset in [(1, 221184), (0, 221184), (3, 225280),
                              (3, 221185), (3, 221440), (3, 155648),
                              (3, 221184 + 0x1000000)]:
            header = self.header(flags)
            struct.pack_into("<I", header, 36, offset)
            with self.subTest(flags=flags, offset=offset):
                self.assertFalse(self.validate(header))

    def test_profiles_require_matching_width_cadence_and_audio_layout(self):
        for profile in range(5):
            for flags in (0, 1, 3):
                header = self.header(flags, profile)
                with self.subTest(profile=profile, flags=flags):
                    self.assertTrue(self.validate(header))
                    for offset, width, value in [(80, 1, 5), (14, 2, 358),
                                                 (20, 1, 4 if profile < 3 else 3)]:
                        bad = header.copy()
                        bad[offset:offset + width] = value.to_bytes(width, "little")
                        self.assertFalse(self.validate(bad))
                    if profile and flags != 3:
                        for rate in (7999, 48001):
                            bad = header.copy()
                            struct.pack_into("<I", bad, 76, rate)
                            self.assertFalse(self.validate(bad))
                        for divider in (127, 128, 129, 779, 780):
                            bad = header.copy()
                            struct.pack_into("<H", bad, 32, divider)
                            self.assertEqual(self.validate(bad), 128 <= divider <= 779)

    def test_wrong_machine_standard_is_rejected(self):
        for profile in range(5):
            for standard in (0, 1, 255):
                cpu, memory = self.machine(profile=profile)
                self.put(memory, "hardware_standard", standard)
                self.call(cpu, "browser_validate_machine")
                expected = standard == int(profile >= 3)
                self.assertEqual(not cpu.p & 1, expected)
                self.assertEqual(self.get(memory, "generic_error"), 0 if expected else 8)

    def test_profile_duration_uses_actual_refresh_clock(self):
        for profile, ticks, clock in [(0, 19656, 985248), (1, 19656, 985248),
                                      (2, 19656, 985248), (3, 17095, 1022730),
                                      (4, 17095, 1022730)]:
            for records in (3, 20, 502, 1795, 32760, 65532):
                cpu, memory = self.machine(profile=profile)
                struct.pack_into("<I", memory, 0x518, records)
                self.put(memory, "browser_list_ptr", 0x3A00, 2)
                self.call(cpu, "browser_format_metadata")
                seconds = (records * ticks + clock // 2) // clock
                expected = f"{seconds // 60:02}:{seconds % 60:02}".encode()
                self.assertEqual(memory[0x3A00:0x3A05], expected)
                self.assertEqual(memory[0x3A0F], profile)

    def test_cold_loader_installs_all_banked_segments(self):
        program = (BUILD / "mupic.prg").read_bytes()
        origin = int.from_bytes(program[:2], "little")
        memory = bytearray(65536)
        memory[origin:origin + len(program) - 2] = program[2:]
        cpu = MPU(memory=memory)
        cpu.pc = 0x080E
        for _ in range(300000):
            cpu.step()
            if cpu.pc == self.symbols["start"]:
                break
        else:
            self.fail("cold loader did not enter the player")
        ranges = [(0x0801, "resident_core_end"), (0x1000, "low_program_end"),
                  (0x1800, "browser_overlay_end"), (0xCA00, "generic_resident_end"),
                  (0xD100, "buffered_shadow_end"), (0xE800, "renderer_program_end"),
                  (0xFC00, "irq_program_end")]
        for start, label in ranges:
            end = self.symbols[label]
            with self.subTest(segment=label):
                self.assertEqual(memory[start:end],
                                 self.core[2 + start - self.origin:2 + end - self.origin])
        self.assertEqual(memory[1], 0x35)
        self.assertLess(origin + len(program) - 2, 0xCA00)

    def test_pcm8_rate_and_divider_boundaries_preserve_legacy(self):
        for flags in [0, 1, 3]:
            for rate in [0, 7999, 8000, 11025, 22050, 44100, 48000,
                         48001, 65536, 44100 + 0x1000000]:
                header = self.header(flags)
                struct.pack_into("<I", header, 76, rate)
                with self.subTest(flags=flags, rate=rate):
                    self.assertEqual(self.validate(header), flags != 3 or 8000 <= rate <= 48000)
            for divider in [1, 128, 129, 140, 255, 256, 779, 780, 781,
                            1024, 65535]:
                header = self.header(flags)
                struct.pack_into("<H", header, 32, divider)
                with self.subTest(flags=flags, divider=divider):
                    self.assertEqual(self.validate(header), flags != 3 or 129 <= divider <= 780)

    def test_length_table_checks_whole_stereo_frames_and_capacity(self):
        for flags, maximum, step, profile in [(1, 36864, 4, 0), (3, 40960, 2, 0),
                                               (1, 77824, 4, 1), (1, 77824, 4, 4)]:
            for block in [0, 255, 256, 1590, 3276]:
                for length, valid in [(step, True), (maximum - step, True),
                                      (maximum, True), (0, False),
                                      (step - 1, False), (maximum + step, False),
                                      (65536, maximum >= 65536)]:
                    with self.subTest(flags=flags, block=block, length=length):
                        cpu, memory = self.machine(flags, block, length, profile)
                        self.call(cpu, "generic_audio_length")
                        self.assertEqual(self.get(memory, "fatal_error") == 0, valid)

    def test_pause_keeps_whole_frames_and_rounds_ties_to_even(self):
        for flags, step, lengths in [(1, 4, [4, 35252, 35256, 36864, 65532, 65536, 76800, 77824]),
                                     (3, 2, [2, 35254, 38402, 40960])]:
            for block, blocks, ticks in [(0, 1591, 20), (255, 256, 9),
                                         (256, 1591, 20), (1590, 1591, 9)]:
                for length in lengths:
                    for elapsed in range(ticks + 1):
                        with self.subTest(flags=flags, block=block, length=length,
                                          elapsed=elapsed):
                            cpu, memory = self.machine(flags, block, length)
                            self.put(memory, "active_stream_blocks", blocks, 2)
                            self.put(memory, "active_audio_length", length, 3)
                            self.put(memory, "audio_tick_count", elapsed)
                            memory[0x500 + 72] = ticks
                            self.call(cpu, "prepare_audio_resume")
                            expected = min(length - step,
                                           step * round((length // step) * elapsed / ticks))
                            self.assertEqual(self.get(memory, "audio_resume_offset", 3), expected)

    def test_sampler_addresses_lengths_dividers_and_interleaved_width(self):
        for flags, step, maximum, offset, divider, control, profile in [
                (1, 4, 36864, 225280, 281, 0x51, 0),
                (3, 2, 40960, 221184, 140, 0x41, 0),
                (1, 4, 77824, 184320, 281, 0x51, 4)]:
            for bank in [0, 1]:
                for resume in [0, step, 252, 256, 32512, maximum - step]:
                    with self.subTest(flags=flags, bank=bank, resume=resume):
                        cpu, memory = self.machine(flags, 256, maximum, profile)
                        self.put(memory, "cache_read_bank", bank)
                        self.put(memory, "audio_resume_offset", resume, 3)
                        self.put(memory, "audio_resume_active", 1)
                        self.put(memory, "audio_tick_count", 19)
                        self.call(cpu, "audio_prepare")
                        self.call(cpu, "audio_start")
                        for voice in range(2):
                            base = 0xDF20 + 32 * voice
                            start = 0x1000000 + 0x100000 + bank * 262144 + offset
                            self.assertEqual(int.from_bytes(memory[base + 4:base + 8], "big"),
                                             start + resume + voice * (step // 2))
                            self.assertEqual(int.from_bytes(memory[base + 9:base + 12], "big"),
                                             maximum - resume)
                            self.assertEqual(int.from_bytes(memory[base + 14:base + 16], "big"), divider)
                            self.assertEqual(memory.written(base), control)
                            # Signed PCM8 is expanded into the sampler's high
                            # byte; changing sample width must not reduce gain.
                            self.assertEqual(memory.written(base + 1), 63)
                            self.assertEqual(memory.written(base + 2), voice * 15)
                        self.assertEqual(self.get(memory, "audio_tick_count"), 19)
                        self.assertEqual(self.get(memory, "audio_resume_active"), 0)
                        self.assertEqual(self.get(memory, "audio_resume_offset", 3), 0)
                        self.call(cpu, "audio_stop")
                        self.assertTrue(all(memory.written(address) == 0
                                            for address in [0xDF20, 0xDF21, 0xDF40, 0xDF41]))

    def test_invalid_length_cannot_enable_sampler(self):
        cpu, memory = self.machine(3, length=40962)
        for address in [0xDF20, 0xDF40]:
            memory[address] = 0x41
        for address in [0xDF21, 0xDF41]:
            memory[address] = 63
        self.call(cpu, "audio_prepare")
        self.call(cpu, "audio_start")
        self.assertEqual(self.get(memory, "fatal_error"), 7)
        self.assertEqual(memory.written(0xDF20), 0)
        self.assertEqual(memory.written(0xDF40), 0)
        self.assertEqual(memory.written(0xDF21), 0)
        self.assertEqual(memory.written(0xDF41), 0)


if __name__ == "__main__":
    unittest.main()
