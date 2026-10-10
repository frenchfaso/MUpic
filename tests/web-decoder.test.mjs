import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {applyRecord, framebufferToRgba, crc32, readMupHeader, MupPreview, PALETTE} from '../site/mup-decoder.mjs';
const fresh = () => ({front:new Uint8Array(46080),dictionary:null,previousColumn:-1});
const dictionary = () => { const bytes = new Uint8Array(1026); bytes[0]=2; for(let p=0;p<4;p++) bytes[2+p*256+7]=[0x21,0x43,0x65,0x87][p]; return bytes; };
test('CRC32 uses the standard container polynomial',()=>assert.equal(crc32(new TextEncoder().encode('123456789')),0xcbf43926));
test('dictionary, unchanged token, raw escape and publication reconstruct pixels',()=>{
  const state=fresh(), first=dictionary(); applyRecord(first,state);
  const record=new Uint8Array(2+2+50+5); record[0]=1; record[1]=1; record[2]=1; record[3]=5;
  record.fill(254,4,54); record[4]=7; record[53]=255;
  record.set([196,0x11,0x22,0x33,0x44],54); applyRecord(record,state);
  assert.deepEqual([...state.front.subarray(256,260)],[0x21,0x43,0x65,0x87]);
  assert.deepEqual([...state.front.subarray(452,456)],[0x11,0x22,0x33,0x44]);
  const rgba=framebufferToRgba(state.front); assert.deepEqual([...rgba.subarray(8,11)],PALETTE[1]);
  assert.deepEqual([...rgba.subarray(12,15)],PALETTE[2]);
});
test('bad escape offsets, borders and truncated dictionaries are rejected',()=>{
  assert.throws(()=>applyRecord(Uint8Array.of(2,0),fresh()),/dictionary/);
  const state=fresh(); applyRecord(dictionary(),state);
  assert.throws(()=>applyRecord(Uint8Array.of(0,1,0,255),state),/border/);
  const escape=new Uint8Array(59); escape.set([0,1,1,5]); escape.fill(254,4,54); escape[4]=255; escape.set([4,1,2,3,4],54);
  assert.throws(()=>applyRecord(escape,state),/escape row/);
});
test('complete native SD MUP fixtures agree with independent encoder SHA256 oracles', {skip: !process.env.MUP_FIXTURE}, async()=>{
  const {openAsBlob}=await import('node:fs');
  const preview=await MupPreview.open(await openAsBlob(process.env.MUP_FIXTURE));
  const oracles=JSON.parse(await readFile(process.env.MUP_ORACLES,'utf8'));
  const indices=[0,1,2,19,20,39,60,61,200,preview.header.frameCount-1,3,61,1]
    .filter(index=>index<preview.header.frameCount);
  for(const index of indices) {
    await preview.frame(index);
    assert.equal(createHash('sha256').update(preview.state.front).digest('hex'),oracles.logical_frame_sha256[index],`frame${index}`);
  }
  const raw=new Uint8Array(await preview.blob.slice(0,256).arrayBuffer()); raw[5]=8;
  assert.throws(()=>readMupHeader(raw,preview.blob.size),/checksum/);
});
