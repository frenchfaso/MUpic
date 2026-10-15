import {EncoderCore} from './wasm-core.mjs?v=523dcdea31bf18f8';
let core,frames=[],sharedLut;
self.onmessage = async ({data}) => {
  const {id,method,args={}}=data;
  try {
    let result,transfer=[];
    if(method==='init') {
      core=await EncoderCore.load(args.url,args.module);
      if(args.profile!==undefined)core.setProfile(args.profile);
      if(args.sharedLut)sharedLut=new Uint32Array(args.sharedLut);
      result={ready:true,module:core.module};
    }
    else if(method==='setProfile')core.setProfile(args.profile);
    else if(method==='videoPlan')result={profile:core.profileInfo(args.profile),fit:core.fit(args.profile,args.width,args.height)};
    else if(method==='audioInfo')result=core.audioInfo(args.audioFormat,args.sampleRate);
    else if(method==='shareLut') {
      const buffer=core.sharedLut();
      if(buffer)sharedLut=new Uint32Array(buffer);
      result=buffer;
    }
    else if(method==='packShared') {
      if(!sharedLut)throw new Error('The shared color table is unavailable.');
      const rgb=new Uint8Array(args.buffer,args.rgbOffset,216000);
      const output=new Uint8Array(args.buffer,args.packedOffset,36000);
      core.packShared(rgb,sharedLut,output);
    }
    else if(method==='quantizeBatch') {
      if(args.frames.length<1||args.frames.length>3||args.frames.some(buffer=>buffer.byteLength!==216000))
        throw new Error('A planning batch must contain one to three 360x200 RGB frames.');
      result=args.frames.map(buffer=>core.quantize(new Uint8Array(buffer)).buffer);transfer=result;
    }
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
