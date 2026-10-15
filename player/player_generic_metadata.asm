// Browser-only metadata. Read one 256-byte header per file after draining
// READ_DIRECTORY; never load video/PCM or keep a file open while browsing.
// Cache 16 display bytes per entry so cursor redraws perform no disk I/O.
browser_metadata_address:
    ldx #0
    stx browser_list_ptr+1
    asl
    rol browser_list_ptr+1
    asl
    rol browser_list_ptr+1
    asl
    rol browser_list_ptr+1
    asl
    rol browser_list_ptr+1
    sta browser_list_ptr
    lda browser_list_ptr+1
    clc
    adc #>DIRECTORY_METADATA
    sta browser_list_ptr+1
    rts

browser_read_metadata:
    lda generic_error
    beq !start+
    rts
!start:
    lda #0
    sta browser_metadata_index
!entry:
    lda browser_metadata_index
    cmp browser_count
    bcc !more+
    rts
!more:
    jsr browser_entry_address
    lda browser_metadata_index
    jsr browser_metadata_address
    lda #32
    ldy #15
!blank:
    sta (browser_list_ptr),y
    dey
    bpl !blank-
    lda #0
    ldy #5
    sta (browser_list_ptr),y
    ldy #14
    sta (browser_list_ptr),y
    iny
    lda #$ff
    sta (browser_list_ptr),y
    ldy #0
    lda (browser_entry_ptr),y
    and #$10
    bne !next+
    // Unsupported/unreadable headers get placeholders, not fabricated data.
    lda #45
    sta (browser_list_ptr),y
    iny
    sta (browser_list_ptr),y
    iny
    lda #58
    sta (browser_list_ptr),y
    iny
    lda #45
    sta (browser_list_ptr),y
    iny
    sta (browser_list_ptr),y
    ldy #12
    sta (browser_list_ptr),y
    iny
    sta (browser_list_ptr),y
    jsr browser_open_selected
    bcs !next+
    jsr browser_read_header
    bcs !close+
    jsr browser_validate_header
    bcs !close+
    jsr browser_validate_counts
    bcs !close+
    jsr browser_format_metadata
!close:
    jsr uci_close_file
!next:
    inc browser_metadata_index
    jmp !entry-

browser_format_metadata:
    // Round the duration using the profile's exact PHI2 frame length/clock.
    // The decoder always consumes one record per raster refresh.
    lda HEADER+24
    sta browser_multiplier
    lda HEADER+25
    sta browser_multiplier+1
    ldx #0
    lda HEADER+80
    cmp #3
    bcc !standard+
    inx
!standard:
    lda browser_frame_low,x
    sta browser_factor
    lda browser_frame_high,x
    sta browser_factor+1
    lda browser_clock_0,x
    sta browser_metadata_clock+0
    lda browser_clock_1,x
    sta browser_metadata_clock+1
    lda browser_clock_2,x
    sta browser_metadata_clock+2
    lda #0
    sta browser_factor+2
    sta browser_factor+3
    sta browser_number
    sta browser_number+1
    sta browser_number+2
    sta browser_number+3
    ldx #16
!multiply:
    lsr browser_multiplier+1
    ror browser_multiplier
    bcc !shift+
    clc
    .for (var part=0;part<4;part++) {
        lda browser_number+part
        adc browser_factor+part
        sta browser_number+part
    }
!shift:
    asl browser_factor
    rol browser_factor+1
    rol browser_factor+2
    rol browser_factor+3
    dex
    bne !multiply-
    ldx #0
    lda HEADER+80
    cmp #3
    bcc !rounding+
    inx
!rounding:
    clc
    lda browser_number+0
    adc browser_half_clock_0,x
    sta browser_number+0
    lda browser_number+1
    adc browser_half_clock_1,x
    sta browser_number+1
    lda browser_number+2
    adc browser_half_clock_2,x
    sta browser_number+2
    lda browser_number+3
    adc #0
    sta browser_number+3
    lda #0
    sta browser_factor
    sta browser_factor+1
!seconds:
    lda browser_number+3
    bne !subtract+
    lda browser_number+2
    cmp browser_metadata_clock+2
    bcc !minutes+
    bne !subtract+
    lda browser_number+1
    cmp browser_metadata_clock+1
    bcc !minutes+
    bne !subtract+
    lda browser_number
    cmp browser_metadata_clock
    bcc !minutes+
!subtract:
    sec
    lda browser_number+0
    sbc browser_metadata_clock+0
    sta browser_number+0
    lda browser_number+1
    sbc browser_metadata_clock+1
    sta browser_number+1
    lda browser_number+2
    sbc browser_metadata_clock+2
    sta browser_number+2
    lda browser_number+3
    sbc #0
    sta browser_number+3
    inc browser_factor
    bne !seconds-
    inc browser_factor+1
    jmp !seconds-
!minutes:
    lda #0
    sta browser_multiplier
!minute:
    lda browser_factor+1
    bne !sixty+
    lda browser_factor
    cmp #60
    bcc !time+
!sixty:
    sec
    lda browser_factor
    sbc #60
    sta browser_factor
    lda browser_factor+1
    sbc #0
    sta browser_factor+1
    inc browser_multiplier
    jmp !minute-
!time:
    ldy #0
    lda browser_multiplier
    jsr browser_two_digits
    lda #58
    sta (browser_list_ptr),y
    iny
    lda browser_factor
    jsr browser_two_digits
    lda #0
    sta (browser_list_ptr),y

    // Canonical MUP9 lengths are multiples of 64 KiB. Tenths of a MiB:
    // round((file_bytes >> 16) * 10 / 16); multiply in 24 bits.
    lda HEADER+66
    sta browser_factor
    lda HEADER+67
    sta browser_factor+1
    lda #0
    sta browser_factor+2
    asl browser_factor
    rol browser_factor+1
    rol browser_factor+2
    ldx #2
!times_two:
    lda browser_factor,x
    sta browser_number,x
    dex
    bpl !times_two-
    asl browser_factor
    rol browser_factor+1
    rol browser_factor+2
    asl browser_factor
    rol browser_factor+1
    rol browser_factor+2
    clc
    .for (var part=0;part<3;part++) {
        lda browser_number+part
        adc browser_factor+part
        sta browser_number+part
    }
    clc
    lda browser_number
    adc #8
    sta browser_number
    lda browser_number+1
    adc #0
    sta browser_number+1
    lda browser_number+2
    adc #0
    sta browser_number+2
    ldx #4
!divide_sixteen:
    lsr browser_number+2
    ror browser_number+1
    ror browser_number
    dex
    bne !divide_sixteen-
    lda #0
    sta browser_leading
    sta browser_digits
    ldy #6
!digit:
    ldx browser_digits
    lda browser_size_low,x
    sta browser_factor
    lda browser_size_high,x
    sta browser_factor+1
    ldx #48
!decimal:
    lda browser_number+1
    cmp browser_factor+1
    bcc !write+
    bne !reduce+
    lda browser_number
    cmp browser_factor
    bcc !write+
!reduce:
    sec
    lda browser_number
    sbc browser_factor
    sta browser_number
    lda browser_number+1
    sbc browser_factor+1
    sta browser_number+1
    inx
    jmp !decimal-
!write:
    txa
    cpx #48
    bne !visible+
    ldx browser_digits
    cpx #2
    beq !visible+
    ldx browser_leading
    bne !visible+
    lda #32
    bne !store+
!visible:
    inc browser_leading
!store:
    sta (browser_list_ptr),y
    iny
    inc browser_digits
    lda browser_digits
    cmp #3
    bcc !digit-
    lda #46
    sta (browser_list_ptr),y
    iny
    lda browser_number
    clc
    adc #48
    sta (browser_list_ptr),y
    iny
    lda #77 // MiB, ASCII; browser_print performs mixed-case screen conversion.
    sta (browser_list_ptr),y
    iny
    lda #105
    sta (browser_list_ptr),y
    iny
    lda #66
    sta (browser_list_ptr),y
    iny
    lda #0
    sta (browser_list_ptr),y
    ldy #15
    lda HEADER+80
    sta (browser_list_ptr),y
    rts
browser_frame_low: .byte <(63*312),<(65*263)
browser_frame_high: .byte >(63*312),>(65*263)
browser_clock_0: .byte 160,10
browser_half_clock_0: .byte 80,133
browser_clock_1: .byte 8,155
browser_half_clock_1: .byte 132,205
browser_clock_2: .byte 15,15
browser_half_clock_2: .byte 7,7
browser_size_low: .byte <1000,<100,<10
browser_size_high: .byte >1000,>100,>10

browser_two_digits:
    ldx #48
!tens:
    cmp #10
    bcc !done+
    sec
    sbc #10
    inx
    bne !tens-
!done:
    pha
    txa
    sta (browser_list_ptr),y
    iny
    pla
    clc
    adc #48
    sta (browser_list_ptr),y
    iny
    rts
