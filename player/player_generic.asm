// SPDX-License-Identifier: GPL-3.0-only
// Isolated360x200 dictionary/SKIP/RAW PAL3 experiment; qualifiedRAW unchanged.
// UPIC1.3: Aleksi Eeben's indexed LDX / eighth-pair immediate LDX,
// 126 CPU slots per sixteen dots and the corrected two-NOP footer.
// STATIC_RENDERER selects the isolated renderer; otherwise UCI streams
// video and PCM through two REU cache slots. Front updates are progressive.
// Primary: xahmol/ultimate-uci-oscar64 include/ultimate_upic_lib.c (2026-10-09),
// authorized UPIC1.3 source port; default first raster25 and delay83.
// Pixel stream uses the primary1.3 first180 columns,22 short pairs and both footer NOPs.
// The separated prepare wrapper adds15 cycles; base delay80 cancels exactly15.
// UE2 PAL6569 calibration adds194 CPU cycles to that base before pixel0.
// This phase adjustment is derived from UE2 uniform63-slot PHI2 timing,
// not a claim that the unpublished physical Ultimate slot phase is identical.
// Standalone container9 player: all per-film metadata comes from its header.
.const WIDE_WIDTH=360
.const WIDE_HEIGHT=200
.const WIDE_FIRST_RASTER=53
.const ACTIVE_WIDTH=356
.const ACTIVE_HEIGHT=200
.const ACTIVE_TOP=0
.const SPAN_VERSION=9
.const DICT_WORDS=(ACTIVE_HEIGHT+3)>>2
.const DICT_ROWS=DICT_WORDS*4
.const DICT_MAX_LITERAL_BYTES=DICT_WORDS*5
.const FRAME_BYTES=46080
.const PREFETCH_BYTES_PER_ROW=13
.const PREFETCH_HEAD_BYTES_PER_ROW=(SPAN_VERSION==9 ? 11 : 9)
.const PREFETCH_TAIL_BYTES_PER_ROW=(SPAN_VERSION==9 ? 2 : 4)
.const STATIC_RENDERER=0
.const TEST_FRONT_SLOT=0
.const TEST_TOGGLE_FRONT=0
.const UCI_DELAY_TICKS=0
.const AUDIO_VOICE_VOLUME=63
.const AUDIO_BLOCK_OFFSET=225280
.const STREAM_BLOCK_BYTES=262144
.const CACHE_SLOT_BANKS=4
.const CACHE_CHUNK_BYTES=1024
.const VIDEO_RECORD_COUNT=1
.const AUDIO_RATE_DIVIDER=0
.const HEADER=$0500
.const SELECTED_PATH=$0600
.const BROWSER_OVERLAY=$1800
.const BROWSER_BYTES=$2800
.const BROWSER_REU=$080000
.const AUDIO_TABLE_REU=$020000
.label active_stream_blocks=$04b9
.label cache_read_block=$04bb
.label active_audio_length=$04f0
.label player_mode=$04f3
.label generic_error=$04f4
.label bootstrap_length=$04f5
.label bootstrap_reu=$04f8
.label bootstrap_offset=$04fb
.label browser_count=$0700
.label browser_selected=$0701
.label browser_top=$0702
.label browser_key=$0703
.label browser_response_size=$0704
.label browser_status_size=$0706
.label browser_reply_state=$0707
.label browser_attributes=$0708
.label browser_entry_ptr=$86
.label browser_text_ptr=$88
.label browser_screen_ptr=$8a
.label browser_list_ptr=$8c
.label browser_path_length=$0709
.label browser_status_code=$070a
.label browser_command=$070c
.label browser_temp=$070d
.label browser_limit=$070e
.label playback_paused=$070f
.label pause_request=$0710 // 1=pause, 2=resume, applied at the next PAL setup IRQ
.label pause_count=$0711 // u16
.label resume_count=$0713 // u16
.label audio_tick_count=$0715 // elapsed PAL ticks in this PCM block, including before pauses
.label audio_resume_offset=$0716 // u16, stereo-frame aligned byte offset
.label audio_resume_active=$0718
.label audio_last_resume_offset=$0719 // u16, diagnostics
.label audio_calc_samples=$071b // u16
.label audio_calc_sum=$071d // u24 numerator, then quotient
.label audio_calc_remainder=$0720
.label audio_calc_ticks=$0721
.label audio_calc_last_block=$0722 // u16
.label browser_metadata_index=$0724
.label browser_number=$0725 // u32, browser-only arithmetic workspace
.label browser_factor=$0729 // u32
.label browser_multiplier=$072d // u16
.label browser_digits=$072f
.label browser_leading=$0730
.const DIRECTORY_METADATA=$3a00 // 96 cached, formatted 16-byte rows; inside overlay
.const DIRECTORY_ENTRIES=$4400
.const DIRECTORY_CAPACITY=96
.const NYBBLES = $0200
.const KERNEL_BASE = $e800
.const COLUMNS = WIDE_WIDTH/2
.const SHORT_PAIRS = floor(COLUMNS/8)
.const PIXEL_KERNEL_BYTES = COLUMNS*12-SHORT_PAIRS+8
.const PREP_BASE = KERNEL_BASE+PIXEL_KERNEL_BYTES
.const UPIC13_PRIMARY_DELAY = 83
.const WRAPPER_EXTRA_CYCLES = 15
.const UE2_PAL_PHASE_ADJUSTMENT_CYCLES = 194
.const WIDE_WRAPPER_DELAY = 118
.label phase_adjustment_cpu_cycles = UE2_PAL_PHASE_ADJUSTMENT_CYCLES
.const PRE_PIXEL_BUSY_CYCLES = (SPAN_VERSION==9 ? 715 : 608)
.const PREP_CALL_CYCLES = (SPAN_VERSION==9 ? 0 : 12)
.const FIRST_D020_RENDER_CYCLE = SHORT_PAIRS*8+PREP_CALL_CYCLES+PRE_PIXEL_BUSY_CYCLES+6+8
.assert "360 firstpixel phase", FIRST_D020_RENDER_CYCLE==(SPAN_VERSION==9 ? 905 : 810), true
.assert "UE2 PAL phase compensation changed", 5*(WIDE_WRAPPER_DELAY-UPIC13_PRIMARY_DELAY)+WRAPPER_EXTRA_CYCLES+4==UE2_PAL_PHASE_ADJUSTMENT_CYCLES, true
.const FRAME_IRQ_RASTER = WIDE_FIRST_RASTER+ACTIVE_TOP-2
.const TOP_SYNC_RASTER = WIDE_FIRST_RASTER+ACTIVE_TOP-1
.const CACHE_BANK = $10
.const CHUNK_BUFFER = $dc00
.const PREFETCH_BUFFER = (ACTIVE_HEIGHT>196 ? $f180 : $f200)
.const PREFETCH_CAPACITY = PREFETCH_BYTES_PER_ROW*ACTIVE_HEIGHT
.assert "360 split prefetch must preserve13 bytes per row", PREFETCH_HEAD_BYTES_PER_ROW+PREFETCH_TAIL_BYTES_PER_ROW==13 && PREFETCH_BYTES_PER_ROW==13, true
.assert "360 split prefix overlaps IRQ", PREFETCH_BUFFER+PREFETCH_CAPACITY<=$fc00, true
.label lz_feed_pointer = $f2
.label lz_destination_pointer = $f4
.label lz_source_pointer = $f6
.label lz_output_logical = $046e
.label lz_feed_available = $04c0
.label lz_token = $04c2
.label lz_run_length = $04c3
.label lz_copy_count = $04c4
.label lz_match_offset = $04c5
.label lz_source_logical = $04c7
.label lz_new_output = $04c9
.label lz_literal_left = $04cb
.label lz_prefetch_count = $04cc
.label lz_prefetch_rows = $04ce
.label lz_prefetch_end_row = $04cf
.label lz_record_prepared = $04d0
.label lz_reader_value = $04d2
.label lz_copy_aligned = $04d3
.label raw_output_column = lz_output_logical
.label raw_columns_remaining = $04d4
.label raw_prefix_columns = $04d5
.label raw_header = $0430
.label raw_column_count = $04db
.label dictionary_load_count = $0464
.label dict_decoded_column_count = $0466
.label raw_decoded_column_count = $0479
.label dict_column_total = $04d4
.label dict_columns_left = $04d5
.label dict_previous_column = $04d6
.label dict_has_previous = $04d7
.label dict_col_header = $04e0
.label dict_body_pointer = $04e4
.label dict_copy_left = $04ea
.label dict_prefix_part = $04ec
.label dict_tail_length = $04ee
.label dict_indices = $80
.label dict_literals = $82
.label dict_dest_page = $84
.label dict_literal_bytes = $85
.label source_pointer = $f8
.label video_frame_count = $0400
.label upic_missed_frames = $0402
.label upic_line_late_count = $0404
.label scanout_row = $0406
.label fatal_error = $0417
.label record_index = $0408
.label cache_underrun = $040a
.label underrun_count = cache_underrun
.label refill_count = $040c
.label update_raster_start = $0410
.label update_raster_end = $0411
.label update_raster_start_high = $0412
.label update_raster_end_high = $0413
.label max_packet_length = $0414
.label playback_ended = $0416
.label uci_error_count = $0418
.label uci_status_chars = $0419
.label deadline_miss_count = $041b
.label blank_updates = $041d
.label uci_busy = $0420
.label uci_fill_bank = $0421
.label cache_ready = $0422
.label cache_need_refill = $0424
.label cache_read_bank = $0425
.label cache_cursor = $0426
.label next_file_block = $0428
.label uci_response_count = $042a
.label uci_status_count = $042b
.label uci_status_bad = $042c
.label uci_wait = $042d
.label record_header = $0430
.label packet_length = $0432
.label audio_pending_start = $0435
.label uci_last_response = $0436
.label uci_refill_start = $043a
.label uci_refill_max_ticks = $043c
.label uci_elapsed_ticks = $043e
.label uci_command_kind = $0440
.label pending_refill_mask = $0441
.label uci_pending_publish = $0442
.label uci_completion_frame = $0443
.label record_update_active = $0445
.label uci_response_prefix = $0450
.label framebuffer_front_slot = $0468
.label compact_row = $0469
.label source_base_low = $046a
.label nmi_count = $046b
.label cache_cursor_high = $046c
.label decoded_chunk_count = $0460
.label framebuffer_swap_count = $0462
.label record_flags = $0470
.label record_remaining = $0471
.label chunk_length = $0473
.label dma_destination = $0475
.label dma_length = $0477
.label decode_remaining = $0479
.label max_chunk_length = $047b
.label span_last = $047d
.label span_source = $f8
.label span_count = $fa
.label span_tag = $fb
.label span_advance = $fc
.label span_destination = $fe

// Persistent state: only0400..047F is reset when starting another pass.
.label current_preset = $0480
.label requested_preset = $0481
.label restart_pending = $0482 // 1=keyboard, 2=completed EOF
.label keyboard_last_key = $0483
.label restart_in_progress = $0484
.label playback_generation = $0485 // u16, first successful pass is1
.label loop_count = $0487 // u16, completed passes restarted at EOF
.label preset_switch_count = $0489 // u16, successful changes of preset
.label restart_count = $048b // u16, successful restarts after cold startup
.label restart_kind = $048d
.label last_completed_preset = $048e
.label eof_audio_stopped = $048f
.label lifetime_missed_frames = $0490
.label lifetime_line_late_count = $0492
.label lifetime_deadline_miss_count = $0494
.label lifetime_underrun_count = $0496
.label lifetime_uci_error_count = $0498
.label fatal_latched = $049a
.label last_completed_record_count = $049c
.label last_completed_generation = $049e
.label last_completed_frame_count = $04a0
.label last_completed_swap_count = $04a2
.label last_completed_chunk_count = $04a4
.label last_completed_refill_count = $04a6
.label last_completed_eof_audio_stopped = $04a8
.label file_open = $04aa
.label keyboard_scan_scratch = $04ab
.label previous_preset = $04ac
.label uci_close_count = $04ad
.label uci_open_count = $04af
.label restart_drain_count = $04b1
.label last_drained_command_kind = $04b3
.label active_record_count = $04b4
.label active_last_audio_length = $04b6 // u24, final PCM block's byte count

.assert "Isolated canvas must be360x200", WIDE_WIDTH==360 && WIDE_HEIGHT==200, true
.assert "Isolated bitmap must contain180 full-page columns", FRAME_BYTES==COLUMNS*256 && COLUMNS==180, true
.assert "Wide active rows exceed canvas", ACTIVE_TOP>=0 && ACTIVE_HEIGHT>0 && ACTIVE_TOP+ACTIVE_HEIGHT<=WIDE_HEIGHT, true
.assert "Wide raster range exceeds PAL", WIDE_FIRST_RASTER>=2 && WIDE_FIRST_RASTER+ACTIVE_TOP+ACTIVE_HEIGHT<=312, true
.assert "Setup IRQ must precede raster256", FRAME_IRQ_RASTER<256, true
.assert "Single wide front must be selected", TEST_FRONT_SLOT==0 && TEST_TOGGLE_FRONT==0, true
.function wide_address(column) {
    .if (column<8) { .return $e000+column*256 }
    .return $1800+(column-8)*256
}
.function kernel_opcode(column) {
    .return KERNEL_BASE+column*12-(column>>3)
}

* = $0801 "MUpic standalone browser SYS loader"
:BasicUpstart2(start)
start:
    sei
    ldx #$ff
    txs
    lda #$7f
    sta $dc0d
    sta $dd0d
    lda $dc0d
    lda $dd0d
    lda #0
    sta $d01a
    sta $dc0e
    sta $dc0f
    // Stock Ultimate firmware holds turbo for 2.06 seconds after reset.
    // This wait precedes playback and matches the proven legacy startup.
    ldx #110
!wrap:
    bit $d011
    bpl !wrap-
!low:
    bit $d011
    bmi !low-
    dex
    bne !wrap-
    lda #$35
    sta $01
    lda #$8f
    sta $d031
    lda #0
    sta $d011
    sta $d015
    sta $d020
    sta $d021
    sta $df20
    sta $df40
    sta $df21
    sta $df41
    ldx #0
!state:
    sta $0400,x
    sta $0500,x
    sta $0600,x
    sta $0700,x
    inx
    bne !state-
    lda #1
    sta current_preset
    sta requested_preset
    lda #$ff
    sta $dc02
    sta $dc00
    lda #0
    sta $dc03
    ldx #0
!nybble:
    txa
    lsr
    lsr
    lsr
    lsr
    sta NYBBLES,x
    inx
    bne !nybble-
    jsr browser_save_overlay
    jmp generic_browser
generic_begin_scanout:
    lda #<nmi_return
    sta $fffa
    lda #>nmi_return
    sta $fffb
    lda #<frame_irq
    sta $fffe
    lda #>frame_irq
    sta $ffff
    lda #FRAME_IRQ_RASTER
    sta $d012
    lda #1
    sta $d019
    sta $d01a
    cli
main:
    .if (STATIC_RENDERER!=0) {
        jmp main
    } else {
        jsr keyboard_scan
        jsr uci_poll
        lda fatal_error
        beq !healthy+
        jmp comparison_halt
!healthy:
        lda restart_pending
        bne !restart+
        jmp !continue+
!restart:
        jsr restart_playback
        jmp main
!continue:
        lda playback_ended
        beq !streaming+
        // Let one final IRQ display the completed image and mute the sampler.
        lda eof_audio_stopped
        beq main
        lda #2
        sta restart_pending
        jmp main
!streaming:
        lda playback_paused
        bne main
        lda uci_busy
        bne main
        lda cache_need_refill
        beq main
        lda next_file_block+1
        cmp active_stream_blocks+1
        bcc !have_block+
        bne main
        lda next_file_block
        cmp active_stream_blocks
        bcs main
!have_block:
        ldx #0
        lda cache_need_refill
        and #1
        bne !refill+
        inx
!refill:
        stx uci_fill_bank
        jsr uci_start_refill
        jmp main
    }

load_initial:
    lda #0
    sta lz_record_prepared
    sta lz_prefetch_count
    sta lz_prefetch_count+1
    sta lz_feed_available
    sta lz_feed_available+1
    sta lz_output_logical
    sta lz_output_logical+1
    sta dict_has_previous
    lda #ACTIVE_TOP
    sta lz_prefetch_end_row
    lda #0
    sta $df04
    sta $df05
    sta $df06
    // Compact file order is logical column0..179: first8 high pages,
    // followed by172 pages at1800. Both DMA spans auto-advance REU source.
    .for (var part=0;part<2;part++) {
        .var dest=List().add($e000,$1800).get(part)
        .var size=List().add(2048,FRAME_BYTES-2048).get(part)
        lda #0
        sta $df09
        sta $df0a
        lda #<dest
        sta $df02
        lda #>dest
        sta $df03
        lda #<size
        sta $df07
        lda #>size
        sta $df08
        lda #$91
        sta $df01
    }
    rts

// SPACE toggles play/pause once per fresh press. RUN/STOP returns to browser.
// Sampler offset arithmetic runs in main while the paused picture is scanned.
keyboard_scan:
    lda #$7f
    sta $dc00
    lda $dc01
    sta keyboard_scan_scratch
    and #$80 // RUN/STOP: row7, column7
    bne !check_space+
    ldx #3
    jmp !selected+
!check_space:
    lda keyboard_scan_scratch
    and #$10 // SPACE: row7, column4
    beq !space+
    ldx #0
    jmp !selected+
!space:
    ldx #1
!selected:
    lda #$ff
    sta $dc00
    txa
    cmp keyboard_last_key
    beq !done+
    stx keyboard_last_key
    cpx #0
    beq !done+
    cpx #3
    bne !toggle_space+
    lda #3
    sta restart_pending
    rts
!toggle_space:
    lda playback_ended
    bne !done+
    lda pause_request
    beq !toggle+
    lda #0 // A second press before the IRQ cancels the pending toggle.
    sta pause_request
    rts
!toggle:
    lda playback_paused
    bne !resume+
    lda #1
    sta pause_request
    rts
!resume:
    jsr prepare_audio_resume
    lda #2
    sta pause_request
!done:
    rts

restart_playback:
    // Scanout and all CPU REU commands stop before waiting for the pending
    // asynchronous DOS command. LOAD_REU must finish and be acknowledged
    // before CLOSE/OPEN or a destination is reused.
    sei
    lda #1
    sta restart_in_progress
    lda restart_pending
    sta restart_kind
    lda current_preset
    sta previous_preset
    lda #0
    sta $d01a
    sta $dc0e
    sta $dc0f
    sta $df01
    lda #1
    sta $d019
    jsr audio_stop
    lda uci_busy
    beq !drain+
    lda uci_command_kind
    sta last_drained_command_kind
    inc restart_drain_count
    bne !drain+
    inc restart_drain_count+1
!drain:
    jsr uci_wait_complete
    lda fatal_error
    beq !drained+
    jmp comparison_halt
!drained:
    jsr accumulate_pass_errors
    lda restart_kind
    cmp #2
    bne !return+
    // Completion is captured before the per-pass counters are reset.
    lda current_preset
    sta last_completed_preset
    .for (var field=0;field<6;field++) {
        .var from=List().add(record_index,playback_generation,video_frame_count,framebuffer_swap_count,decoded_chunk_count,refill_count).get(field)
        .var to=List().add(last_completed_record_count,last_completed_generation,last_completed_frame_count,last_completed_swap_count,last_completed_chunk_count,last_completed_refill_count).get(field)
        lda from
        sta to
        lda from+1
        sta to+1
    }
    lda eof_audio_stopped
    sta last_completed_eof_audio_stopped
!return:
    jsr uci_close_file
    jmp generic_browser

accumulate_pass_errors:
    .for (var field=0;field<4;field++) {
        .var from=List().add(upic_missed_frames,upic_line_late_count,deadline_miss_count,underrun_count).get(field)
        .var to=List().add(lifetime_missed_frames,lifetime_line_late_count,lifetime_deadline_miss_count,lifetime_underrun_count).get(field)
        clc
        lda to
        adc from
        sta to
        lda to+1
        adc from+1
        sta to+1
    }
    clc
    lda lifetime_uci_error_count
    adc uci_error_count
    sta lifetime_uci_error_count
    bcc !done+
    inc lifetime_uci_error_count+1
!done:
    rts

uci_close_file:
    // CLOSE has no LOAD_REU response prefix; validate its ASCII status only.
    lda #0
    sta uci_command_kind
    lda #2
    sta $df1d
    lda #3
    sta $df1d
    jsr uci_push
    jsr uci_wait_complete
    lda fatal_error
    bne !done+
    lda #0
    sta file_open
    inc uci_close_count
    bne !done+
    inc uci_close_count+1
!done:
    rts

comparison_halt:
    sei
    lda fatal_error
    sta fatal_latched
    lda #0
    sta $d01a
    sta $dc0e
    sta $dc0f
    jsr audio_stop
    lda #2
    sta $d020
!halt:
    jmp !halt-

stream_startup:
    ldx #56
    jsr generic_seek_header_offset
    lda fatal_error
    bne stream_startup_halt
    lda #0
    sta uci_fill_bank
    jsr uci_start_refill
    jsr uci_wait_complete
    lda fatal_error
    bne stream_startup_halt
    lda active_stream_blocks+1
    bne !second+
    lda active_stream_blocks
    cmp #2
    bcc !single+
!second:
    lda #1
    sta uci_fill_bank
    jsr uci_start_refill
    jsr uci_wait_complete
    lda fatal_error
    bne stream_startup_halt
!single:
    lda #1
    sta audio_pending_start
    lda #$34
    sta $01
    jsr lz_prepare_next
    lda #$35
    sta $01
    rts
stream_startup_error:
    sta fatal_error
stream_startup_halt:
    jmp comparison_halt
uci_push:
    lda #1
    sta uci_busy
    sta $df1c
    lda #0
    sta uci_response_count
    sta uci_status_count
    sta uci_status_bad
    sta uci_wait
    sta uci_wait+1
    sta uci_wait+2
    sta uci_pending_publish
    rts
uci_start_refill:
    lda #1
    sta uci_command_kind
    lda video_frame_count
    sta uci_refill_start
    lda video_frame_count+1
    sta uci_refill_start+1
    ldx uci_fill_bank
    lda #0
    sta cache_ready,x
    lda cache_clear_mask,x
    and cache_need_refill
    sta cache_need_refill
    lda #2
    sta $df1d
    lda #$21 // DOS LOAD_REU
    sta $df1d
    lda #0
    sta $df1d
    sta $df1d
    txa
    .for (var banks=CACHE_SLOT_BANKS;banks>1;banks=banks/2) { asl }
    clc
    adc #CACHE_BANK
    sta $df1d
    lda #0
    sta $df1d
    .if (STREAM_BLOCK_BYTES==65536) {
        // Preserve the original64KiB command instruction sequence.
        sta $df1d
        sta $df1d
        lda #1
        sta $df1d
        lda #0
        sta $df1d
    } else {
        .for (var byte=0;byte<4;byte++) {
            lda #((STREAM_BLOCK_BYTES>>(byte*8))&255)
            sta $df1d
        }
    }
    inc next_file_block
    bne !pushed+
    inc next_file_block+1
!pushed:
    jmp uci_push
uci_wait_complete:
    jsr uci_poll
    lda uci_busy
    beq !done+
    inc uci_wait
    bne uci_wait_complete
    inc uci_wait+1
    bne uci_wait_complete
    inc uci_wait+2
    bne uci_wait_complete
    lda #4
    sta $df1c
    lda #2
    sta fatal_error
!done:
    rts
uci_poll:
    lda uci_busy
    bne !busy+
    rts
!busy:
    lda uci_pending_publish
    beq !protocol+
    sec
    lda video_frame_count
    sbc uci_completion_frame
    sta uci_elapsed_ticks
    lda video_frame_count+1
    sbc uci_completion_frame+1
    bne !delayed_done+
    lda uci_elapsed_ticks
    cmp #UCI_DELAY_TICKS
    bcs !delayed_done+
    rts
!delayed_done:
    jmp uci_publish_cache
!protocol:
    lda $df1c
    and #$30
    cmp #$20
    beq !drain+
    cmp #$30
    beq !drain+
    rts
!drain:
    lda $df1c
    bpl !status+
    lda $df1e
    ldx uci_response_count
    cpx #7
    bcs !prefix+
    sta uci_response_prefix,x
!prefix:
    cpx #4
    bcs !count+
    sta uci_last_response,x
!count:
    inc uci_response_count
    jmp !drain-
!status:
    lda $df1c
    and #$40
    beq !ack+
    lda $df1f
    ldx uci_status_count
    cpx #2
    bcs !status-
    sta uci_status_chars,x
    inc uci_status_count
    cmp #48
    beq !status-
    inc uci_status_bad
    jmp !status-
!ack:
    lda $df1c
    and #$30
    pha
    lda #2
    sta $df1c
    pla
    cmp #$30
    bne !last_reply+
    rts
!last_reply:
    lda #0
    sta uci_busy
    lda uci_status_count
    cmp #2
    beq !two_status+
    jmp uci_bad_reply
!two_status:
    lda uci_status_bad
    beq !valid_status+
    jmp uci_bad_reply
!valid_status:
    // This firmware returns an ASCII LOAD_REU summary in the response channel.
    // A successful load becomes available after the complete reply is accepted.
    lda uci_command_kind
    bne !load_response+
    rts
!load_response:
    lda uci_response_count
    cmp #7
    bcs !prefix_valid+
    jmp uci_bad_reply
!prefix_valid:
    ldx #6
!full_read:
    lda uci_response_prefix,x
    cmp uci_full_read_prefix,x
    bne !short+
    dex
    bpl !full_read-
    .if (UCI_DELAY_TICKS>0) {
        // A reproducible cache-stall experiment, after the real command
        // completed. Startup priming is exempt because scanout is not running.
        lda video_frame_count
        ora video_frame_count+1
        beq uci_publish_cache
        lda video_frame_count
        sta uci_completion_frame
        lda video_frame_count+1
        sta uci_completion_frame+1
        lda #1
        sta uci_pending_publish
        sta uci_busy
        rts
    }
uci_publish_cache:
    lda #0
    sta uci_busy
    sta uci_pending_publish
    ldx uci_fill_bank
    lda #1
    sta cache_ready,x
    sec
    lda video_frame_count
    sbc uci_refill_start
    sta uci_elapsed_ticks
    lda video_frame_count+1
    sbc uci_refill_start+1
    sta uci_elapsed_ticks+1
    cmp uci_refill_max_ticks+1
    bcc !refill_count+
    bne !refill_time+
    lda uci_elapsed_ticks
    cmp uci_refill_max_ticks
    bcc !refill_count+
!refill_time:
    lda uci_elapsed_ticks
    sta uci_refill_max_ticks
    lda uci_elapsed_ticks+1
    sta uci_refill_max_ticks+1
!refill_count:
    inc refill_count
    bne !done+
    inc refill_count+1
!done:
    rts
uci_bad_reply:
    inc uci_error_count
    lda #3
    sta fatal_error
    rts
!short:
    inc uci_error_count
    lda #6
    sta fatal_error
    rts


cache_clear_mask: .byte $fe,$fd
cache_bit: .byte 1,2
uci_full_read_prefix:
    .byte 36
    .for (var nibble=5;nibble>=0;nibble--) {
        .var digit=(STREAM_BLOCK_BYTES>>(nibble*4))&15
        .byte (digit<10 ? 48+digit : 65+digit-10)
    }


lz_configure_prefetch:
    lda #ACTIVE_TOP
    sta lz_prefetch_row_compare+1
    sta raw_post_row_compare+1
    lda playback_paused
    bne !done+
    lda lz_record_prepared
    beq !done+
    ldx cache_read_bank
    lda cache_ready,x
    beq !done+
    lda lz_prefetch_end_row
    sta lz_prefetch_row_compare+1
    sta raw_post_row_compare+1
    lda #0
    sta $df09
    sta $df0a
    sta $df08
    lda cache_cursor
    sta $df04
    lda cache_cursor+1
    sta $df05
    lda cache_read_bank
    .for (var banks=CACHE_SLOT_BANKS;banks>1;banks=banks/2) { asl }
    clc
    adc #CACHE_BANK
    adc cache_cursor_high
    sta $df06
    lda #<PREFETCH_BUFFER
    sta $df02
    lda #>PREFETCH_BUFFER
    sta $df03
!done:
    rts

.label resident_core_end = *
.assert "Dictionary resident core overlaps parser", * <= $1000, true
* = $1000 "Dictionary body feeder and blank parser"
lz_prepare_next:
    lda #0
    sta lz_record_prepared
    sta lz_prefetch_count
    sta lz_prefetch_count+1
    sta lz_prefetch_rows
    lda #ACTIVE_TOP
    sta lz_prefetch_end_row
!ready:
    ldx cache_read_bank
    lda cache_ready,x
    bne !header+
    rts
!header:
    lda #<raw_header
    sta dma_destination
    lda #>raw_header
    sta dma_destination+1
    lda #4
    sta dma_length
    lda #0
    sta dma_length+1
    jsr cache_fetch
    lda raw_header
    ora raw_header+1
    bne !record+
    jsr switch_cache_bank
    jmp !ready-
!record:
    lda raw_header+2
    sta record_flags
    and #$fc
    beq !flags+
    jmp record_malformed
!flags:
    lda raw_header+3
    sta dict_column_total
    cmp #(COLUMNS+1)
    bcc !count+
    jmp record_malformed
!count:
    sec
    lda raw_header
    sta packet_length
    sbc #2
    sta record_remaining
    lda raw_header+1
    sta packet_length+1
    sbc #0
    sta record_remaining+1
    bcs !maximum+
    jmp record_malformed
!maximum:
    lda packet_length+1
    cmp max_packet_length+1
    bcc !prefix+
    bne !save+
    lda packet_length
    cmp max_packet_length
    bcc !prefix+
!save:
    lda packet_length
    sta max_packet_length
    lda packet_length+1
    sta max_packet_length+1
!prefix:
    lda record_remaining+1
    cmp #>PREFETCH_CAPACITY
    bcc !partial+
    bne !full+
    lda record_remaining
    cmp #<PREFETCH_CAPACITY
    bcc !partial+
!full:
    lda #<PREFETCH_CAPACITY
    sta lz_prefetch_count
    lda #>PREFETCH_CAPACITY
    sta lz_prefetch_count+1
    lda #ACTIVE_HEIGHT
    sta lz_prefetch_rows
    bne !prepared+
!partial:
    // Small quiet records have ample slack; repeated subtraction avoids
    // a512-byte division table. Busy records take the constant full path.
    lda record_remaining
    sta lz_prefetch_count
    lda record_remaining+1
    sta lz_prefetch_count+1
!divide:
    lda lz_prefetch_count+1
    bne !subtract+
    lda lz_prefetch_count
    cmp #PREFETCH_BYTES_PER_ROW
    bcc !remainder+
!subtract:
    sec
    lda lz_prefetch_count
    sbc #PREFETCH_BYTES_PER_ROW
    sta lz_prefetch_count
    lda lz_prefetch_count+1
    sbc #0
    sta lz_prefetch_count+1
    inc lz_prefetch_rows
    jmp !divide-
!remainder:
    sec
    lda record_remaining
    sbc lz_prefetch_count
    sta lz_prefetch_count
    lda record_remaining+1
    sbc lz_prefetch_count+1
    sta lz_prefetch_count+1
!prepared:
    clc
    lda lz_prefetch_rows
    adc #ACTIVE_TOP
    sta lz_prefetch_end_row
    lda #1
    sta lz_record_prepared
    rts

apply_buffered_record:
    lda playback_ended
    ora fatal_error
    beq !active+
    rts
!active:
    lda lz_record_prepared
    bne !record+
    inc cache_underrun
    bne !retry+
    inc cache_underrun+1
!retry:
    jsr lz_prepare_next
    rts
!record:
    lda #1
    sta record_update_active
    lda #<PREFETCH_BUFFER
    sta lz_feed_pointer
    lda #>PREFETCH_BUFFER
    sta lz_feed_pointer+1
    lda lz_prefetch_count
    sta lz_feed_available
    sta dma_length
    lda lz_prefetch_count+1
    sta lz_feed_available+1
    sta dma_length+1
    // Visible DMA already consumed the prefix. Publish its cursor once.
    jsr cache_advance
    lda record_flags
    and #2
    beq !columns+
    lda #0
    sta raw_output_column
    sta raw_output_column+1
    sta dict_has_previous
    lda #<$c600
    sta dma_destination
    lda #>$c600
    sta dma_destination+1
    lda #0
    sta dma_length
    lda #4
    sta dma_length+1
    jsr dict_read_block
    lda fatal_error
    beq !loaded+
    rts
!loaded:
    inc dictionary_load_count
    bne !columns+
    inc dictionary_load_count+1
!columns:
    clc
    lda raw_output_column
    adc dict_column_total
    bcs !bad_count+
    cmp #(COLUMNS+1)
    bcc !count_ok+
!bad_count:
    jmp record_malformed
!count_ok:
    lda dict_column_total
    sta dict_columns_left
    bne dict_next_column
    jmp record_complete

dict_next_column:
    jsr dict_read_column_header
    lda fatal_error
    beq !header+
    rts
!header:
    lda dict_col_header
    cmp #COLUMNS
    bcc !range+
    jmp record_malformed
!range:
    lda dict_has_previous
    beq !ordered+
    lda dict_col_header
    cmp dict_previous_column
    bcc !bad_order+
    beq !bad_order+
    bne !ordered+
!bad_order:
    jmp record_malformed
!ordered:
    lda dict_col_header
    sta dict_previous_column
    cmp #8
    bcs !low_page+
    clc
    adc #$e0
    bne !page+
!low_page:
    clc
    adc #$10
!page:
    sta dict_dest_page
    lda #1
    sta dict_has_previous
    lda dict_col_header+1
    cmp #255
    bne dict_mode
    // RAW goes straight from suffix REU to front. Only prefix bytes need
    // aRAM copy; neither LUT/stage nor an intermediate RAM buffer is used.
    lda #ACTIVE_TOP
    sta dma_destination
    lda dict_dest_page
    sta dma_destination+1
    lda #DICT_ROWS
    sta dma_length
    lda #0
    sta dma_length+1
    jsr dict_read_block
    lda fatal_error
    beq !raw_count+
    rts
!raw_count:
    inc raw_decoded_column_count
    bne dict_column_complete
    inc raw_decoded_column_count+1
    jmp dict_column_complete

dict_mode:
    sta dict_literal_bytes
    cmp #(DICT_MAX_LITERAL_BYTES+1)
    bcc !literal_range+
    jmp record_malformed
!literal_range:
    tax
    lda dict_literal_length_valid,x
    beq !length+
    jmp record_malformed
!length:
    clc
    lda dict_literal_bytes
    adc #DICT_WORDS
    sta dma_length
    lda #0
    adc #0
    sta dma_length+1
    jsr dict_get_body
    lda fatal_error
    beq !decode+
    rts
!decode:
    lda dict_body_pointer
    sta dict_indices
    clc
    adc #DICT_WORDS
    sta dict_literals
    lda dict_body_pointer+1
    sta dict_indices+1
    adc #0
    sta dict_literals+1
    jsr dict_decode_column
    lda fatal_error
    beq !decoded+
    rts
!decoded:
    inc dict_decoded_column_count
    bne dict_column_complete
    inc dict_decoded_column_count+1

dict_column_complete:
    inc raw_output_column
    inc decoded_chunk_count
    bne !counted+
    inc decoded_chunk_count+1
!counted:
    dec dict_columns_left
    beq !done+
    jmp dict_next_column
!done:
    jmp record_complete

// Two headerbytes are frequent; avoid general RAMcopy/SMC machinery.
dict_read_column_header:
    lda #2
    sta dma_length
    lda #0
    sta dma_length+1
    jsr dict_consume_length
    bcc !valid+
    rts
!valid:
    lda lz_feed_available+1
    bne !prefix+
    lda lz_feed_available
    cmp #2
    bcs !prefix+
    lda #<dict_col_header
    sta dma_destination
    lda #>dict_col_header
    sta dma_destination+1
    // Zero available uses directDMA; one remaining byte uses crossingcopy.
    jmp dict_read_block_consumed
!prefix:
    ldy #0
    lda (lz_feed_pointer),y
    sta dict_col_header
    iny
    lda (lz_feed_pointer),y
    sta dict_col_header+1
    clc
    lda lz_feed_pointer
    adc #2
    sta lz_feed_pointer
    bcc !pointer+
    inc lz_feed_pointer+1
!pointer:
    sec
    lda lz_feed_available
    sbc #2
    sta lz_feed_available
    bcs !done+
    dec lz_feed_available+1
!done:
    rts

// Carryset signals malformed length. The whole requested body is consumed
// here once; prefix/suffix paths subsequently move bytes without recounting.
dict_consume_length:
    lda dma_length
    ora dma_length+1
    bne !nonzero+
    jmp record_malformed
!nonzero:
    lda record_remaining+1
    cmp dma_length+1
    bcc !bad+
    bne !consume+
    lda record_remaining
    cmp dma_length
    bcc !bad+
!consume:
    sec
    lda record_remaining
    sbc dma_length
    sta record_remaining
    lda record_remaining+1
    sbc dma_length+1
    sta record_remaining+1
    clc
    rts
!bad:
    jmp record_malformed

// Return acontiguous encoded column, zerocopy when wholly in theprefix.
// Only the single straddlingcolumn is copied; suffix bodies DMA toDC00.
dict_get_body:
    jsr dict_consume_length
    bcc !valid+
    rts
!valid:
    lda lz_feed_available+1
    cmp dma_length+1
    bcc !stage+
    bne !direct+
    lda lz_feed_available
    cmp dma_length
    bcc !stage+
!direct:
    lda lz_feed_pointer
    sta dict_body_pointer
    lda lz_feed_pointer+1
    sta dict_body_pointer+1
    clc
    lda lz_feed_pointer
    adc dma_length
    sta lz_feed_pointer
    lda lz_feed_pointer+1
    adc dma_length+1
    sta lz_feed_pointer+1
    sec
    lda lz_feed_available
    sbc dma_length
    sta lz_feed_available
    lda lz_feed_available+1
    sbc dma_length+1
    sta lz_feed_available+1
    rts
!stage:
    lda #<CHUNK_BUFFER
    sta dma_destination
    sta dict_body_pointer
    lda #>CHUNK_BUFFER
    sta dma_destination+1
    sta dict_body_pointer+1
    jmp dict_read_block_consumed

// General block transfer toRAM: copy any prefetched part, then directDMA
// the suffix toits finaldestination. Calls occur under$34, including DC00.
dict_read_block:
    jsr dict_consume_length
    bcc dict_read_block_consumed
    rts
dict_read_block_consumed:
    lda lz_feed_available
    ora lz_feed_available+1
    bne !prefetched+
    // No prefix remains: a single directDMA replaces all RAMcopy setup.
    jmp cache_fetch
!prefetched:
    lda dma_destination
    sta lz_destination_pointer
    lda dma_destination+1
    sta lz_destination_pointer+1
    lda dma_length
    sta dict_tail_length
    lda dma_length+1
    sta dict_tail_length+1
    lda lz_feed_available+1
    cmp dma_length+1
    bcc !prefix_available+
    bne !all_prefix+
    lda lz_feed_available
    cmp dma_length
    bcc !prefix_available+
!all_prefix:
    lda dma_length
    sta dict_copy_left
    sta dict_prefix_part
    lda dma_length+1
    sta dict_copy_left+1
    sta dict_prefix_part+1
    bne !copy_setup+
    jmp !copy_setup+
!prefix_available:
    lda lz_feed_available
    sta dict_copy_left
    sta dict_prefix_part
    lda lz_feed_available+1
    sta dict_copy_left+1
    sta dict_prefix_part+1
!copy_setup:
    sec
    lda dict_tail_length
    sbc dict_prefix_part
    sta dict_tail_length
    lda dict_tail_length+1
    sbc dict_prefix_part+1
    sta dict_tail_length+1
    lda lz_feed_pointer
    sta lz_source_pointer
    lda lz_feed_pointer+1
    sta lz_source_pointer+1
!copy:
    lda dict_copy_left
    ora dict_copy_left+1
    beq !copied+
    lda dict_copy_left+1
    beq !small+
    lda #255
    bne !count+
!small:
    lda dict_copy_left
!count:
    sta lz_copy_count
    jsr lz_copy_forward
    sec
    lda dict_copy_left
    sbc lz_copy_count
    sta dict_copy_left
    lda dict_copy_left+1
    sbc #0
    sta dict_copy_left+1
    jmp !copy-
!copied:
    lda lz_source_pointer
    sta lz_feed_pointer
    lda lz_source_pointer+1
    sta lz_feed_pointer+1
    sec
    lda lz_feed_available
    sbc dict_prefix_part
    sta lz_feed_available
    lda lz_feed_available+1
    sbc dict_prefix_part+1
    sta lz_feed_available+1
    lda dict_tail_length
    ora dict_tail_length+1
    beq !done+
    lda lz_destination_pointer
    sta dma_destination
    lda lz_destination_pointer+1
    sta dma_destination+1
    lda dict_tail_length
    sta dma_length
    lda dict_tail_length+1
    sta dma_length+1
    jsr cache_fetch
!done:
    rts

record_malformed:
    lda #5
    sta fatal_error
    sec
    rts

record_complete:
    lda record_remaining
    ora record_remaining+1
    beq !consumed+
    jmp record_malformed
!consumed:
    lda #0
    sta lz_record_prepared
    lda record_flags
    and #1
    beq !published+
    inc framebuffer_swap_count
    bne !published+
    inc framebuffer_swap_count+1
!published:
    inc record_index
    bne !updated+
    inc record_index+1
!updated:
    inc blank_updates
    bne !eof+
    inc blank_updates+1
!eof:
    lda record_index+1
record_count_compare_high:
    cmp #>VIDEO_RECORD_COUNT
    bcc !lookahead+
    bne !ended+
    lda record_index
record_count_compare_low:
    cmp #<VIDEO_RECORD_COUNT
    bcc !lookahead+
!ended:
    lda #1
    sta playback_ended
    rts
!lookahead:
    jmp lz_prepare_next
switch_cache_bank:
    ldx cache_read_bank
    lda #0
    sta cache_ready,x
    lda cache_bit,x
    ora pending_refill_mask
    sta pending_refill_mask
    txa
    eor #1
    sta cache_read_bank
    inc cache_read_block
    bne !block_advanced+
    inc cache_read_block+1
!block_advanced:
    lda #1
    sta audio_pending_start
    lda #0
    sta cache_cursor
    sta cache_cursor+1
    .if (CACHE_SLOT_BANKS>1) { sta cache_cursor_high }
    rts

dict_literal_length_valid: .fill 256, (i<=DICT_MAX_LITERAL_BYTES && mod(i,5)==0 ? 0 : 1)
.label low_program_end = *
.assert "Wide resident program overlaps page framebuffer", * <= $1800, true

* = $ca00 "Generic player resident bridge"
browser_save_overlay:
    lda #$90 // RAM -> REU, immediate, address auto advance
    bne browser_overlay_transfer
browser_restore_overlay:
    lda #$91 // REU -> RAM
browser_overlay_transfer:
    pha
    lda #0
    sta $df09
    sta $df0a
    sta $df02
    sta $df04
    sta $df05
    lda #>BROWSER_OVERLAY
    sta $df03
    lda #BROWSER_REU>>16
    sta $df06
    lda #<BROWSER_BYTES
    sta $df07
    lda #>BROWSER_BYTES
    sta $df08
    pla
    sta $df01
    rts

generic_browser:
    sei
    ldx #$ff
    txs // Returning to browser discards the old playback call chain.
    lda #0
    sta $d01a
    sta $d011
    sta $dc0e
    sta $dc0f
    sta $df01
    lda #1
    sta $d019
    jsr audio_stop
    jsr uci_wait_complete
    lda file_open
    beq !closed+
    jsr uci_close_file
!closed:
    jsr browser_restore_overlay
    lda #0
    sta player_mode
    sta playback_paused
    sta pause_request
    sta audio_resume_active
    sta audio_resume_offset
    sta audio_resume_offset+1
    jsr browser_main
    lda #0
    ldx #$7f
!reset:
    sta $0400,x
    dex
    bpl !reset-
    sta cache_read_block
    sta cache_read_block+1
    sta restart_pending
    sta restart_in_progress
    sta keyboard_last_key
    sta eof_audio_stopped
    sta audio_tick_count
    sta audio_last_resume_offset
    sta audio_last_resume_offset+1
    sta $d011
    sta $d015
    sta $d020
    sta $d021
    lda #1
    sta file_open
    sta player_mode
    jsr load_initial
    jsr stream_startup
    inc playback_generation
    bne !started+
    inc playback_generation+1
!started:
    jmp generic_begin_scanout

// File remains open on a manual restart; seek rather than reopen/reload.
generic_seek_header_offset:
    lda #0
    sta uci_command_kind
    lda #2
    sta $df1d
    lda #6
    sta $df1d
    ldy #4
!offset:
    lda HEADER,x
    sta $df1d
    inx
    dey
    bne !offset-
    jsr uci_push
    jmp uci_wait_complete

// Table is interleaved u24, held below the stream cache in the REU.
// Three bytes and a little setup are paid once per 20 refreshes.
generic_audio_length:
    lda #0
    sta $df09
    sta $df0a
    sta $df08
    lda #<active_audio_length
    sta $df02
    lda #>active_audio_length
    sta $df03
    lda #3
    sta $df07
    lda cache_read_block
    asl
    sta bootstrap_offset
    lda cache_read_block+1
    rol
    sta bootstrap_offset+1
    lda #0
    rol
    sta bootstrap_offset+2
    clc
    lda bootstrap_offset
    adc cache_read_block
    sta $df04
    lda bootstrap_offset+1
    adc cache_read_block+1
    sta $df05
    lda bootstrap_offset+2
    adc #AUDIO_TABLE_REU>>16
    sta $df06
    lda #$91
    sta $df01
    lda active_audio_length+2
    bne !bad+
    lda active_audio_length+1
    cmp #$90
    bcc !bounded+
    bne !bad+
    lda active_audio_length
    bne !bad+
!bounded:
    lda active_audio_length
    and #3
    bne !bad+
    lda active_audio_length
    ora active_audio_length+1
    beq !bad+
    rts
!bad:
    lda #7
    sta fatal_error
    rts
#import "player_generic_pause.asm"
.label generic_resident_end=*
.assert "Generic resident exceeds free high RAM", *<=$d000, true

#import "player_generic_browser.asm"

* = KERNEL_BASE "UPIC1.3 first180-column pixel stream"
pixel_stream:
    .for (var column=0;column<COLUMNS;column++) {
        .assert "Wide kernel formula drift", * == kernel_opcode(column), true
        .if (mod(column,8)==7) {
            ldx #0
        } else {
            ldx wide_address(column),y
        }
        stx $d020
        lda NYBBLES,x
        sta $d020
    }
    nop
    nop
    lda #0
    sta $d020
    rts
.label pixel_stream_end = *
.assert "Wide kernel size changed", pixel_stream_end-pixel_stream==PIXEL_KERNEL_BYTES, true
.assert "Wide kernel overlaps short-pair preparation", * == PREP_BASE, true

prepare_short_pairs:
    .if (SPAN_VERSION!=9) {
        .for (var pair=0;pair<SHORT_PAIRS;pair++) {
            .var column=pair*8+7
            lda wide_address(column),y
            sta kernel_opcode(column)+1
        }
        rts
    }
.align $40
render_line:
    // SD centers the unchanged360-dot kernel at nativeX12. Moving two
    // prefetch bytes ahead of it and inlining preparation shifts pixel0
    // by95 CPU slots, while saving31 slots over the complete active line.
    // The legacy profile keeps nativeX0 and its original9+4 schedule.
    .if (SPAN_VERSION==9) {
        .for (var pair=0;pair<SHORT_PAIRS;pair++) {
            .var column=pair*8+7
            lda wide_address(column),y
            sta kernel_opcode(column)+1
        }
    } else {
        jsr prepare_short_pairs
    }
lz_prefetch_row_compare:
    cpy #ACTIVE_TOP
    bcs lz_no_prefetch
    // SD:4 compare/branch +12 setup +693 DMA +6 padding =715c.
    // Legacy:4 +12 +567 DMA +21 loop +4 NOP =608c.
    lda #PREFETCH_HEAD_BYTES_PER_ROW
    sta $df07
    lda #$91
    sta $df01
    ldx #4
wide_delay:
    .if (SPAN_VERSION!=9) {
        dex
        bne wide_delay
    }
    nop
    nop
    jsr pixel_stream
    jmp raw_post_prefetch
lz_no_prefetch:
    // Disabled prefetch must keep exactly the same first-pixel phase.
    ldx #(SPAN_VERSION==9 ? 141 : 120)
lz_idle_delay:
    dex
    bne lz_idle_delay
wide_phase_padding:
    nop
    .if (SPAN_VERSION==9) { nop }
    jsr pixel_stream
    jmp raw_post_prefetch
raw_post_prefetch:
raw_post_row_compare:
    cpy #ACTIVE_TOP
    bcs !done+
    // D020 is black at nativeX372 (SD) orX360 (legacy).
    // SD transfers2 trailing bytes; legacy transfers4. Total remains13.
    lda #PREFETCH_TAIL_BYTES_PER_ROW
    sta $df07
    lda #$91
    sta $df01
!done:
raw_post_done:
    rts
.label renderer_program_end = *
.assert "360 prep/wrapper overlaps prefetch buffer", * <= PREFETCH_BUFFER, true
.assert "RAW idle delay branch crosses a page", (lz_idle_delay>>8)==((lz_idle_delay+3)>>8), true
.assert "Wide delay branch crosses a page", (wide_delay>>8)==((wide_delay+3)>>8), true
.assert "Leading prefetch branch crosses a page", (lz_prefetch_row_compare>>8)==(lz_no_prefetch>>8), true
.assert "Trailing prefetch branch crosses a page", ((raw_post_row_compare+4)>>8)==(raw_post_done>>8), true

* = $d100 "Dictionary4-word decoder"
dict_decode_column:
    lda dict_dest_page
    .for (var word=0;word<DICT_WORDS;word++) {
        .for (var component=0;component<4;component++) {
            sta dict_words+word*32+10+component*6+2
        }
    }
    sta literal_put_0+2
    sta literal_put_1+2
    sta literal_put_2+2
    sta literal_put_3+2
    ldy #0
dict_words:
    .for (var word=0;word<DICT_WORDS;word++) {
        .assert "Dictionary unroll word size drift", * == dict_words+word*32, true
        lda (dict_indices),y
        tax
        cpx #$fe
        bcs !skip+
        .for (var component=0;component<4;component++) {
            lda $c600+component*256,x
            sta $2000+ACTIVE_TOP+word*4+component
        }
!skip:
        iny
    }
    lda dict_literal_bytes
    beq dict_primitive_done
    sta dict_literal_compare+1
    ldy #0
dict_literal_loop:
    lda (dict_literals),y
    cmp #(DICT_ROWS-3)
    bcs dict_bad_exception
    tax
    and #3
    bne dict_bad_exception
    iny
    lda (dict_literals),y
literal_put_0: sta $2000+ACTIVE_TOP,x
    iny
    lda (dict_literals),y
literal_put_1: sta $2001+ACTIVE_TOP,x
    iny
    lda (dict_literals),y
literal_put_2: sta $2002+ACTIVE_TOP,x
    iny
    lda (dict_literals),y
literal_put_3: sta $2003+ACTIVE_TOP,x
    iny
dict_literal_compare: cpy #0
    bne dict_literal_loop
dict_primitive_done: rts
dict_bad_exception: jmp record_malformed

.label dictionary_primitive_end = *
.assert "Dictionary primitive overlaps RAMcopy", * <= $da00, true
.align $40
lz_copy_forward:
    lda lz_copy_count
    cmp #40
    bcs lz_copy_large
    tax
    ldy #0
lz_copy_small:
    lda (lz_source_pointer),y
    sta (lz_destination_pointer),y
    iny
    dex
.label lz_copy_small_branch = *
    bne lz_copy_small
    tya
    jsr lz_advance_copy_pointers
    rts
.assert "LZ4 small copy branch crosses a page", (lz_copy_small>>8)==((lz_copy_small_branch+2)>>8), true
lz_copy_large:
    lda lz_copy_count
    and #3
    tax
    beq !group+
    ldy #0
!remainder:
    lda (lz_source_pointer),y
    sta (lz_destination_pointer),y
    iny
    dex
    bne !remainder-
    tya
    jsr lz_advance_copy_pointers
!group:
    lda lz_copy_count
    and #$fc
    sta lz_copy_aligned
    bne !has_group+
    rts
!has_group:
    jsr lz_advance_copy_pointers
    .for (var i=0;i<4;i++) {
        clc
        lda lz_source_pointer
        adc #i
        sta lz_copy_group+i*6+1
        lda lz_source_pointer+1
        adc #0
        sec
        sbc #1
        sta lz_copy_group+i*6+2
        clc
        lda lz_destination_pointer
        adc #i
        sta lz_copy_group+i*6+4
        lda lz_destination_pointer+1
        adc #0
        sec
        sbc #1
        sta lz_copy_group+i*6+5
    }
    lda #0
    sec
    sbc lz_copy_aligned
    tax
    jmp lz_copy_group
!done:
    rts
lz_advance_copy_pointers:
    pha
    clc
    adc lz_source_pointer
    sta lz_source_pointer
    lda lz_source_pointer+1
    adc #0
    sta lz_source_pointer+1
    pla
    clc
    adc lz_destination_pointer
    sta lz_destination_pointer
    lda lz_destination_pointer+1
    adc #0
    sta lz_destination_pointer+1
    rts
.align $40
lz_copy_group:
    .for (var i=0;i<4;i++) {
        lda $ffff,x
        sta $ffff,x
    }
    txa
    clc
    adc #4
    tax
.label lz_copy_branch = *
    bne lz_copy_group
    rts
.assert "LZ4 forward copy branch crosses a page", (lz_copy_group>>8)==((lz_copy_branch+2)>>8), true

.label buffered_shadow_end = *
.assert "Dictionary decoder exceeds shadowRAM", * <= CHUNK_BUFFER, true

* = $fc00 "Single PAL frame IRQ with primary UPIC1.3 raster polling"
frame_irq:
    pha
    txa
    pha
    tya
    pha
    cld
    lda #1
    sta $d019
    bit $d011
    bpl !low+
    jmp missed_frame
!low:
    lda $d012
    cmp #FRAME_IRQ_RASTER
    bcs !after_start+
    // A pending setup IRQ delayed by a whole frame is also too late.
    jmp missed_frame
!after_start:
    cmp #(FRAME_IRQ_RASTER+3)
    bcc !ontime+
    jmp missed_frame
!ontime:
    .if (STATIC_RENDERER==0) {
        lda playback_ended
        ora fatal_error
        beq !playing+
        jsr audio_stop
        lda playback_ended
        beq !audio_ready+
        lda #1
        sta eof_audio_stopped
        jmp !audio_ready+
!playing:
        jsr playback_control
        lda playback_paused
        bne !audio_ready+
        lda audio_pending_start
        beq !audio_ready+
        ldx cache_read_bank
        lda cache_ready,x
        bne !buffered+
        // Never start stale/in-flight PCM after an underrun.
        jsr audio_stop
        jmp !audio_ready+
!buffered:
        jsr audio_prepare
        lda fatal_error
        beq !pcm_valid+
        jmp !audio_ready+
!pcm_valid:
        jsr audio_start
        lda #0
        sta audio_pending_start
        lda pending_refill_mask
        ora cache_need_refill
        sta cache_need_refill
        lda #0
        sta pending_refill_mask
!audio_ready:
    }
    jsr lz_configure_prefetch
    ldy #ACTIVE_TOP
    sty scanout_row
    lda #TOP_SYNC_RASTER
!top:
    cmp $d012
    beq !first+
    lda $d012
    cmp #(TOP_SYNC_RASTER+1)
    bcc !top_again+
    jmp missed_frame
!top_again:
    lda #TOP_SYNC_RASTER
    jmp !top-
!first:
line_wait:
    lda $d012
line_edge:
    cmp $d012
    beq line_edge
    // Match the primary1.3 resync and JSR phase on every physical line.
    lda #$80
    ldx #$8f
    sta $d031
    stx $d031
    jsr render_line
    // A completed kernel must still be on the intended physical row.
    // This probes timing without changing any pre-pixel instruction.
    tya
    clc
    adc #WIDE_FIRST_RASTER
    cmp $d012
    beq !line_ok+
    inc upic_line_late_count
    bne !line_ok+
    inc upic_line_late_count+1
!line_ok:
    iny
    sty scanout_row
    cpy #((ACTIVE_TOP+ACTIVE_HEIGHT)&255)
    bne line_wait
frame_finished:
    lda #0
    sta $dc0e
    lda $dc0d
    .if (STATIC_RENDERER==0) { jsr buffered_blank_update }
    lda playback_paused
    bne !tick_counted+
    inc audio_tick_count
!tick_counted:
    inc video_frame_count
    bne irq_restore
    inc video_frame_count+1
irq_restore:
    pla
    tay
    pla
    tax
    pla
    rti
missed_frame:
    lda #0
    sta $dc0e
    lda $dc0d
    inc upic_missed_frames
    bne !counted+
    inc upic_missed_frames+1
!counted:
    jmp irq_restore
nmi_return:
    inc nmi_count
    rti

// Called from shadow $34; returns to shadow $34. Even tiny reads use the
// deferred FF00 trigger, so hidden D000..DFFF is always ordinary RAM.
cache_peek:
    lda #$35
    sta $01
    lda #0
    sta $df09
    sta $df0a
    lda cache_cursor
    sta $df04
    lda cache_cursor+1
    sta $df05
    lda cache_read_bank
    .for (var banks=CACHE_SLOT_BANKS;banks>1;banks=banks/2) { asl }
    clc
    adc #CACHE_BANK
    .if (CACHE_SLOT_BANKS>1) { adc cache_cursor_high }
    sta $df06
    lda dma_destination
    sta $df02
    lda dma_destination+1
    sta $df03
    lda dma_length
    sta $df07
    lda dma_length+1
    sta $df08
    lda #$81
    sta $df01
    lda #$34
    sta $01
    sta $ff00
    rts
cache_fetch:
    jsr cache_peek
cache_advance:
    clc
    lda cache_cursor
    adc dma_length
    sta cache_cursor
    lda cache_cursor+1
    adc dma_length+1
    sta cache_cursor+1
    .if (CACHE_SLOT_BANKS>1) {
        bcc !advanced+
        inc cache_cursor_high
!advanced:
    }
    rts

buffered_blank_update:
    lda #0
    sta record_update_active
    lda playback_ended
    ora fatal_error
    ora playback_paused
    beq !active+
    rts
!active:
    lda $d012
    sta update_raster_start
    lda $d011
    and #$80
    sta update_raster_start_high
    lda #$34
    sta $01
    jsr apply_buffered_record
    lda #$35
    sta $01
    lda $d012
    sta update_raster_end
    lda $d011
    and #$80
    sta update_raster_end_high
    bne !done+
    lda update_raster_start_high
    bne !wrapped+
    lda update_raster_end
    cmp update_raster_start
    bcs !done+
!wrapped:
    lda update_raster_end
    cmp #FRAME_IRQ_RASTER
    bcc !done+
    inc deadline_miss_count
    bne !done+
    inc deadline_miss_count+1
!done:
    lda #0
    sta record_update_active
    rts
audio_prepare:
    jsr generic_audio_length
    lda audio_resume_active
    bne !continuation+
    lda #0
    sta audio_tick_count
!continuation:
    lda #0
    sta $df20
    sta $df40
    sta $df21
    sta $df41
    lda #$ff
    sta $df3f
    .for (var voice=0;voice<2;voice++) {
        .var base=$df20+voice*$20
        .var address=AUDIO_BLOCK_OFFSET+voice*2
        lda #1
        sta base+4
        lda cache_read_bank
        .for (var banks=CACHE_SLOT_BANKS;banks>1;banks=banks/2) { asl }
        clc
        adc #(CACHE_BANK+((address>>16)&255))
        sta base+5
        lda #((address>>8)&255)
        clc
        adc audio_resume_offset+1
        sta base+6
        lda #<address
        clc
        adc audio_resume_offset
        sta base+7
        lda active_audio_length+2
        sta base+9
        lda active_audio_length+1
        ldx active_audio_length
        pha
        txa
        sec
        sbc audio_resume_offset
        tax
        pla
        sbc audio_resume_offset+1
        sta base+10
        txa
        sta base+11
        lda HEADER+33
        sta base+14
        lda HEADER+32
        sta base+15
        lda #(voice==0 ? 0 : 15)
        sta base+2
    }
    lda audio_resume_offset
    sta audio_last_resume_offset
    lda audio_resume_offset+1
    sta audio_last_resume_offset+1
    lda #0
    sta audio_resume_offset
    sta audio_resume_offset+1
    sta audio_resume_active
    rts
audio_start:
    lda #AUDIO_VOICE_VOLUME
    sta $df21
    sta $df41
    lda #$51 // enable, 16-bit, stride four bytes
    sta $df20
    sta $df40
    rts
audio_stop:
    lda #0
    sta $df20
    sta $df40
    sta $df21
    sta $df41
    rts

.label irq_program_end = *
.label program_end = *
.assert "Buffered IRQ overlaps the FF00 REU trigger", * <= $ff00, true
.assert "Wide line poll branch crosses a page", (line_edge>>8)==((line_edge+5)>>8), true
