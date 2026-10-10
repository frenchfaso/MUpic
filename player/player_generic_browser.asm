// Cold/paused overlay. Playback replaces this RAM with image pixels; the
// resident bridge saves/restores the code from REU $080000 while IRQs are off.
* = BROWSER_OVERLAY "Ultimate DOS directory browser and container loader"
browser_main:
    lda $dd00
    and #$fc
    ora #1 // VIC bank $8000: screen $a400, mixed-case character ROM at $9800.
    sta $dd00
    lda #$96
    sta $d018
    lda #$1b
    sta $d011
    lda #8
    sta $d016
    lda #0
    sta $d020
    sta $d021
    lda #1
    ldx #0
!colours:
    sta $d800,x
    sta $d900,x
    sta $da00,x
    sta $db00,x
    inx
    bne !colours-
    lda SELECTED_PATH
    bne !path_ready+
    ldx #0
!root:
    lda browser_root,x
    sta SELECTED_PATH,x
    inx
    cmp #0
    bne !root-
!path_ready:
    lda #0
    sta fatal_error
    sta uci_busy
    sta generic_error
    sta browser_key
browser_refresh:
    lda #0
    sta browser_selected
    sta browser_top
    jsr browser_read_directory
    jsr browser_read_metadata
    jsr browser_draw
    jsr browser_get_key
    sta browser_key // A held Return/RUN-STOP must be released after navigation.
browser_idle:
    jsr browser_get_key
    cmp browser_key
    beq browser_idle
    sta browser_key
    cmp #0
    beq browser_idle
    cmp #1
    beq browser_down
    cmp #2
    beq browser_up
    cmp #3
    beq browser_enter
    cmp #4
    beq browser_parent
    jmp browser_idle
browser_down:
    lda browser_selected
    clc
    adc #1
    cmp browser_count
    bcs browser_idle
    sta browser_selected
    sec
    sbc browser_top
    cmp #18
    bcc !draw+
    inc browser_top
!draw:
    jsr browser_draw
    jmp browser_idle
browser_up:
    lda browser_selected
    beq browser_idle
    dec browser_selected
    lda browser_selected
    cmp browser_top
    bcs !draw+
    dec browser_top
!draw:
    jsr browser_draw
    jmp browser_idle
browser_parent:
    jsr browser_path_parent
    jmp browser_refresh
browser_enter:
    lda browser_count
    beq browser_idle
    lda browser_selected
    jsr browser_entry_address
    ldy #0
    lda (browser_entry_ptr),y
    and #$10
    beq !file+
    jsr browser_path_append
    bcs !failed+
    jmp browser_refresh
!file:
    jsr browser_open_selected
    bcs !failed+
    jsr browser_load_container
    bcs !bad_container+
    rts
!bad_container:
    jsr uci_close_file
!failed:
    lda #7
    sta generic_error
    jsr browser_draw
    jmp browser_idle

// KickAssembler's default lowercase text encoding is PETSCII, whereas UCI
// filesystem paths are ASCII. Spell these bytes explicitly.
browser_root: .byte 47,85,115,98,48,0
browser_heading: .byte 77,85,112,105,99,32,80,108,97,121,101,114,0 // MUpic Player
browser_error: .text "ERROR: FILE OR PROFILE NOT SUPPORTED"
    .byte 0
browser_empty: .text "NO .MUP FILES OR FOLDERS"
    .byte 0
browser_limit_text: .text "FIRST 96 ENTRIES; USE SUBFOLDERS"
    .byte 0

// Directory entries retain complete names of <=254 bytes, not display names.
browser_entry_address:
    clc
    adc #>DIRECTORY_ENTRIES
    sta browser_entry_ptr+1
    lda #0
    sta browser_entry_ptr
    rts
browser_draw:
    lda #32
    ldx #0
!clear:
    sta $a400,x
    sta $a500,x
    sta $a600,x
    sta $a700,x
    inx
    bne !clear-
    lda #<browser_heading
    sta browser_text_ptr
    lda #>browser_heading
    sta browser_text_ptr+1
    lda #<($a400+14) // Twelve characters centered in forty columns.
    sta browser_screen_ptr
    lda #>$a400
    sta browser_screen_ptr+1
    jsr browser_print
    lda #<SELECTED_PATH
    sta browser_text_ptr
    lda #>SELECTED_PATH
    sta browser_text_ptr+1
    lda #<$a450
    sta browser_screen_ptr
    jsr browser_print
    lda #0
    sta browser_temp
!entry:
    lda browser_temp
    cmp #18
    bcc !row+
    jmp !footer+
!row:
    clc
    adc browser_top
    cmp browser_count
    bcc !present+
    jmp !footer+
!present:
    pha
    jsr browser_entry_address
    lda browser_entry_ptr
    clc
    adc #1
    sta browser_text_ptr
    lda browser_entry_ptr+1
    adc #0
    sta browser_text_ptr+1
    ldy browser_temp
    lda browser_row_low,y
    sta browser_screen_ptr
    lda browser_row_high,y
    sta browser_screen_ptr+1
    ldy #0
    pla
    pha
    cmp browser_selected
    bne !plain+
    lda #62
    sta (browser_screen_ptr),y
!plain:
    iny
    lda (browser_entry_ptr),y // first name byte, label handled below
    ldy #0
    lda (browser_entry_ptr),y
    and #$10
    beq !name+
    iny
    lda #47
    sta (browser_screen_ptr),y
!name:
    inc browser_screen_ptr
    inc browser_screen_ptr
    ldx #21 // Filename columns 2..22; full name remains available for OPEN.
    jsr browser_print_limited
    pla
    jsr browser_metadata_address
    lda browser_list_ptr
    sta browser_text_ptr
    lda browser_list_ptr+1
    sta browser_text_ptr+1
    ldy browser_temp
    lda browser_row_low,y
    clc
    adc #24
    sta browser_screen_ptr
    lda browser_row_high,y
    adc #0
    sta browser_screen_ptr+1
    jsr browser_print
    clc
    lda browser_text_ptr
    adc #6
    sta browser_text_ptr
    lda browser_text_ptr+1
    adc #0
    sta browser_text_ptr+1
    clc
    lda browser_screen_ptr
    adc #7
    sta browser_screen_ptr
    bcc !size+
    inc browser_screen_ptr+1
!size:
    jsr browser_print
    inc browser_temp
    jmp !entry-
!footer:
    lda generic_error
    beq !not_error+
    lda #<browser_error
    sta browser_text_ptr
    lda #>browser_error
    bne !message+
!not_error:
    lda browser_limit
    beq !not_limit+
    lda #<browser_limit_text
    sta browser_text_ptr
    lda #>browser_limit_text
    bne !message+
!not_limit:
    lda browser_count
    bne !done+
    lda #<browser_empty
    sta browser_text_ptr
    lda #>browser_empty
!message:
    sta browser_text_ptr+1
    lda #<$a7c0
    sta browser_screen_ptr
    lda #>$a7c0
    sta browser_screen_ptr+1
    jsr browser_print
!done:
    rts
browser_row_low: .fill 18, <($a4a0+i*40)
browser_row_high: .fill 18, >($a4a0+i*40)

browser_print:
    ldx #38
browser_print_limited:
    stx browser_print_width+1
    ldy #0
!char:
    lda (browser_text_ptr),y
    beq !done+
    cmp #96
    bcc !screen+
    cmp #128
    bcs !screen+
    and #$1f // ASCII lowercase $61..$7a -> screen codes $01..$1a.
!screen:
    sta (browser_screen_ptr),y
    iny
browser_print_width:
    cpy #38
    bcc !char-
!done:
    rts

browser_get_key:
    lda #$fe
    sta $dc00
    lda $dc01
    sta browser_temp
    and #2
    beq !return+
    lda browser_temp
    and #$80
    beq !cursor+
    lda browser_temp
    and #4
    beq !parent+
    lda #$7f
    sta $dc00
    lda $dc01
    and #$80
    beq !parent+
    lda #0
    beq !done+
!cursor:
    lda #$fd
    sta $dc00
    lda $dc01
    and #$80
    beq !up+
    lda #$bf
    sta $dc00
    lda $dc01
    and #$10
    beq !up+
    lda #1
    bne !done+
!up:
    lda #2
    bne !done+
!return:
    lda #3
    bne !done+
!parent:
    lda #4
!done:
    pha
    lda #$ff
    sta $dc00
    pla
    rts

browser_path_parent:
    ldx #0
!end:
    lda SELECTED_PATH,x
    beq !back+
    inx
    bne !end-
!back:
    dex
    beq !root+
    lda SELECTED_PATH,x
    cmp #47
    bne !back-
    lda #0
    sta SELECTED_PATH,x
    rts
!root:
    lda #47
    sta SELECTED_PATH
    lda #0
    sta SELECTED_PATH+1
    rts
browser_path_append:
    ldx #0
!end:
    lda SELECTED_PATH,x
    beq !found+
    inx
    bne !end-
!too_long:
    sec
    rts
!found:
    cpx #250
    bcs !too_long-
    stx browser_path_length
    cpx #1
    beq !root+
    lda #47
    sta SELECTED_PATH,x
    inx
!root:
    ldy #1
!copy:
    lda (browser_entry_ptr),y
    sta SELECTED_PATH,x
    beq !done+
    iny
    inx
    bne !copy-
    ldx browser_path_length
    lda #0
    sta SELECTED_PATH,x
    sec
    rts
!done:
    clc
    rts

browser_read_directory:
    lda #0
    sta browser_count
    sta browser_limit
    sta generic_error
    lda $df1d
    cmp #$c9
    beq !interface+
    lda #2
    sta generic_error
    rts
!interface:
    lda #2
    sta $df1d
    lda #$11
    sta $df1d
    ldx #0
!path:
    lda SELECTED_PATH,x
    sta $df1d
    inx
    cmp #0
    bne !path-
    jsr browser_reply_ignore
    bcs !error+
    lda #2
    sta $df1d
    lda #$13
    sta $df1d
    jsr browser_reply_ignore
    bcs !empty+
    lda #2
    sta $df1d
    lda #$14
    sta $df1d
    lda #2 // Directory entry per reply chunk
    sta browser_command
    jsr browser_push
!part:
    lda browser_count
    jsr browser_entry_address
    jsr browser_wait_part
    lda browser_reply_state
    cmp #$30
    beq !part-
    lda browser_status_code
    ora browser_status_code+1
    bne !error+
!empty:
    rts
!error:
    lda #1
    sta generic_error
    rts

browser_open_selected:
    lda #0
    sta fatal_error
    lda #2
    sta $df1d
    sta $df1d
    lda #1
    sta $df1d
    ldx #0
!path:
    lda SELECTED_PATH,x
    beq !separator+
    sta $df1d
    inx
    bne !path-
!separator:
    cpx #1
    beq !name+
    lda #47
    sta $df1d
!name:
    ldy #1
!copy:
    lda (browser_entry_ptr),y
    sta $df1d
    beq !open+
    iny
    bne !copy-
!open:
    jsr browser_reply_ignore
    bcs !done+
    lda #1
    sta file_open
!done:
    rts

// UCI transactions must acknowledge every response fragment before issuing
// OPEN_FILE, LOAD_REU or the next directory command. No deprecated UI path copy.
browser_reply_ignore:
    lda #0
    sta browser_command
    jsr browser_push
!next:
    jsr browser_wait_part
    lda browser_reply_state
    cmp #$30
    beq !next-
    lda browser_status_code
    ora browser_status_code+1
    beq !ok+
    sec
    rts
!ok:
    clc
    rts
browser_push:
    lda #0
    sta browser_status_size
    sta browser_status_code
    sta browser_status_code+1
    lda #1
    sta $df1c
    rts
browser_wait_part:
    lda #0
    sta browser_response_size
    sta browser_response_size+1
!wait:
    lda $df1c
    and #$30
    cmp #$20
    beq !available+
    cmp #$30
    bne !wait-
!available:
    sta browser_reply_state
!data:
    lda $df1c
    bpl !status+
    lda $df1e
    ldx browser_command
    beq !prefix+
    cpx #1
    bne !directory+
    // Exact 256-byte header; excess data remains drained but never stored.
    ldx browser_response_size+1
    bne !count+
    ldx browser_response_size
    sta HEADER,x
    jmp !count+
!prefix:
    ldx browser_response_size+1
    bne !count+
    ldx browser_response_size
    cpx #7
    bcs !count+
    sta uci_response_prefix,x
    jmp !count+
!directory:
    ldy browser_response_size+1
    bne !count+
    ldy browser_response_size
    cpy #255
    bcs !count+
    ldx browser_count
    cpx #DIRECTORY_CAPACITY
    bcs !count+
    sta (browser_entry_ptr),y
!count:
    inc browser_response_size
    bne !data-
    inc browser_response_size+1
    jmp !data-
!status:
    lda $df1c
    and #$40
    beq !accept+
    lda $df1f
    ldx browser_status_size
    cpx #2
    bcs !status-
    eor #48
    sta browser_status_code,x
    inc browser_status_size
    jmp !status-
!accept:
    lda #2
    sta $df1c
    lda browser_command
    cmp #2
    bne !done+
    jsr browser_filter_entry
!done:
    rts
browser_filter_entry:
    lda browser_response_size+1
    bne !done+
    lda browser_response_size
    cmp #2
    bcc !done+
    cmp #255
    bcs !done+
    tay
    lda browser_count
    cmp #DIRECTORY_CAPACITY
    bcc !space+
    lda #1
    sta browser_limit
    rts
!space:
    lda #0
    sta (browser_entry_ptr),y
    ldy #0
    lda (browser_entry_ptr),y
    and #$10
    beq !extension+
    iny
    lda (browser_entry_ptr),y
    cmp #46 // Omit only '.' and '..'; keep other dot-prefixed folders.
    bne !keep+
    iny
    lda (browser_entry_ptr),y
    beq !done+
    cmp #46
    bne !keep+
    iny
    lda (browser_entry_ptr),y
    beq !done+
    bne !keep+
!extension:
    ldy browser_response_size
    dey
    lda (browser_entry_ptr),y
    and #$df
    cmp #80
    bne !done+
    dey
    lda (browser_entry_ptr),y
    and #$df
    cmp #85
    bne !done+
    dey
    lda (browser_entry_ptr),y
    and #$df
    cmp #77
    bne !done+
    dey
    beq !done+
    lda (browser_entry_ptr),y
    cmp #46
    bne !done+
!keep:
    inc browser_count
!done:
    rts

browser_read_header:
    lda #2
    sta $df1d
    lda #4 // READ_DATA, 256 bytes
    sta $df1d
    lda #0
    sta $df1d
    lda #1
    sta $df1d
    sta browser_command
    jsr browser_push
    jsr browser_wait_part
    lda browser_reply_state
    cmp #$20
    beq !check_next+
    jmp !bad+
!check_next:
    lda browser_response_size
    beq !check_next+
    jmp !bad+
!check_next:
    lda browser_response_size+1
    cmp #1
    beq !check_next+
    jmp !bad+
!check_next:
    lda browser_status_code
    ora browser_status_code+1
    beq !check_next+
    jmp !bad+
!check_next:
    clc
    rts
!bad:
    sec
    rts

browser_load_container:
    jsr browser_read_header
    bcc !check_next+
    jmp !bad+
!check_next:
    jsr browser_validate_header
    bcc !check_next+
    jmp !bad+
!check_next:
    jsr browser_validate_counts
    bcc !counts_valid+
    jmp !bad+
!counts_valid:
    // File pointer is now 256, precisely the start of the PCM-length table.
    lda active_stream_blocks
    sta bootstrap_length
    lda active_stream_blocks+1
    sta bootstrap_length+1
    lda #0
    sta bootstrap_length+2
    asl bootstrap_length
    rol bootstrap_length+1
    rol bootstrap_length+2
    clc
    lda bootstrap_length
    adc active_stream_blocks
    sta bootstrap_length
    lda bootstrap_length+1
    adc active_stream_blocks+1
    sta bootstrap_length+1
    lda #0
    sta bootstrap_reu
    sta bootstrap_reu+1
    lda #AUDIO_TABLE_REU>>16
    sta bootstrap_reu+2
    jsr browser_load_reu
    bcc !check_next+
    jmp !bad+
!check_next:
    ldx #48
    jsr generic_seek_header_offset
    lda fatal_error
    beq !check_next+
    jmp !bad+
!check_next:
    lda #<FRAME_BYTES
    sta bootstrap_length
    lda #>FRAME_BYTES
    sta bootstrap_length+1
    lda #0
    sta bootstrap_length+2
    sta bootstrap_reu
    sta bootstrap_reu+1
    sta bootstrap_reu+2
    jsr browser_load_reu
    bcc !check_next+
    jmp !bad+
!check_next:
    clc
    rts
!bad:
    sec
    rts
browser_load_reu:
    lda #2
    sta $df1d
    lda #$21
    sta $df1d
    ldx #0
!destination:
    lda bootstrap_reu,x
    sta $df1d
    inx
    cpx #3
    bne !destination-
    lda #0
    sta $df1d
    ldx #0
!length:
    lda bootstrap_length,x
    sta $df1d
    inx
    cpx #3
    bne !length-
    lda #0
    sta $df1d
    jsr browser_reply_ignore
    bcs !bad+
    lda uci_response_prefix
    cmp #36
    bne !bad+
    .for (var part=2;part>=0;part--) {
        lda bootstrap_length+part
        lsr
        lsr
        lsr
        lsr
        tax
        lda browser_hex,x
        cmp uci_response_prefix+(2-part)*2+1
        bne !bad+
        lda bootstrap_length+part
        and #15
        tax
        lda browser_hex,x
        cmp uci_response_prefix+(2-part)*2+2
        bne !bad+
    }
    clc
    rts
!bad:
    sec
    rts
browser_hex: .text "0123456789ABCDEF"

browser_validate_header:
    ldx #0
!fixed:
    lda browser_header_offsets,x
    tay
    lda HEADER,y
    cmp browser_header_values,x
    beq !check_next+
    jmp !bad+
!check_next:
    inx
    cpx #browser_header_values_end-browser_header_values
    bcc !fixed-
    lda HEADER+7
    and #$fe
    beq !check_next+
    jmp !bad+
!check_next:
    lda HEADER+26
    ora HEADER+27
    ora HEADER+30
    ora HEADER+31
    beq !check_next+
    jmp !bad+
!check_next:
    lda HEADER+24
    ora HEADER+25
    bne !check_next+
    jmp !bad+
!check_next:
    lda HEADER+28
    ora HEADER+29
    bne !check_next+
    jmp !bad+
!check_next:
    lda HEADER+29
    cmp #64 // 256KiB blocks + prefix must fit below 4GiB.
    bcc !check_next+
    jmp !bad+
!check_next:
    lda HEADER+32
    ora HEADER+33
    bne !check_next+
    jmp !bad+
!check_next:
    lda HEADER+72
    bne !check_next+
    jmp !bad+
!check_next:
    cmp #21
    bcc !check_next+
    jmp !bad+
!check_next:
    lda HEADER+48
    ora HEADER+51
    ora HEADER+56
    ora HEADER+57
    ora HEADER+59
    beq !check_next+
    jmp !bad+
!check_next:
    lda HEADER+24
    sta active_record_count
    sta record_count_compare_low+1
    lda HEADER+25
    sta active_record_count+1
    sta record_count_compare_high+1
    lda HEADER+28
    sta active_stream_blocks
    lda HEADER+29
    sta active_stream_blocks+1
    // Exact stream length, u32 blocks*262144.
    lda HEADER+28
    asl
    sta bootstrap_length
    lda HEADER+29
    rol
    sta bootstrap_length+1
    asl bootstrap_length
    rol bootstrap_length+1
    lda bootstrap_length
    cmp HEADER+62
    beq !check_next+
    jmp !bad+
!check_next:
    lda bootstrap_length+1
    cmp HEADER+63
    beq !check_next+
    jmp !bad+
!check_next:
    clc
    lda bootstrap_length
    adc HEADER+58
    sta bootstrap_offset
    lda bootstrap_length+1
    adc #0
    sta bootstrap_offset+1
    lda bootstrap_offset
    cmp HEADER+66
    beq !check_next+
    jmp !bad+
!check_next:
    lda bootstrap_offset+1
    cmp HEADER+67
    beq !check_next+
    jmp !bad+
!check_next:
    // Initial/table layout is canonical and cannot cross into menu/cache REU.
    lda HEADER+28
    sta bootstrap_length
    lda HEADER+29
    sta bootstrap_length+1
    asl bootstrap_length
    rol bootstrap_length+1
    clc
    lda bootstrap_length
    adc HEADER+28
    sta bootstrap_length
    lda bootstrap_length+1
    adc HEADER+29
    sta bootstrap_length+1
    lda bootstrap_length
    cmp #1
    lda bootstrap_length+1
    adc #1
    cmp HEADER+49
    beq !check_next+
    jmp !bad+
!check_next:
    lda #0
    cmp HEADER+50
    beq !check_next+
    jmp !bad+
!check_next:
    clc
    lda HEADER+49
    adc #>FRAME_BYTES
    sta bootstrap_length
    lda HEADER+50
    adc #0
    sta bootstrap_length+1
    lda bootstrap_length
    cmp #1
    lda bootstrap_length+1
    adc #0
    cmp HEADER+58
    beq !check_next+
    jmp !bad+
!check_next:
    clc
    rts
!bad:
    sec
    rts
browser_header_offsets:
    .byte 0,1,2,3,4,5,6,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23
    .byte 34,35,36,37,38,39,40,41,42,43,44,45,46,47,52,53,54,55
    .byte 60,61,64,65,73
browser_header_values:
    .byte 77,85,80,57,9,SPAN_VERSION,13,0,1,<WIDE_WIDTH,>WIDE_WIDTH,<WIDE_HEIGHT,>WIDE_HEIGHT,<ACTIVE_WIDTH,>ACTIVE_WIDTH,<ACTIVE_HEIGHT,>ACTIVE_HEIGHT,<ACTIVE_TOP,>ACTIVE_TOP,3,0,13,0
    .byte 20,0,0,112,3,0,0,0,4,0,0,1,0,0,0,180,0,0
    .byte 0,0,0,0,0
browser_header_values_end:

// Semantic counts are checked before any payload DMA. Divider/geometry and
// canonical offsets are validated separately; these divisions run only at OPEN.
browser_validate_counts:
    lda HEADER+24
    sta bootstrap_length
    lda HEADER+25
    sta bootstrap_length+1
    lda #0
    sta bootstrap_offset
    sta bootstrap_offset+1
!divide_twenty:
    lda bootstrap_length+1
    bne !subtract_twenty+
    lda bootstrap_length
    cmp #21
    bcc !twenty_remainder+
!subtract_twenty:
    sec
    lda bootstrap_length
    sbc #20
    sta bootstrap_length
    lda bootstrap_length+1
    sbc #0
    sta bootstrap_length+1
    inc bootstrap_offset
    bne !divide_twenty-
    inc bootstrap_offset+1
    jmp !divide_twenty-
!twenty_remainder:
    lda bootstrap_length
    cmp HEADER+72
    bne !bad+
    inc bootstrap_offset
    bne !blocks+
    inc bootstrap_offset+1
!blocks:
    lda bootstrap_offset
    cmp HEADER+28
    bne !bad+
    lda bootstrap_offset+1
    cmp HEADER+29
    bne !bad+
    lda HEADER+24
    sta bootstrap_length
    lda HEADER+25
    sta bootstrap_length+1
    lda #0
    sta bootstrap_offset
    sta bootstrap_offset+1
!divide_three:
    lda bootstrap_length+1
    bne !subtract_three+
    lda bootstrap_length
    cmp #3
    bcc !three_remainder+
!subtract_three:
    sec
    lda bootstrap_length
    sbc #3
    sta bootstrap_length
    lda bootstrap_length+1
    sbc #0
    sta bootstrap_length+1
    inc bootstrap_offset
    bne !divide_three-
    inc bootstrap_offset+1
    jmp !divide_three-
!three_remainder:
    lda bootstrap_length
    bne !bad+
    lda HEADER+70
    ora HEADER+71
    bne !bad+
    lda bootstrap_offset
    cmp HEADER+68
    bne !bad+
    lda bootstrap_offset+1
    cmp HEADER+69
    bne !bad+
    clc
    rts
!bad:
    sec
    rts
.import source "player_generic_metadata.asm"
.label browser_overlay_end=*
.assert "Browser code overlaps formatted metadata cache", *<=DIRECTORY_METADATA, true
.assert "Formatted metadata exceeds overlay save range", DIRECTORY_METADATA+16*DIRECTORY_CAPACITY<=BROWSER_OVERLAY+BROWSER_BYTES, true
