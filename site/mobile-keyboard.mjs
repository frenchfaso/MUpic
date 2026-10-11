const LETTERS = {A:10,B:28,C:20,D:18,E:14,F:21,G:26,H:29,I:33,J:34,K:37,L:42,
  M:36,N:39,O:38,P:41,Q:62,R:17,S:13,T:22,U:30,V:31,W:9,X:23,Y:25,Z:12};
const DIGITS = [35,56,59,8,11,16,19,24,27,32];
const SYMBOLS = {' ':60,'\n':1,'\r':1,'\b':0,'+':40,'-':43,'.':44,':':45,
  '@':46,',':47,'£':48,'*':49,';':50,'=':53,'/':55};
const SHIFTED = {'!':56,'"':59,'#':8,'$':11,'%':16,'&':19,"'":24,'(':27,')':32,
  '<':47,'>':44,'?':55,'[':45,']':50};

/** Actual C64 matrix keys, independent of a soft keyboard's missing event.code. */
export function characterKeys(character) {
  const letter=LETTERS[character.toUpperCase()];
  if(letter!==undefined)return [letter];
  if(/^[0-9]$/.test(character))return [DIGITS[Number(character)]];
  if(SYMBOLS[character]!==undefined)return [SYMBOLS[character]];
  if(SHIFTED[character]!==undefined)return [15,SHIFTED[character]];
  return null;
}

/** Hold typed keys for three PAL frames; clear every timer on focus/reset. */
export class KeyPulseQueue {
  constructor({press,release,active=()=>true,schedule=(fn,delay)=>setTimeout(fn,delay),cancel=id=>clearTimeout(id)}) {
    Object.assign(this,{press,release,active,schedule,cancel});this.queue=[];this.held=null;this.timer=null;
  }
  add(keys) {
    if(!keys||!this.active()||this.queue.length>=128)return;
    this.queue.push([...keys]);if(this.timer===null)this.next();
  }
  text(value) {for(const character of String(value).replace(/\r\n/g,'\n'))this.add(characterKeys(character));}
  next() {
    this.timer=null;
    if(!this.active()){this.reset();return;}
    if(!this.queue.length)return;
    this.held=this.queue.shift();this.press(this.held);
    this.timer=this.schedule(()=>{
      this.release(this.held);this.held=null;
      this.timer=this.schedule(()=>this.next(),20);
    },60);
  }
  reset() {
    if(this.timer!==null)this.cancel(this.timer);this.timer=null;this.queue.length=0;
    if(this.held){this.release(this.held);this.held=null;}
  }
}

const SENTINEL='\u200b';
/** Use native editable input; composition commits once after its final input event. */
export class NativeKeyboardInput {
  constructor(input,{text,active=()=>true,schedule=(fn,delay)=>setTimeout(fn,delay),cancel=id=>clearTimeout(id)}) {
    Object.assign(this,{input,text,active,schedule,cancel});this.composing=false;this.compositionTimer=null;
    this.suppressedEdit=null;this.suppressionTimer=null;
    input.addEventListener('beforeinput',event=>this.beforeInput(event));
    input.addEventListener('input',()=>this.readInput());
    input.addEventListener('compositionstart',()=>{this.composing=true;});
    input.addEventListener('compositionend',event=>{
      if(!this.composing)return;
      this.composing=false;
      if(this.compositionTimer!==null)this.cancel(this.compositionTimer);
      this.compositionTimer=this.schedule(()=>{
        this.compositionTimer=null;
        const value=this.value()||event.data||'';
        if(this.active())this.text(value);this.clear();
      },0);
    });
    this.clear();
  }
  value() {return this.input.value.replaceAll(SENTINEL,'');}
  clear() {this.input.value=SENTINEL;this.input.setSelectionRange(1,1);}
  suppressKey(code) {
    const kind=['Enter','NumpadEnter'].includes(code)?'return':['Backspace','Delete'].includes(code)?'delete':null;
    if(!kind)return;
    if(this.suppressionTimer!==null)this.cancel(this.suppressionTimer);
    this.suppressedEdit=kind;
    this.suppressionTimer=this.schedule(()=>{this.suppressionTimer=null;this.suppressedEdit=null;},0);
  }
  beforeInput(event) {
    if(!this.active()){if(event.cancelable)event.preventDefault();this.reset();return;}
    if(this.composing||event.isComposing||this.compositionTimer!==null)return;
    let value;
    if(event.inputType==='deleteContentBackward'||event.inputType==='deleteContentForward')value='\b';
    else if(event.inputType==='insertLineBreak'||event.inputType==='insertParagraph')value='\n';
    else if(event.inputType?.startsWith('insert'))value=event.data??event.dataTransfer?.getData('text/plain');
    if(value===undefined||value===null)return;
    const kind=value==='\b'?'delete':value==='\n'||value==='\r'?'return':null;
    if(kind&&kind===this.suppressedEdit){if(event.cancelable){event.preventDefault();this.clear();}return;}
    if(!event.cancelable)return;
    event.preventDefault();this.text(value);this.clear();
  }
  readInput() {
    if(this.composing||this.compositionTimer!==null)return;
    if(this.active()) {
      const value=this.value();
      const duplicate=this.suppressedEdit==='return'&&/^(\r\n|\r|\n)$/.test(value)||
        this.suppressedEdit==='delete'&&!value&&this.input.value!==SENTINEL;
      if(!duplicate) {
        if(value)this.text(value);
        else if(this.input.value!==SENTINEL)this.text('\b');
      }
    }
    this.clear();
  }
  reset() {
    if(this.compositionTimer!==null)this.cancel(this.compositionTimer);
    if(this.suppressionTimer!==null)this.cancel(this.suppressionTimer);
    this.suppressionTimer=null;this.suppressedEdit=null;
    this.compositionTimer=null;this.composing=false;this.clear();
  }
}
