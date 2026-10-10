import {EncoderCore} from './wasm-core.mjs?v=a402dff20670e152';
let core;
self.onmessage = async ({data}) => {
  const {id,method,args={}}=data;
  try {
    let result={},transfer=[];
    if(method==='init') { core=await EncoderCore.load(args.url); result={ready:true}; }
    else if(method==='begin') core.begin(args.frameCount,new Uint8Array(args.pcm),args.title);
    else if(method==='frame') { result={chunks:core.frame(new Uint8Array(args.packed))}; transfer=result.chunks; }
    else if(method==='finish') { result=core.finish(); transfer=[result.prefix,...result.chunks]; }
    else throw new Error('Unknown encoder request.');
    self.postMessage({id,result},transfer);
  } catch(error) { self.postMessage({id,error:error.message || String(error)}); }
};
