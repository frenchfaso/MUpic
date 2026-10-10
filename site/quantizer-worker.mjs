import {EncoderCore} from './wasm-core.mjs?v=a402dff20670e152';
let core,frames=[];
self.onmessage = async ({data}) => {
  const {id,method,args={}}=data;
  try {
    let result,transfer=[];
    if(method==='init') { core=await EncoderCore.load(args.url); result={ready:true}; }
    else if(method==='search') { result=core.searchData(); if(result) transfer=[result]; }
    else if(method==='prepare') {
      frames=args.frames.map(buffer=>new Uint8Array(buffer));
      if(frames.length<1||frames.length>3||frames.some(frame=>frame.length!==216000))
        throw new Error('A planning batch must contain one to three 360x200 RGB frames.');
      const rgb=new Uint8Array(frames.reduce((length,frame)=>length+frame.length,0));
      let offset=0;for(const frame of frames){rgb.set(frame,offset);offset+=frame.length;}
      result=core.prepareKeys(rgb).buffer;transfer=[result];
    }
    else if(method==='plan') {
      result=core.planKeys(new Uint32Array(args.keys),args.seeds?new Uint32Array(args.seeds):null).buffer;
      transfer=[result];
    }
    else if(method==='pack') {
      core.importPlans(new Uint32Array(args.keys),new Uint32Array(args.plans));
      result=frames.map(frame=>core.quantize(frame).buffer);frames=[];transfer=result;
    }
    else if(method==='quantize') {
      result=core.quantize(new Uint8Array(args.rgb),args.seeds?new Uint32Array(args.seeds):null).buffer;
      transfer=[result];
    } else throw new Error('Unknown quantizer request.');
    self.postMessage({id,result},transfer);
  } catch(error) { self.postMessage({id,error:error.message || String(error)}); }
};
