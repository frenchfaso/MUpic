import {EncoderCore} from './wasm-core.mjs?v=523dcdea31bf18f8';
let core;
self.onmessage = async ({data}) => {
  const {id,method,args={}}=data;
  try {
    let result={},transfer=[];
    if(method==='init') { core=await EncoderCore.load(args.url,args.module); result={ready:true}; }
    else if(method==='begin') core.begin(args.frameCount,new Uint8Array(args.pcm),args.title,{audioFormat:args.audioFormat,sampleRate:args.sampleRate,profile:args.profile});
    else if(method==='frame') {
      result={chunks:core.frame(args.packed instanceof Uint8Array?args.packed:new Uint8Array(args.packed))};transfer=result.chunks;
    }
    else if(method==='finish') { result=core.finish(); transfer=[result.prefix,...result.chunks]; }
    else throw new Error('Unknown encoder request.');
    self.postMessage({id,result},transfer);
  } catch(error) { self.postMessage({id,error:error.message || String(error)}); }
};
