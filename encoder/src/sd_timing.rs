//! NMOS instruction costs of the production codec9 player.
//!
//! RAM and REU costs use the vendored PAL/64 MHz emulator's timing: one CPU
//! cycle is one turbo slot and a DMA byte stalls the CPU for 63 slots. Helpers
//! reserve only their data-dependent branch/carry variations. Their assembled
//! instruction paths are checked independently with py65 and the emulator.

pub(super) const DMA_SLOTS: usize = 63;
const PREFETCH_ADDRESS: usize = 0xf180;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Cost {
    pub(super) cpu: usize,
    pub(super) dma_bytes: usize,
}
impl Cost {
    pub(super) fn slots(self) -> usize {
        self.cpu + DMA_SLOTS * self.dma_bytes
    }
}

// buffered_blank_update entry, apply-buffered-record setup, optional reset
// state, prefix cursor publication, record completion and return. Includes
// cache-cursor/record-index carries and the longer record-count comparison.
// Column loops, LUT transfer and lookahead are charged separately.
// At most nine consume-length calls take their nine-cycle low-byte comparison:
// four minimum-size (52-byte) columns fit in the final 255 bytes, plus one
// longer body or the LUT can need the comparison. Reserve it once per packet.
pub(super) const RECORD_CPU: usize = 280 + 9 * 9;

fn page_reads(low: usize, count: usize) -> usize {
    (low + count).saturating_sub(256)
}

#[derive(Clone, Debug)]
pub(super) struct Primitive {
    base: usize,
    words: usize,
    escaped: usize,
}
impl Primitive {
    pub(super) fn new(code: &[u8], escaped: usize) -> Self {
        let dictionary = code.iter().filter(|&&value| value < 254).count();
        let control = code.len() - dictionary;
        // At dict_words=$d368 only these six taken BCS instructions cross a page.
        let branches = code
            .iter()
            .enumerate()
            .filter(|&(index, &value)| value >= 254 && index % 8 == 4)
            .count();
        let base = 33
            + 16 * code.len()
            + 45 * dictionary
            + 14 * control
            + branches
            + if escaped > 0 {
                4 + 70 * (escaped / 5)
            } else {
                0
            };
        Self {
            base,
            words: code.len(),
            escaped,
        }
    }
    pub(super) fn cycles(&self, source: usize) -> usize {
        self.base
            + page_reads(source & 255, self.words)
            + page_reads((source + self.words) & 255, self.escaped)
    }
}

// lz_copy_forward, including RTS. The large loop uses absolute,X reads from
// four patched addresses; unlike an indirect,Y loop its page penalties are
// determined by the patched bases and descending-page starting X.
fn copy_forward(count: usize, source: usize) -> usize {
    debug_assert!((1..=255).contains(&count));
    let low = source & 255;
    if count < 40 {
        return 70 + 18 * count + page_reads(low, count);
    }
    let remainder = count % 4;
    let aligned = count - remainder;
    let groups = aligned / 4;
    let mut cycles = if remainder == 0 {
        294
    } else {
        347 + 18 * remainder + page_reads(low, remainder)
    };
    cycles += 47 * groups;
    for component in 0..4 {
        let base = (low + remainder + aligned + component) & 255;
        let first = aligned.saturating_sub(base).div_ceil(4).min(groups);
        cycles += groups - first;
    }
    cycles
}

// cache_fetch = JSR cache_peek, 104 instruction cycles and cache_advance.
// A packet's maximum body is less than 64 KiB, so at most one cursor carry
// occurs across the prefix publication and all body transfers. RECORD_CPU
// already reserves its five cycles; lookahead has its own carry allowance.
const FETCH_CPU: usize = 145;
// The occasional nine-cycle low-byte comparison is reserved in RECORD_CPU.
const CONSUME_CPU: usize = 58;

fn read_block_consumed(feed: usize, pointer: usize, length: usize) -> Cost {
    if feed == 0 {
        return Cost {
            cpu: 13 + FETCH_CPU,
            dma_bytes: length,
        };
    }
    let copied = feed.min(length);
    let tail = length - copied;
    let compare = if feed >> 8 < length >> 8 {
        11
    } else if feed >> 8 > length >> 8 {
        13
    } else if feed < length {
        23
    } else {
        22
    };
    let count_setup = if feed < length {
        24
    } else if length >> 8 != 0 {
        27
    } else {
        29
    };
    let mut cpu = 41 + compare + count_setup + 38;
    let mut remaining = copied;
    let mut source = pointer;
    while remaining > 0 {
        let count = remaining.min(255);
        cpu += 58 + copy_forward(count, source);
        source += count;
        remaining -= count;
    }
    cpu += 11 + 12 + 26 + 6;
    cpu += if tail == 0 { 11 } else { 10 + 36 + FETCH_CPU };
    Cost {
        cpu,
        dma_bytes: tail,
    }
}

fn column_header(feed: usize, pointer: usize) -> Cost {
    if feed >= 2 {
        Cost {
            cpu: 142
                + usize::from(feed < 256) * 8
                + usize::from(pointer & 255 >= 254) * 4
                + usize::from(pointer & 255 == 255)
                + usize::from(feed & 255 < 2) * 8,
            dma_bytes: 0,
        }
    } else {
        let mut cost = read_block_consumed(feed, pointer, 2);
        cost.cpu += 108;
        cost
    }
}

fn get_body(feed: usize, pointer: usize, length: usize) -> Cost {
    let compare = if feed >> 8 < length >> 8 {
        11
    } else if feed >> 8 > length >> 8 {
        13
    } else if feed < length {
        23
    } else {
        22
    };
    let setup = 6 + CONSUME_CPU + 3 + compare;
    if feed >= length {
        Cost {
            cpu: setup + 14 + 22 + 26 + 6,
            dma_bytes: 0,
        }
    } else {
        let mut cost = read_block_consumed(feed, pointer, length);
        cost.cpu += setup + 23;
        cost
    }
}

pub(super) fn column(
    primitive: &Primitive,
    escaped: usize,
    raw: bool,
    index: usize,
    body_at: usize,
    prefix: usize,
    _remaining: usize,
) -> Cost {
    let header_at = body_at - 2;
    let mut feed = prefix.saturating_sub(header_at);
    let mut pointer = PREFETCH_ADDRESS + header_at.min(prefix);
    let mut cost = column_header(feed, pointer);
    pointer += feed.min(2);
    feed = feed.saturating_sub(2);
    let body = if raw {
        let mut transfer = read_block_consumed(feed, pointer, 200);
        transfer.cpu += 6 + CONSUME_CPU + 3;
        // Header validation, RAW mode and completed-column loop. Assume an
        // existing previous column and the one-cycle-longer last-column path.
        transfer.cpu += 136 + usize::from(index < 8) * 2;
        transfer
    } else {
        let length = 50 + escaped;
        let mut transfer = get_body(feed, pointer, length);
        let source = if feed >= length { pointer } else { 0xdc00 };
        transfer.cpu += primitive.cycles(source);
        // Literal validation table starts at $1445; its indexed read crosses
        // a page at index187. All remaining mode/loop instructions are fixed.
        transfer.cpu += 184 + usize::from(index < 8) * 2 + usize::from(escaped >= 187);
        transfer
    };
    cost.cpu += body.cpu;
    cost.dma_bytes += body.dma_bytes;
    cost
}

pub(super) fn lut(prefix: usize) -> Cost {
    let mut cost = read_block_consumed(prefix, PREFETCH_ADDRESS, 1024);
    cost.cpu += 6 + CONSUME_CPU + 3;
    cost
}

pub(super) fn next_prepare(body: usize, switch_cache: bool) -> Cost {
    let cpu = if body < 2600 {
        let quotient = body / 13;
        // Eight fixed trial subtractions; each accepted bit adds 17 cycles.
        // Only the bit-1 rejection branch crosses a code page ($15fd->$160b).
        525 + 17 * quotient.count_ones() as usize
            + usize::from(quotient & 2 == 0)
            + usize::from(body >= 2560) * 10
    } else if body < 2816 {
        335
    } else {
        328
    };
    Cost {
        // One cache cursor carry; a switch also allows a block-index carry.
        cpu: cpu + 5 + usize::from(switch_cache) * 276,
        dma_bytes: if switch_cache { 8 } else { 4 },
    }
}

pub(super) fn max_next_prepare(switch_cache: bool) -> Cost {
    next_prepare(127 * 13, switch_cache)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_sources_match_the_instruction_calibration() {
        use sha2::{Digest, Sha256};
        // Changes in included sources can move branches and tables. Rebuild
        // the player and rerun the assembled timing oracle before updating
        // this fingerprint; blindly refreshing it invalidates the budget.
        let sources: [&[u8]; 4] = [
            include_bytes!("../../player/player_generic.asm"),
            include_bytes!("../../player/player_generic_pause.asm"),
            include_bytes!("../../player/player_generic_browser.asm"),
            include_bytes!("../../player/player_generic_metadata.asm"),
        ];
        let mut digest = Sha256::new();
        for source in sources {
            digest.update(source);
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "e6a09484cc64be9a98bd82fb15a5e63fc39553cfa5748441e2a404e76463e13f",
            "Player assembly changed: rerun assembled decoder timing calibration"
        );
    }

    #[test]
    fn primitive_instruction_counts_include_literal_validation() {
        assert_eq!(Primitive::new(&[0; 50], 0).cycles(0xdc00), 3083);
        assert_eq!(Primitive::new(&[254; 50], 0).cycles(0xdc00), 1539);
        let mut code = [0; 50];
        code[..10].fill(255);
        assert_eq!(Primitive::new(&code, 50).cycles(0xdc00), 3478);
        assert_eq!(Primitive::new(&code, 50).cycles(0xf5f8), 3520);
    }

    #[test]
    fn unrolled_ram_copy_preserves_real_source_alignment() {
        assert_eq!(copy_forward(39, 0xf100), 772);
        assert_eq!(copy_forward(39, 0xf1ff), 810);
        assert_eq!(copy_forward(40, 0xf100), 804);
        assert_eq!(copy_forward(200, 0xf180), 2716);
        assert_eq!(copy_forward(255, 0xf180), 3489);
    }

    #[test]
    fn partial_next_record_has_the_largest_preparation_cost() {
        let max = (0..65530)
            .map(|body| next_prepare(body, false).cpu)
            .max()
            .unwrap();
        assert_eq!(max, 649);
        assert_eq!(max_next_prepare(false).cpu, max);
        assert_eq!(next_prepare(2600, false).cpu, 340);
        assert_eq!(next_prepare(1024, true).dma_bytes, 8);
    }

    #[test]
    fn assembled_record_paths_are_bounded_without_large_per_column_padding() {
        // Independent production-assembly instruction execution, checked
        // against the vendored emulator. Tuples describe uniform columns;
        // measurements include buffered_blank_update and next preparation.
        // Order: reset, RAW, columns, escaped words, skipped words, next body,
        // cache switch, measured turbo slots (including REU DMA stalls).
        let cases = [
            (false, false, 0, 0, 0, 2587, false, 1079),
            (true, false, 0, 0, 0, 2587, true, 16811),
            (true, false, 1, 0, 0, 1024, false, 21152),
            (false, false, 1, 10, 0, 1024, false, 7591),
            (false, false, 30, 10, 0, 2600, false, 150882),
            (true, false, 60, 10, 0, 2587, true, 551953),
            (false, true, 1, 0, 0, 1024, false, 4968),
            (false, true, 30, 0, 0, 2587, true, 273740),
            (true, true, 30, 0, 0, 2600, false, 338155),
            (true, false, 80, 0, 40, 2587, false, 377432),
        ];
        for (reset, raw, count, escapes, skips, next, switch, measured) in cases {
            let escaped = escapes * 5;
            let length = if raw { 200 } else { 50 + escaped };
            let body = usize::from(reset) * 1024 + count * (2 + length);
            let prefix = body.min(2600) / 13 * 13;
            let mut code = [0; 50];
            code[..escapes].fill(255);
            code[escapes..escapes + skips].fill(254);
            let primitive = Primitive::new(&code, escaped);
            let mut slots = RECORD_CPU + next_prepare(next, switch).slots();
            let mut at = 0;
            if reset {
                slots += lut(prefix).slots();
                at += 1024;
            }
            for index in 0..count {
                slots += column(&primitive, escaped, raw, index, at + 2, prefix, body - at).slots();
                at += length + 2;
            }
            assert!(slots >= measured, "{slots} < {measured}");
            assert!(
                slots - measured <= count + 200,
                "excessive margin: {}",
                slots - measured
            );
        }
    }

    #[test]
    fn low_byte_consume_comparisons_need_at_most_nine_packet_allowances() {
        // A low-byte check needs remaining and requested high bytes equal.
        // Any comparison outside the final 512 bytes is impossible (the
        // largest column body is 300). Exhaust all legal tail lengths and
        // column sequences; a reset LUT comparison only occurs below 1280.
        let mut checks = [None; 513];
        checks[0] = Some(0usize);
        for remaining in 1..checks.len() {
            for body in (50..=300).step_by(5) {
                if remaining < body + 2 {
                    continue;
                }
                if let Some(tail) = checks[remaining - body - 2] {
                    let count = tail
                        + usize::from(remaining < 256)
                        + usize::from((remaining - 2) / 256 == body / 256);
                    checks[remaining] = Some(checks[remaining].unwrap_or(0).max(count));
                }
            }
        }
        assert_eq!(checks.iter().flatten().copied().max(), Some(9));
        assert_eq!(checks[470], Some(9));
        for (remaining, count) in checks.iter().enumerate() {
            if let Some(count) = count {
                assert!(count + usize::from(remaining < 256) <= 9);
            }
        }
    }
}
