#!/usr/bin/env python3
"""Build the PAL/3 and NTSC/4 standalone player using KickAssembler and Java."""
import argparse, hashlib, json, re, shutil
from pathlib import Path
from tempfile import TemporaryDirectory
import assemble as assembler
ROOT=Path(__file__).resolve().parent.parent
assemble=assembler.assemble

def package_hardware_loader(core, symbols, build):
    """KERNAL LOAD stays below I/O; SYS installs each banked segment itself."""
    data = core.read_bytes()
    origin = int.from_bytes(data[:2], 'little')
    ranges = [(0x0801, symbols['resident_core_end']),
              (0x1000, symbols['low_program_end']),
              (0x1800, symbols['browser_overlay_end']),
              (0xca00, symbols['generic_resident_end']),
              (0xd100, symbols['buffered_shadow_end']),
              (0xe800, symbols['renderer_program_end']),
              (0xfc00, symbols['irq_program_end'])]
    packed = bytearray()
    descriptors = []
    for start, end in ranges:
        assert origin <= start < end <= 65536
        descriptors.append((0x3000 + len(packed), start, end - start))
        packed.extend(data[2 + start - origin:2 + end - origin])
    assert 0x3000 + len(packed) < 0xca00
    # Relocate the tiny copier before restoring the old entry-point's bytes.
    # Its RAM at C400 is outside the 180 page-columns and dictionary LUT.
    text = '''*=$0801
:BasicUpstart2(cold_start)
cold_start:
    sei
    lda #$34
    sta $01
    ldx #0
!copy:
    lda loader_image,x
    sta $c400,x
    inx
    cpx #loader_image_end-loader_image
    bne !copy-
    jmp $c400
loader_image:
.pseudopc $c400 {
    ldx #0
!segment:
    stx $96
    lda copy_source_low,x
    sta $90
    lda copy_source_high,x
    sta $91
    lda copy_destination_low,x
    sta $92
    lda copy_destination_high,x
    sta $93
    lda copy_length_low,x
    sta $94
    lda copy_length_high,x
    sta $95
    ldy #0
!byte:
    lda ($90),y
    sta ($92),y
    inc $90
    bne !src+
    inc $91
!src:
    inc $92
    bne !dst+
    inc $93
!dst:
    lda $94
    bne !low+
    dec $95
!low:
    dec $94
    lda $94
    ora $95
    bne !byte-
    ldx $96
    inx
    cpx #7
    bne !segment-
    lda #$35
    sta $01
    jmp CORE_ENTRY
'''.replace('CORE_ENTRY', f'${symbols["start"]:04x}')
    for name, values in [('source', [x[0] for x in descriptors]),
                         ('destination', [x[1] for x in descriptors]),
                         ('length', [x[2] for x in descriptors])]:
        for part, shift in [('low', 0), ('high', 8)]:
            text += f'copy_{name}_{part}: .byte ' + ','.join(
                str((v >> shift) & 255) for v in values) + '\n'
    text += '''}
loader_image_end:
.assert "Resident copy stub must fit one page", loader_image_end-loader_image<256, true
*=$3000 "Packed player segments"
.import binary "segments.bin"
.assert "KERNAL LOAD must stay below I/O and resident loader", *<$ca00, true
'''
    with TemporaryDirectory(prefix='.generic-loader-', dir=build) as temporary:
        stage = Path(temporary)
        (stage / 'segments.bin').write_bytes(packed)
        (stage / 'loader.asm').write_text(text)
        assemble(stage / 'loader.asm', stage / 'player.prg')
        shutil.copyfile(stage / 'player.prg', build / 'mupic.prg')
        shutil.copyfile(stage / 'player.vs', build / 'loader.vs')
    return dict(load_origin=0x0801, load_end=0x3000 + len(packed),
                cold_start=0x080e, copier=0xc400,
                segments=[dict(source=a, destination=b, bytes=c)
                          for a, b, c in descriptors])

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--kickass',type=Path,required=True,help='Path to KickAss.jar')
    p.add_argument('--output',type=Path,default=ROOT/'build/player')
    a=p.parse_args();assembler.KICK_JAR=a.kickass.resolve();b=a.output.resolve();b.mkdir(parents=True,exist_ok=True)
    assemble(ROOT/'player/player_generic.asm',b/'mupic-core.prg')
    symbols={name:int(address,16) for address,name in re.findall(r'al C:([0-9a-fA-F]+) \.([^\s]+)',(b/'mupic-core.vs').read_text())}
    package_hardware_loader(b/'mupic-core.prg',symbols,b)
    # Per-program autoload must preserve the user's chosen PAL/NTSC and HDMI
    # mode. The reference configuration's SD setting belongs to the demo.
    settings=(ROOT/'releases/ultimate-pal.cfg').read_text().splitlines()
    (b/'MUPIC.cfg').write_text('\n'.join(line for line in settings
        if not line.startswith(('HDMI Scan Resolution=', 'System Mode=')))+'\n')
    if not (b/'MUPIC.PRG').exists() or not (b/'MUPIC.PRG').samefile(b/'mupic.prg'):
        shutil.copy2(b/'mupic.prg',b/'MUPIC.PRG')
    print(json.dumps(dict(player=str(b/'MUPIC.PRG'),sha256=hashlib.sha256((b/'MUPIC.PRG').read_bytes()).hexdigest()),indent=2))
if __name__=='__main__': main()
