// Called before scanout: no renderer or REU cache cursor advances in pause.
playback_control:
    lda pause_request
    beq !done+
    cmp #1
    bne !resume+
    jsr audio_stop
    lda audio_pending_start
    beq !current_block+
    lda #0 // Lookahead selected a block whose PCM has not started yet.
    sta audio_tick_count
!current_block:
    lda #1
    sta playback_paused
    jmp !clear+
!resume:
    lda #0
    sta playback_paused
    lda #1
    sta audio_pending_start
!clear:
    lda #0
    sta pause_request
!done:
    rts

// The sampler has no readable position/pause register. A refresh-edge pause is
// resumed at the nearest stereo sample interpolated within the authored PCM
// block. Its full byte length anchors each calculation: no cross-block drift.
// Main executes this arithmetic while IRQs continue scanning the frozen image.
prepare_audio_resume:
    lda #0
    sta audio_resume_offset
    sta audio_resume_offset+1
    sta audio_resume_offset+2
    sta audio_resume_active
    lda audio_pending_start
    beq !current+
    rts
!current:
    lda #1
    sta audio_resume_active
    lda #20
    sta audio_calc_ticks
    sec
    lda active_stream_blocks
    sbc #1
    sta audio_calc_last_block
    lda active_stream_blocks+1
    sbc #0
    sta audio_calc_last_block+1
    lda cache_read_block
    cmp audio_calc_last_block
    bne !full_block+
    lda cache_read_block+1
    cmp audio_calc_last_block+1
    bne !full_block+
    lda HEADER+72
    sta audio_calc_ticks
!full_block:
    lda active_audio_length
    sta audio_calc_samples
    lda active_audio_length+1
    sta audio_calc_samples+1
    lda active_audio_length+2
    sta audio_calc_samples_high
    lsr audio_calc_samples_high
    ror audio_calc_samples+1
    ror audio_calc_samples
    lda HEADER+7
    and #2
    bne !samples_ready+
    lsr audio_calc_samples_high
    ror audio_calc_samples+1
    ror audio_calc_samples
!samples_ready:
    lda #0
    sta audio_calc_sum
    sta audio_calc_sum+1
    sta audio_calc_sum+2
    sta audio_calc_remainder
    ldy audio_tick_count
    bne !multiply+
    rts
!multiply:
    clc
    lda audio_calc_sum
    adc audio_calc_samples
    sta audio_calc_sum
    lda audio_calc_sum+1
    adc audio_calc_samples+1
    sta audio_calc_sum+1
    lda audio_calc_sum+2
    adc audio_calc_samples_high
    sta audio_calc_sum+2
    dey
    bne !multiply-
    ldx #24
!divide:
    asl audio_calc_sum
    rol audio_calc_sum+1
    rol audio_calc_sum+2
    rol audio_calc_remainder
    lda audio_calc_remainder
    cmp audio_calc_ticks
    bcc !bit+
    sbc audio_calc_ticks
    sta audio_calc_remainder
    inc audio_calc_sum // The just-shifted low bit is zero.
!bit:
    dex
    bne !divide-
    lda audio_calc_remainder
    asl
    cmp audio_calc_ticks
    bcc !rounded+
    bne !round_up+
    lda audio_calc_sum
    and #1 // round ties to even, matching the mux
    beq !rounded+
!round_up:
    inc audio_calc_sum
    bne !rounded+
    inc audio_calc_sum+1
!rounded:
    asl audio_calc_sum
    rol audio_calc_sum+1
    rol audio_calc_sum+2
    lda HEADER+7
    and #2
    bne !bytes_ready+
    asl audio_calc_sum
    rol audio_calc_sum+1
    rol audio_calc_sum+2
!bytes_ready:
    lda audio_calc_sum
    sta audio_resume_offset
    lda audio_calc_sum+1
    sta audio_resume_offset+1
    lda audio_calc_sum+2
    sta audio_resume_offset+2
    // Keep a nonzero aligned tail at extremely low sample rates.
    cmp active_audio_length+2
    bcc !done+
    bne !clamp+
    lda audio_resume_offset+1
    cmp active_audio_length+1
    bcc !done+
    bne !clamp+
    lda audio_resume_offset
    cmp active_audio_length
    bcc !done+
!clamp:
    ldx #4
    lda HEADER+7
    and #2
    beq !width+
    ldx #2
!width:
    stx audio_calc_samples
    sec
    lda active_audio_length
    sbc audio_calc_samples
    sta audio_resume_offset
    lda active_audio_length+1
    sbc #0
    sta audio_resume_offset+1
    lda active_audio_length+2
    sbc #0
    sta audio_resume_offset+2
!done:
    rts
