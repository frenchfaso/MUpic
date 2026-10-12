import {EncoderCore} from './wasm-core.mjs?v=8ae769a619a74291';
let core,frames=[];
self.onmessage = async ({data}) => {
  const {id,method,args={}}=data;
  try {
    let result,transfer=[];
    if(method==='init') { core=await EncoderCore.load(args.url); result={ready:true}; }
    else if(method==='lutIdentity')result=core.lutIdentity();
    else if(method==='lutInfo')result=core.lutInfo();
    else if(method==='beginLutAsset')core.beginLutAsset(new Uint8Array(args.header));
    else if(method==='appendLutAsset')core.appendLutAsset(new Uint8Array(args.chunk));
    else if(method==='finishLutAsset')core.finishLutAsset(args.expectedSha);
    else if(method==='abortLutAsset')core.abortLutAsset();
    else if(method==='prepare') {
      frames=args.frames.map(buffer=>new Uint8Array(buffer));
      if(frames.length<1||frames.length>3||frames.some(frame=>frame.length!==216000))
        throw new Error('A planning batch must contain one to three 360x200 RGB frames.');
      const rgb=new Uint8Array(frames.reduce((length,frame)=>length+frame.length,0));
      let offset=0;for(const frame of frames){rgb.set(frame,offset);offset+=frame.length;}
      result=core.prepareKeys(rgb).buffer;transfer=[result];
    }
    else if(method==='plan') {
      result=core.planKeys(new Uint32Array(args.keys)).buffer;
      transfer=[result];
    }
    else if(method==='pack') {
      core.importPlans(new Uint32Array(args.keys),new Uint32Array(args.plans));
      result=frames.map(frame=>core.quantize(frame).buffer);frames=[];transfer=result;
    }
    else if(method==='quantize') {
      result=core.quantize(new Uint8Array(args.rgb)).buffer;
      transfer=[result];
    } else throw new Error('Unknown quantizer request.');
    self.postMessage({id,result},transfer);
  } catch(error) { self.postMessage({id,error:error.message || String(error)}); }
};
