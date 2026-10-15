import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {applyRecord, framebufferToRgba, crc32, readMupHeader, MupPreview, PALETTE, PAL_HZ, NTSC_HZ} from '../site/mup-decoder.mjs';
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
function checksum(header) {
  const checked=new Uint8Array(252);checked.set(header.subarray(0,224));checked.set(header.subarray(228),224);
  new DataView(header.buffer,header.byteOffset,header.byteLength).setUint32(224,crc32(checked),true);
}
function profileFixture(profile,pcm8=false,frames=66) {
  const ticks=profile>2?4:3,records=frames*ticks,blocks=Math.ceil(records/20),streamOffset=65536;
  const file=new Uint8Array(streamOffset+blocks*262144),header=file.subarray(0,256),view=new DataView(file.buffer);
  header.set(new TextEncoder().encode('MUP9'));header.set([9,9,13,pcm8?3:1],4);
  for(const [at,value] of [[8,256],[10,360],[12,200],[14,profile?360:356],[16,200],[32,Math.round(6250000/44100-(pcm8?1.5:2))],[34,20],[72,records-(blocks-1)*20]])view.setUint16(at,value,true);
  header[20]=ticks;header[22]=13;header[80]=profile;
  for(const [at,value] of [[24,records],[28,blocks],[36,pcm8?221184:profile?184320:225280],[40,262144],[44,256],[48,512],[52,46080],[56,streamOffset],[60,blocks*262144],[64,file.length],[68,frames],[76,44100]])view.setUint32(at,value,true);
  let cursor=0;
  for(let tick=0;tick<records;tick++) {
    if(tick%20===0)cursor=streamOffset+Math.floor(tick/20)*262144;
    const phase=tick%ticks,first=phase===0,last=phase===ticks-1;
    const payload=new Uint8Array(first?1026:last?406:2);payload[0]=(first?2:0)|(last?1:0);
    if(last) {
      const value=Math.min(frames-1,Math.floor(tick/ticks)+1)%256;payload[1]=2;
      payload.set([profile?0:1,255],2);payload.fill(value,4,204);
      payload.set([profile?179:178,255],204);payload.fill(value,206,406);
    }
    view.setUint16(cursor,payload.length,true);file.set(payload,cursor+2);cursor+=payload.length+2;
  }
  checksum(header);return file;
}
test('all four profile headers validate geometry, source PCM16/8 and PAL/NTSC cadence',()=>{
  for(const profile of [1,2,3,4])for(const pcm8 of [false,true]) {
    const fixture=profileFixture(profile,pcm8),header=readMupHeader(fixture.subarray(0,256),fixture.length),hz=profile>2?NTSC_HZ:PAL_HZ;
    assert.equal(header.profile,profile);assert.equal(header.ticksPerFrame,profile>2?4:3);
    assert.equal(header.duration,header.recordCount/hz);assert.equal(header.fps,hz/header.ticksPerFrame);
    assert.equal(header.pixelAspect,profile>2?5/6:profile===2?15/16:1);
    assert.equal(header.sampleRate,44100);assert.equal(header.audioOffset,pcm8?221184:184320);
  }
});
test('full-width PAL and NTSC decode both edge columns across blocks, checkpoints and backward seeks',async()=>{
  for(const profile of [1,2,3,4]) {
    const preview=await MupPreview.open(new Blob([profileFixture(profile)]));
    for(const frame of [1,5,61,65,3,60,0,64]) {
      const rgba=await preview.frame(frame);
      assert.equal(preview.state.front[0],frame);assert.equal(preview.state.front[179*256],frame);
      assert.deepEqual([...rgba.subarray(0,3)],PALETTE[frame&15]);
      assert.deepEqual([...rgba.subarray(359*4,359*4+3)],PALETTE[frame>>4]);
    }
  }
});
test('profile headers reject mismatched cadence, legacy geometry, unknown IDs and invalid audio layout',()=>{
  for(const mutate of [header=>{header[20]=3;},header=>{header[80]=5;},header=>{header[14]=100;},header=>{header[81]=1;},header=>{header[36]=1;},header=>{header[32]=1;}]) {
    const fixture=profileFixture(4),header=fixture.subarray(0,256);mutate(header);checksum(header);
    assert.throws(()=>readMupHeader(header,fixture.length),/Unsupported|Invalid|Reserved/);
  }
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
