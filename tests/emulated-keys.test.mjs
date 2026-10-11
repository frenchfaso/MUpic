import test from 'node:test';
import assert from 'node:assert/strict';
import {EmulatedKeys} from '../site/emulated-keys.mjs';
function fixture(){const calls=[];return {calls,keys:new EmulatedKeys((index,down)=>calls.push([index,down]))};}
test('a complete host tap between CPU batches lasts three emulated PAL frames',()=>{
  const {keys,calls}=fixture();keys.set(7,true,100);keys.set(7,false,100);
  keys.advance(159);assert.deepEqual(calls,[[7,true]]);
  keys.advance(160);assert.deepEqual(calls,[[7,true],[7,false]]);
});
test('two rapid taps keep a release interval so the menu sees both edges',()=>{
  const {keys,calls}=fixture();
  keys.set(7,true,0);keys.set(7,false,0);keys.set(7,true,0);keys.set(7,false,0);
  for(const t of [60,79,80,139,140])keys.advance(t);
  assert.deepEqual(calls,[[7,true],[7,false],[7,true],[7,false]]);
});
test('a CPU stall cannot collapse queued key transitions into a single batch',()=>{
  const {keys,calls}=fixture();
  keys.set(7,true,0);keys.set(7,false,0);keys.set(7,true,0);keys.set(7,false,0);
  keys.advance(1000);assert.deepEqual(calls,[[7,true],[7,false]]);
  keys.advance(1019);assert.equal(calls.length,2);
  keys.advance(1020);keys.advance(1080);
  assert.deepEqual(calls,[[7,true],[7,false],[7,true],[7,false]]);
});
test('modifier chords remain simultaneous and duplicate downs do not repeat',()=>{
  const {keys,calls}=fixture();keys.set(15,true,0);keys.set(7,true,0);keys.set(7,true,0);
  keys.set(7,false,10);keys.set(15,false,10);keys.advance(60);
  assert.deepEqual(calls,[[15,true],[7,true],[15,false],[7,false]]);
});
test('focus loss/reset immediately releases keys and discards deferred input',()=>{
  const {keys,calls}=fixture();keys.set(7,true,0);keys.set(7,false,0);keys.set(7,true,0);
  keys.reset();keys.advance(1000);assert.deepEqual(calls,[[7,true],[7,false]]);
  keys.set(7,true,1000);assert.deepEqual(calls.at(-1),[7,true]);
});
