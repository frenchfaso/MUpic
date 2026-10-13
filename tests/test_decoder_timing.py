"""Keep the encoder's calibrated costs tied to assembled production instructions.

Uses the same optional py65 / MUPIC_PLAYER_BUILD setup as test_pcm8_player.
These tests cover CPU instructions and REU stalls, not physical VIC timing.
"""
import re
import unittest

from test_pcm8_player import BUILD, MPU


class TimedMPU(MPU if MPU else object):
    # py65 1.2.0 incorrectly declares DEC absolute as 3 cycles; NMOS and the
    # production emulator both take 6. Keep the correction private to this test.
    cycletime = list(MPU.cycletime) if MPU else []
    if MPU:
        cycletime[0xCE] = 6


class DecoderMemory(bytearray):
    def __init__(self):
        super().__init__(65536)
        self.regs = bytearray(32)
        self.reu = bytearray(0x200000)
        self.dma_bytes = 0

    def io_visible(self):
        port = super().__getitem__(1)
        return bool(port & 4 and port & 3)

    def __getitem__(self, key):
        if isinstance(key, int) and 0xDF00 <= key < 0xDF20 and self.io_visible():
            return self.regs[key - 0xDF00]
        return super().__getitem__(key)

    def __setitem__(self, key, value):
        if isinstance(key, int) and 0xDF00 <= key < 0xDF20 and self.io_visible():
            self.regs[key - 0xDF00] = value
            if key == 0xDF01 and value == 0x91:
                self.transfer()
            return
        super().__setitem__(key, value)
        if key == 0xFF00 and self.regs[1] == 0x81:
            self.transfer()

    def transfer(self):
        source = int.from_bytes(self.regs[4:7], 'little')
        dest = int.from_bytes(self.regs[2:4], 'little')
        count = int.from_bytes(self.regs[7:9], 'little') or 65536
        self.dma_bytes += count
        super().__setitem__(slice(dest, dest + count), self.reu[source:source + count])
        self.regs[4:7] = (source + count).to_bytes(3, 'little')
        self.regs[2:4] = ((dest + count) & 65535).to_bytes(2, 'little')
        self.regs[7:9] = b'\0\0'
        self.regs[1] &= 0x7F


@unittest.skipUnless(MPU and (BUILD / 'mupic-core.vs').is_file(),
                     'requires py65 and an assembled MUPIC_PLAYER_BUILD')
class DecoderTiming(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.symbols = {name: int(address, 16) for address, name in re.findall(
            r'al C:([0-9a-fA-F]+) \.([^\s]+)',
            (BUILD / 'mupic-core.vs').read_text())}
        cls.core = (BUILD / 'mupic-core.prg').read_bytes()
        cls.origin = int.from_bytes(cls.core[:2], 'little')

    def put(self, memory, name, value, width=1):
        address = self.symbols[name]
        memory[address:address + width] = value.to_bytes(width, 'little')

    def get(self, memory, name, width=1):
        address = self.symbols[name]
        return int.from_bytes(memory[address:address + width], 'little')

    def call(self, cpu, name):
        cpu.pc = self.symbols[name]
        cpu.sp = 0xFD
        cpu.memory[0x1FE:0x200] = b'\xFE\x0F'
        for _ in range(500000):
            cpu.step()
            if cpu.pc == 0x0FFF:
                return
        self.fail(f'{name} did not return: ${cpu.pc:04x}')

    def test_instruction_layout_matches_cycle_model(self):
        self.assertEqual(self.symbols['dict_words'], 0xD368)
        self.assertEqual(self.symbols['dict_literal_length_valid'], 0x1445)
        self.assertEqual(self.symbols['lz_copy_group'], 0xDB00)
        self.assertEqual(self.symbols['lz_copy_forward'], 0xDA00)
        self.assertEqual(self.symbols['lz_prefix_divide'], 0x1545)

    def test_bounded_prefix_division_preserves_metadata_and_x(self):
        memory = DecoderMemory()
        memory[self.origin:self.origin + len(self.core) - 2] = self.core[2:]
        self.put(memory, 'cache_ready', 1)
        cpu = TimedMPU(memory=memory)
        for body in [*range(2600), 2600, 2815, 2816, 65530]:
            with self.subTest(body=body):
                memory[1] = 0x34
                if body < 2600:
                    self.put(memory, 'lz_prefetch_count', body, 2)
                    self.put(memory, 'lz_prefetch_rows', 0)
                    cpu.x = (body * 17) & 255
                    self.call(cpu, 'lz_prefix_divide')
                    self.assertEqual(cpu.x, (body * 17) & 255)
                    self.assertEqual(self.get(memory, 'lz_prefetch_count', 2), body % 13)
                    self.assertEqual(self.get(memory, 'lz_prefetch_rows'), body // 13)
                self.put(memory, 'cache_cursor', 0, 2)
                memory.reu[0x100000:0x100004] = (body + 2).to_bytes(2, 'little') + b'\0\0'
                self.call(cpu, 'lz_prepare_next')
                count = min(body, 2600) // 13 * 13
                self.assertEqual(self.get(memory, 'lz_prefetch_count', 2), count)
                self.assertEqual(self.get(memory, 'lz_prefetch_rows'), count // 13)
                self.assertEqual(self.get(memory, 'lz_prefetch_end_row'), count // 13)
                self.assertEqual(self.get(memory, 'record_remaining', 2), body)

    def test_record_instruction_and_dma_costs(self):
        # Shared with sd_timing.rs and independently confirmed by the actual
        # vendored emulator. Includes prefix-only, suffix, crossing, RAW,
        # literals, SKIP, dictionary reset and bank-marker lookahead paths.
        cases = [
            (False, False, 0, 0, 0, 2587, False, 1079),
            (True, False, 0, 0, 0, 2587, True, 16811),
            (True, False, 1, 0, 0, 1024, False, 21152),
            (False, False, 1, 10, 0, 1024, False, 7591),
            (False, False, 30, 10, 0, 2600, False, 150882),
            (True, False, 60, 10, 0, 2587, True, 551953),
            (False, True, 1, 0, 0, 1024, False, 4968),
            (False, True, 30, 0, 0, 2587, True, 273740),
            (True, True, 30, 0, 0, 2600, False, 338155),
            (True, False, 80, 0, 40, 2587, False, 377432),
        ]
        for reset, raw, count, escapes, skips, next_body, switch, expected in cases:
            with self.subTest(reset=reset, raw=raw, count=count, escapes=escapes,
                              skips=skips, next_body=next_body, switch=switch):
                body = bytearray(1024 if reset else 0)
                for index in range(count):
                    body.extend([index, 255 if raw else escapes * 5])
                    if raw:
                        body.extend(bytes(200))
                    else:
                        body.extend([255] * escapes + [254] * skips +
                                    [0] * (50 - escapes - skips))
                        body.extend(value for word in range(escapes)
                                    for value in [word * 4, 1, 2, 3, 4])
                memory = DecoderMemory()
                memory[self.origin:self.origin + len(self.core) - 2] = self.core[2:]
                memory[1] = 0x35
                prefix = min(len(body), 2600) // 13 * 13
                current = (len(body) + 2).to_bytes(2, 'little') + bytes([2 if reset else 0, count]) + body
                next_record = (next_body + 2).to_bytes(2, 'little') + bytes(next_body + 2)
                stream = current + (bytes(4) if switch else next_record)
                memory.reu[0x100000:0x100000 + len(stream)] = stream
                if switch:
                    memory.reu[0x140000:0x140000 + len(next_record)] = next_record
                    memory[self.symbols['cache_ready'] + 1] = 1
                memory[0xF180:0xF180 + prefix] = body[:prefix]
                for name, value, width in [('lz_record_prepared', 1, 1),
                                           ('lz_prefetch_count', prefix, 2),
                                           ('record_flags', 2 if reset else 0, 1),
                                           ('dict_column_total', count, 1),
                                           ('record_remaining', len(body), 2),
                                           ('cache_cursor', 4, 2), ('cache_ready', 1, 1)]:
                    self.put(memory, name, value, width)
                memory[self.symbols['record_count_compare_high'] + 1] = 0xFF
                memory[self.symbols['record_count_compare_low'] + 1] = 0xFF
                cpu = TimedMPU(memory=memory)
                self.call(cpu, 'buffered_blank_update')
                self.assertEqual(memory[self.symbols['fatal_error']], 0)
                self.assertEqual(cpu.processorCycles + 63 * memory.dma_bytes, expected)
