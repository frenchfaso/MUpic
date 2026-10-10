import {EncoderCore} from './wasm-core.mjs';
let core;
self.onmessage = async ({data}) => {
  const {id,method,args={}}=data;
  try {
    let result,transfer=[];
    if(method==='init') { core=await EncoderCore.load(args.url); result={ready:true}; }
    else if(method==='search') { result=core.searchData(); if(result) transfer=[result]; }
    else if(method==='quantize') {
      result=core.quantize(new Uint8Array(args.rgb),args.seeds?new Uint32Array(args.seeds):null).buffer;
      transfer=[result];
    } else throw new Error('Unknown quantizer request.');
    self.postMessage({id,result},transfer);
  } catch(error) { self.postMessage({id,error:error.message || String(error)}); }
};
