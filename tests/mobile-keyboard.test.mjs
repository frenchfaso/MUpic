import test from 'node:test';
import assert from 'node:assert/strict';
import {characterKeys,KeyPulseQueue,NativeKeyboardInput} from '../site/mobile-keyboard.mjs';

function clock() {
  let now=0,id=0;const timers=new Map();
  return {schedule(fn,delay){const key=++id;timers.set(key,{fn,at:now+delay});return key;},
    cancel(key){timers.delete(key);},get now(){return now;},get size(){return timers.size;},
    next(){const [key,timer]=[...timers].sort((a,b)=>a[1].at-b[1].at)[0]||[];
      if(!timer)return false;timers.delete(key);now=timer.at;timer.fn();return true;}};
}
function editable() {
  return {value:'',listeners:new Map(),addEventListener(type,callback){this.listeners.set(type,callback);},
    setSelectionRange(start,end){this.selectionStart=start;this.selectionEnd=end;},
    emit(type,values={}){const event={cancelable:true,preventDefault(){this.prevented=true;},...values};
      this.listeners.get(type)(event);return event;}};
}

test('native text maps to the actual C64 matrix, including punctuation and return',()=>{
  assert.deepEqual(characterKeys('a'),[10]);assert.deepEqual(characterKeys('Z'),[12]);
  assert.deepEqual(characterKeys('0'),[35]);assert.deepEqual(characterKeys('\n'),[1]);
  assert.deepEqual(characterKeys('\b'),[0]);assert.deepEqual(characterKeys(' '),[60]);
  assert.deepEqual(characterKeys('+'),[40]);assert.deepEqual(characterKeys('='),[53]);
  assert.deepEqual(characterKeys('?'),[15,55]);assert.deepEqual(characterKeys('"'),[15,59]);
  assert.equal(characterKeys('🙂'),null);
});
test('default timers keep the browser API receiver outside keyboard instances',()=>{
  const timing=clock(),originalSet=globalThis.setTimeout,originalClear=globalThis.clearTimeout;
  const receivers=[];
  const check=receiver=>{receivers.push(receiver);if(receiver!==undefined&&receiver!==globalThis)throw new TypeError('Illegal invocation');};
  globalThis.setTimeout=function(fn,delay){check(this);return timing.schedule(fn,delay);};
  globalThis.clearTimeout=function(id){check(this);return timing.cancel(id);};
  try {
    const pulses=new KeyPulseQueue({press(){},release(){}});pulses.text('A');pulses.reset();
    const input=editable(),native=new NativeKeyboardInput(input,{text(){}});
    input.emit('compositionstart');input.value+='A';input.emit('compositionend',{data:'A'});native.reset();
    native.suppressKey('Enter');native.reset();
    assert.equal(receivers.length,6);assert.equal(timing.size,0);
  } finally {globalThis.setTimeout=originalSet;globalThis.clearTimeout=originalClear;}
});
test('typed keys stay down for 60ms with a release gap and preserve their modifiers',()=>{
  const timing=clock(),events=[];
  const pulses=new KeyPulseQueue({...timing,press:keys=>events.push(['down',keys,timing.now]),
    release:keys=>events.push(['up',keys,timing.now])});
  pulses.text('?A');assert.deepEqual(events,[['down',[15,55],0]]);
  timing.next();assert.deepEqual(events.at(-1),['up',[15,55],60]);
  timing.next();assert.deepEqual(events.at(-1),['down',[10],80]);
  timing.next();assert.deepEqual(events.at(-1),['up',[10],140]);
  timing.next();assert.equal(timing.size,0);
});
test('reset releases the held chord and cancels queued input across worker generations',()=>{
  const timing=clock(),events=[];let active=true;
  const pulses=new KeyPulseQueue({...timing,active:()=>active,press:keys=>events.push(['down',keys]),release:keys=>events.push(['up',keys])});
  pulses.text('?abc');pulses.reset();active=false;
  assert.equal(timing.size,0);assert.deepEqual(events,[['down',[15,55]],['up',[15,55]]]);
  assert.equal(timing.next(),false);pulses.text('A');assert.equal(events.length,2);
});
test('beforeinput commits once and the input fallback handles text, return and backspace',()=>{
  const input=editable(),committed=[];
  new NativeKeyboardInput(input,{text:value=>committed.push(value)});
  const event=input.emit('beforeinput',{inputType:'insertText',data:'a'});
  assert.equal(event.prevented,true);input.emit('input');assert.deepEqual(committed,['a']);
  input.value+='bc';input.emit('input');assert.deepEqual(committed,['a','bc']);
  input.emit('beforeinput',{inputType:'insertLineBreak'});
  input.emit('beforeinput',{inputType:'deleteContentBackward'});
  assert.deepEqual(committed,['a','bc','\n','\b']);
  input.emit('beforeinput',{inputType:'deleteContentBackward',cancelable:false});input.value='';input.emit('input');
  assert.equal(committed.at(-1),'\b');assert.equal(input.selectionStart,1);
});
test('composition and its final input event commit one text sequence',()=>{
  const timing=clock(),input=editable(),committed=[];
  new NativeKeyboardInput(input,{...timing,text:value=>committed.push(value)});
  input.emit('compositionstart');input.value+='A';input.emit('input');
  input.emit('beforeinput',{inputType:'insertCompositionText',data:'A',isComposing:true});
  input.emit('compositionend',{data:'A'});input.emit('input');assert.deepEqual(committed,[]);
  timing.next();assert.deepEqual(committed,['A']);input.emit('input');assert.deepEqual(committed,['A']);
});
test('native return and delete edits cannot duplicate a handled keydown',()=>{
  const timing=clock(),input=editable(),committed=[];
  const native=new NativeKeyboardInput(input,{...timing,text:value=>committed.push(value)});
  native.suppressKey('Enter');input.emit('beforeinput',{inputType:'insertLineBreak'});
  input.value+='\n';input.emit('input');assert.deepEqual(committed,[]);
  timing.next();input.emit('beforeinput',{inputType:'insertLineBreak'});assert.deepEqual(committed,['\n']);
  native.suppressKey('Backspace');input.emit('beforeinput',{inputType:'deleteContentBackward'});
  input.value='';input.emit('input');assert.deepEqual(committed,['\n']);
  native.reset();assert.equal(timing.size,0);
  input.emit('beforeinput',{inputType:'deleteContentBackward'});assert.deepEqual(committed,['\n','\b']);
});
test('blur/reset cancels a pending composition and inactive input never reaches the C64',()=>{
  const timing=clock(),input=editable(),committed=[];let active=true;
  const native=new NativeKeyboardInput(input,{...timing,active:()=>active,text:value=>committed.push(value)});
  input.emit('compositionstart');input.value+='A';input.emit('compositionend',{data:'A'});
  native.reset();active=false;assert.equal(timing.size,0);
  input.emit('beforeinput',{inputType:'insertText',data:'B'});input.value+='C';input.emit('input');
  active=true;input.emit('compositionend',{data:'stale'});assert.equal(timing.size,0);
  assert.deepEqual(committed,[]);
});
