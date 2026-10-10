import test from 'node:test';
import assert from 'node:assert/strict';
import {videoPlan,asciiTitle,interleavePcm,MAX_FRAMES} from '../site/media.mjs';
import {CONTENT_FPS} from '../site/mup-decoder.mjs';
test('16:9 fits the native SD active area without stretching',()=>{
  const plan=videoPlan(60,1920,1080);assert.equal(plan.width,356);assert.equal(plan.height,200);assert.equal(plan.left,2);assert.equal(plan.top,0);
});
test('portrait and4:3 sources retain aspect ratio and centered black bars',()=>{
  assert.deepEqual({...videoPlan(1,1080,1920),frameCount:undefined,duration:undefined,estimatedBytes:undefined},
    {frameCount:undefined,duration:undefined,estimatedBytes:undefined,width:113,height:200,left:123,top:0});
  const plan=videoPlan(1,640,480);assert.equal(plan.width,267);assert.equal(plan.height,200);assert.equal(plan.left,46);
});
test('player frame-count limits reject oversized or empty videos',()=>{
  assert.equal(videoPlan(MAX_FRAMES/CONTENT_FPS,356,200).frameCount,MAX_FRAMES);
  assert.throws(()=>videoPlan((MAX_FRAMES+1)/CONTENT_FPS,356,200),/21 minutes/);
  assert.throws(()=>videoPlan(0,356,200),/duration/);assert.throws(()=>videoPlan(0.03,356,200),/0.12/);
});
test('audio gain saturates signed16-bit samples and interleaves stereo',()=>{
  const pcm=interleavePcm(Float32Array.of(-1,0.5,2),Float32Array.of(1,-0.5,-2));
  const view=new DataView(pcm.buffer);assert.deepEqual(Array.from({length:6},(_,i)=>view.getInt16(i*2,true)),[-32768,32767,16384,-16384,32767,-32768]);
});
test('container titles are ASCII and fit the64-byte field',()=>{
  assert.equal(asciiTitle('Café 🌞.mp4'),'Cafe');assert.equal(asciiTitle('😀.webm'),'MY VIDEO');assert.equal(asciiTitle('x'.repeat(100)+'.mp4').length,63);
});
