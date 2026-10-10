// SPDX-License-Identifier: GPL-3.0-only
// WebGPU transport for the exact native Commodore 1702 Guest PAL preset.
// The offline Rust exporter compiles GLSL to WGSL. This module schedules passes,
// binds the native reflected uniforms, and advances history on complete VIC frames.
const IDENTITY=[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1];
const FINAL=[2,0,0,0,0,2,0,0,0,0,0,0,-1,-1,0,1];
const BLIT=`
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vs(@builtin(vertex_index) id:u32)->Vertex {
  let uv=vec2<f32>(f32(i32(id)/2)*2.0,f32(i32(id)&1)*2.0);
  return Vertex(vec4<f32>(uv.x*2.0-1.0,1.0-uv.y*2.0,0.0,1.0),uv);
}
@group(0) @binding(0) var image:texture_2d<f32>;
@group(0) @binding(1) var image_sampler:sampler;
@fragment fn fs(v:Vertex)->@location(0) vec4<f32> { return textureSample(image,image_sampler,v.uv); }
`;
const VERTICES=new Float32Array([
  -1,-1,0,1,0,0,-1,1,0,1,0,1,1,-1,0,1,1,0,1,1,0,1,1,1,
  0,0,0,1,0,0,0,1,0,1,0,1,1,0,0,1,1,0,1,1,0,1,1,1,
]);
async function checkedBytes(url,asset) {
  const response=await fetch(url,{cache:"no-cache"});
  if (!response.ok) throw new Error(`CRT asset ${url}: HTTP ${response.status}.`);
  const data=await response.arrayBuffer();
  if (data.byteLength!==asset.bytes) throw new Error(`CRT asset size mismatch: ${url}.`);
  const hash=Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256",data)),n=>n.toString(16).padStart(2,"0")).join("");
  if (hash!==asset.sha256) throw new Error(`CRT asset hash mismatch: ${url}. Reload after staging the build.`);
  return data;
}
function imageSize(image) { return image.size; }
function cachedGroup(groups,resources) {
  for (const group of groups) {
    let match=true;
    for (let index=0;index<resources.length;index++) if (group.resources[index]!==resources[index]) { match=false; break; }
    if (match) return group.binding;
  }
  return null;
}
function scale(axis,source,viewport,original) {
  const input=axis.type==="Input"?source:axis.type==="Viewport"?viewport:axis.type==="Original"?original:1;
  return Math.max(1,Math.min(16384,Math.round(input*axis.factor)));
}
function stage(mask) { return (mask&1?GPUShaderStage.VERTEX:0)|(mask&2?GPUShaderStage.FRAGMENT:0); }

class Guest1702 {
  constructor(device,format) {
    this.device=device; this.format=format; this.outputs=[]; this.feedback=[]; this.history=[]; this.luts=[];
    this.samplers=new Map(); this.blitPipelines=new Map(); this.frameCount=0; this.resetPending=true; this.wasEnabled=false;
    this.source=null; this.shape="";
    this.quad=device.createBuffer({size:VERTICES.byteLength,usage:GPUBufferUsage.VERTEX|GPUBufferUsage.COPY_DST});
    device.queue.writeBuffer(this.quad,0,VERTICES);
    this.blitShader=device.createShaderModule({code:BLIT,label:"Native librashader mipmap/blit"});
  }
  sampler(filter="Linear",wrap="ClampToBorder",mip=filter) {
    const key=`${filter}/${wrap}/${mip}`;
    if (!this.samplers.has(key)) {
      const address=wrap==="Repeat"?"repeat":wrap==="MirroredRepeat"?"mirror-repeat":"clamp-to-edge";
      // Generated WGSL implements the native transparent-border footprint.
      this.samplers.set(key,this.device.createSampler({addressModeU:address,addressModeV:address,addressModeW:address,
        magFilter:filter.toLowerCase(),minFilter:filter.toLowerCase(),mipmapFilter:mip.toLowerCase(),lodMinClamp:0,lodMaxClamp:1000}));
    }
    return this.samplers.get(key);
  }
  image(width,height,format="rgba8unorm",mipmap=false,label="Guest framebuffer") {
    const levels=mipmap?Math.floor(Math.log2(Math.max(width,height)))+1:1;
    const texture=this.device.createTexture({label,size:[width,height],format,mipLevelCount:levels,
      usage:GPUTextureUsage.TEXTURE_BINDING|GPUTextureUsage.RENDER_ATTACHMENT|GPUTextureUsage.COPY_SRC|GPUTextureUsage.COPY_DST});
    const views=Array.from({length:levels},(_,level)=>texture.createView({baseMipLevel:level,mipLevelCount:1}));
    return {texture,view:texture.createView(),views,width,height,format,levels,size:[width,height,1/width,1/height]};
  }
  async load(contract) {
    const url=new URL(contract.url,document.baseURI), raw=await checkedBytes(url,contract);
    const manifest=JSON.parse(new TextDecoder().decode(raw));
    if (manifest.version!==1||manifest.kind!=="guest1702"||manifest.passes.length!==19) throw new Error("Unsupported native CRT manifest.");
    this.manifest=manifest; this.parameters={...manifest.parameters};
    const [shaders,lutData]=await Promise.all([
      Promise.all(manifest.passes.map(async pass=>({
        vertex:new TextDecoder().decode(await checkedBytes(new URL(pass.vertex.file,url),pass.vertex)),
        fragment:new TextDecoder().decode(await checkedBytes(new URL(pass.fragment.file,url),pass.fragment)),
      }))),Promise.all(manifest.luts.map(lut=>checkedBytes(new URL(lut.asset.file,url),lut.asset))),
    ]);
    this.passes=await Promise.all(manifest.passes.map(async (meta,index)=>{
      const vertex=this.device.createShaderModule({code:shaders[index].vertex,label:`Guest ${index} vertex`});
      const fragment=this.device.createShaderModule({code:shaders[index].fragment,label:`Guest ${index} fragment`});
      const information=await Promise.all([vertex.getCompilationInfo(),fragment.getCompilationInfo()]);
      const errors=information.flatMap(info=>info.messages.filter(m=>m.type==="error"));
      if (errors.length) throw new Error(`Guest PAL pass ${index}: ${errors.map(m=>m.message).join("; ")}`);
      const main=[],samplers=[],uniforms={};
      for (const block of ["ubo","push"]) {
        const config=meta[block]; if (!config||!config.stages||config.binding===null) continue;
        const data=new ArrayBuffer(config.size),buffer=this.device.createBuffer({size:config.size,usage:GPUBufferUsage.UNIFORM|GPUBufferUsage.COPY_DST,label:`Guest ${index} ${block}`});
        uniforms[block]={data,view:new DataView(data),buffer,config,dirty:true};
        main.push({binding:config.binding,visibility:stage(config.stages),buffer:{type:"uniform",minBindingSize:config.size}});
      }
      for (const input of meta.textures) {
        main.push({binding:input.binding,visibility:GPUShaderStage.FRAGMENT,texture:{sampleType:"float",viewDimension:"2d",multisampled:false}});
        samplers.push({binding:input.binding,visibility:GPUShaderStage.FRAGMENT,sampler:{type:"filtering"}});
      }
      main.push({binding:meta.borderBinding,visibility:GPUShaderStage.FRAGMENT,texture:{sampleType:"float",viewDimension:"2d",multisampled:false}});
      samplers.push({binding:meta.borderBinding,visibility:GPUShaderStage.FRAGMENT,sampler:{type:"filtering"}});
      const mainLayout=this.device.createBindGroupLayout({entries:main,label:`Guest ${index} textures/uniforms`});
      const samplerLayout=this.device.createBindGroupLayout({entries:samplers,label:`Guest ${index} samplers`});
      const layout=this.device.createPipelineLayout({bindGroupLayouts:[mainLayout,samplerLayout]});
      const pipeline=await this.device.createRenderPipelineAsync({label:`Guest PAL pass ${index}`,layout,
        vertex:{module:vertex,entryPoint:meta.vertexEntry,buffers:[{arrayStride:24,stepMode:"vertex",attributes:[
          {shaderLocation:0,offset:0,format:"float32x4"},{shaderLocation:1,offset:16,format:"float32x2"}]}]},
        fragment:{module:fragment,entryPoint:meta.fragmentEntry,targets:[{format:index===18?this.format:meta.format}]},
        primitive:{topology:"triangle-strip",frontFace:"ccw",cullMode:"none"}});
      return {meta,pipeline,mainLayout,samplerLayout,uniforms,uniformBlocks:Object.values(uniforms),
        mainGroups:[],samplerGroups:[],textureViews:new Array(meta.textures.length),samplerRefs:new Array(meta.textures.length+1)};
    }));
    // Black border with a white centre reproduces the hardware sampler's own
    // fractional precision, including corners, without software quantization.
    this.borderTexture=this.device.createTexture({label:"Native transparent-border coverage",size:[3,3],format:"r16float",usage:GPUTextureUsage.TEXTURE_BINDING|GPUTextureUsage.COPY_DST});
    this.borderView=this.borderTexture.createView();
    this.device.queue.writeTexture({texture:this.borderTexture},new Uint16Array([0,0,0,0,0x3c00,0,0,0,0]),{bytesPerRow:6},[3,3]);
    const encoder=this.device.createCommandEncoder({label:"Guest LUT initialization"});
    for (let index=0;index<manifest.luts.length;index++) {
      const meta=manifest.luts[index],image=this.image(meta.width,meta.height,"rgba8unorm",meta.mipmap,meta.name);
      image.filter=meta.filter; image.wrap=meta.wrap;
      this.device.queue.writeTexture({texture:image.texture},lutData[index],{bytesPerRow:meta.width*4},[meta.width,meta.height]);
      if (meta.mipmap) this.mipmaps(encoder,image,this.sampler(meta.filter,"ClampToEdge",meta.filter));
      this.luts[index]=image;
    }
    this.device.queue.submit([encoder.finish()]); await this.device.queue.onSubmittedWorkDone();
    this.feedbackIndices=new Set(manifest.passes.flatMap(p=>p.textures.filter(t=>t.texture.semantic==="PassFeedback").map(t=>t.texture.index)));
    // The final pass renders into the canvas. Keep an extra framebuffer only
    // when the reflected graph actually samples its output or feedback.
    this.needsLastOutput=this.feedbackIndices.has(18)||manifest.passes.some(pass=>pass.textures.some(
      binding=>binding.texture.semantic==="PassOutput"&&binding.texture.index===18));
    // librashader OriginalHistory0 aliases the current original image; stored
    // previous inputs begin at OriginalHistory1, unlike PassFeedback0.
    this.historyCount=Math.max(0,...manifest.passes.flatMap(p=>p.textures.filter(t=>t.texture.semantic==="OriginalHistory").map(t=>t.texture.index)));
  }
  blitPipeline(format) {
    if (!this.blitPipelines.has(format)) this.blitPipelines.set(format,this.device.createRenderPipeline({layout:"auto",
      vertex:{module:this.blitShader,entryPoint:"vs"},fragment:{module:this.blitShader,entryPoint:"fs",targets:[{format}]},primitive:{topology:"triangle-list"}}));
    return this.blitPipelines.get(format);
  }
  blit(encoder,source,view,format,sampler) {
    const pipeline=this.blitPipeline(format),bindings=this.device.createBindGroup({layout:pipeline.getBindGroupLayout(0),entries:[{binding:0,resource:source},{binding:1,resource:sampler}]});
    const pass=encoder.beginRenderPass({colorAttachments:[{view,loadOp:"clear",storeOp:"store",clearValue:{r:0,g:0,b:0,a:0}}]});
    pass.setPipeline(pipeline); pass.setBindGroup(0,bindings); pass.draw(3); pass.end();
  }
  mipmaps(encoder,image,sampler=this.sampler("Linear","ClampToEdge","Nearest")) {
    for (let level=1;level<image.levels;level++) this.blit(encoder,image.views[level-1],image.views[level],image.format,sampler);
  }
  allocate(width,height,outputWidth,outputHeight) {
    const shape=`${width}/${height}/${outputWidth}/${outputHeight}`; if (shape===this.shape) return;
    this.shape=shape;
    this.viewportSize=[outputWidth,outputHeight,1/outputWidth,1/outputHeight];
    for (const pass of this.passes) { pass.mainGroups.length=0; pass.samplerGroups.length=0; }
    for (const image of [...this.outputs,...this.feedback,...this.history,this.source]) image?.texture.destroy();
    this.outputs=[]; this.feedback=[]; this.history=[];
    this.source=this.image(width,height,"rgba8unorm",false,"C64 VIC frame");
    this.source.filter=this.passes[0].meta.filter; this.source.wrap=this.passes[0].meta.wrap;
    let w=width,h=height;
    for (let index=0;index<19;index++) {
      const meta=this.passes[index].meta;
      w=scale(meta.scaling.x,w,outputWidth,width); h=scale(meta.scaling.y,h,outputHeight,height);
      if (index===18&&!this.needsLastOutput) continue;
      // Match the pinned native librashader 0.12.0 allocation: its framebuffer
      // pool starts at max_miplevels=1 and retains that cap when scaling, even
      // for mipmap_input passes. Preserve the preset flag in the manifest;
      // allocating a full pyramid here would change native shader sampling.
      const mipmap=false;
      const image=this.image(w,h,meta.format,mipmap,`Guest ${index} output`);
      image.filter=meta.filter; image.wrap=meta.wrap; this.outputs[index]=image;
      if (this.feedbackIndices.has(index)) {
        const feedback=this.image(w,h,meta.format,mipmap,`Guest ${index} feedback`);
        feedback.filter=meta.filter; feedback.wrap=meta.wrap; this.feedback[index]=feedback;
      }
    }
    for (let index=0;index<this.historyCount;index++) {
      const image=this.image(width,height,"rgba8unorm",false,`Guest VIC history ${index}`);
      image.filter=this.source.filter; image.wrap=this.source.wrap; this.history.push(image);
    }
    this.resetPending=true;
  }
  reset(encoder) {
    for (const pass of this.passes) { pass.mainGroups.length=0; pass.samplerGroups.length=0; }
    for (const image of [...this.outputs,...this.feedback,...this.history]) {
      if (image) for (const view of image.views) encoder.beginRenderPass({colorAttachments:[{view,clearValue:{r:0,g:0,b:0,a:0},loadOp:"clear",storeOp:"store"}]}).end();
    }
    this.frameCount=0; this.resetPending=false;
  }
  texture(semantic,source) {
    switch (semantic.semantic) {
      case "Original":return this.source; case "Source":return source;
      case "OriginalHistory":return semantic.index===0?this.source:this.history[semantic.index-1]; case "PassOutput":return this.outputs[semantic.index];
      case "PassFeedback":return this.feedback[semantic.index]; case "User":return this.luts[semantic.index];
      default:throw new Error(`Unknown Guest texture semantic: ${semantic.semantic}.`);
    }
  }
  uniform(pass,uniform,value,kind="float") {
    const multiple=Array.isArray(value),length=multiple?value.length:1;
    for (const block of ["ubo","push"]) {
      const offset=uniform.offset[block]; if (offset===null||offset===undefined) continue;
      const storage=pass.uniforms[block]; if (!storage) throw new Error("Missing native CRT uniform storage.");
      for (let index=0;index<length;index++) {
        const at=offset+index*4,before=storage.view.getUint32(at,true),item=multiple?value[index]:value;
        if (kind==="uint") storage.view.setUint32(at,item,true);
        else storage.view.setFloat32(at,item,true);
        if (before!==storage.view.getUint32(at,true)) storage.dirty=true;
      }
    }
  }
  render(encoder,frame,outputWidth,outputHeight,target,enabled) {
    this.allocate(frame.width,frame.height,outputWidth,outputHeight);
    this.device.queue.writeTexture({texture:this.source.texture},frame.buffer,{bytesPerRow:frame.width*4},[frame.width,frame.height]);
    if (!enabled) { this.blit(encoder,this.source.view,target,this.format,this.sampler("Nearest","ClampToEdge")); this.wasEnabled=false; return; }
    if (!this.wasEnabled||this.resetPending) this.reset(encoder);
    this.parameters.masksize=Math.max(1,Math.min(2,Math.round(outputWidth/1440)));
    for (const index of this.feedbackIndices) [this.outputs[index],this.feedback[index]]=[this.feedback[index],this.outputs[index]];
    let source=this.source;
    for (let index=0;index<19;index++) {
      const pass=this.passes[index],meta=pass.meta,last=index===18,output=this.outputs[index];
      for (const uniform of meta.uniforms) {
        if (uniform.kind==="Parameter") this.uniform(pass,uniform,this.parameters[uniform.name]);
        else if (uniform.kind==="TextureSize") this.uniform(pass,uniform,imageSize(this.texture(uniform.texture,source)));
        else if (uniform.semantic==="MVP") this.uniform(pass,uniform,last?FINAL:IDENTITY);
        else if (uniform.semantic==="FrameCount") this.uniform(pass,uniform,meta.frameCountMod?this.frameCount%meta.frameCountMod:this.frameCount,"uint");
        else if (uniform.semantic==="Output") this.uniform(pass,uniform,last?this.viewportSize:imageSize(output));
        else throw new Error(`Unsupported native CRT uniform: ${uniform.semantic}.`);
      }
      for (const storage of pass.uniformBlocks) if (storage.dirty) {
        this.device.queue.writeBuffer(storage.buffer,0,storage.data); storage.dirty=false;
      }
      // Resource identities remain valid when uniforms change. Feedback swaps
      // choose another cached group; a resize/reset releases every old group.
      for (let bindingIndex=0;bindingIndex<meta.textures.length;bindingIndex++) {
        const binding=meta.textures[bindingIndex];
        const image=this.texture(binding.texture,source); if (!image) throw new Error(`Guest ${index}: absent ${binding.texture.semantic}${binding.texture.index}.`);
        pass.textureViews[bindingIndex]=image.view;
        pass.samplerRefs[bindingIndex]=binding.texture.semantic==="Source"?this.sampler(meta.filter,meta.wrap):this.sampler(image.filter,image.wrap);
      }
      pass.samplerRefs[meta.textures.length]=this.sampler("Linear","ClampToEdge");
      let mainGroup=cachedGroup(pass.mainGroups,pass.textureViews);
      if (!mainGroup) {
        const entries=pass.uniformBlocks.map(storage=>({binding:storage.config.binding,resource:{buffer:storage.buffer}}));
        for (let bindingIndex=0;bindingIndex<meta.textures.length;bindingIndex++) entries.push({binding:meta.textures[bindingIndex].binding,resource:pass.textureViews[bindingIndex]});
        entries.push({binding:meta.borderBinding,resource:this.borderView});
        mainGroup=this.device.createBindGroup({layout:pass.mainLayout,entries});
        pass.mainGroups.push({resources:pass.textureViews.slice(),binding:mainGroup});
      }
      let samplerGroup=cachedGroup(pass.samplerGroups,pass.samplerRefs);
      if (!samplerGroup) {
        const entries=meta.textures.map((binding,bindingIndex)=>({binding:binding.binding,resource:pass.samplerRefs[bindingIndex]}));
        entries.push({binding:meta.borderBinding,resource:pass.samplerRefs[meta.textures.length]});
        samplerGroup=this.device.createBindGroup({layout:pass.samplerLayout,entries});
        pass.samplerGroups.push({resources:pass.samplerRefs.slice(),binding:samplerGroup});
      }
      const rendering=encoder.beginRenderPass({label:`Guest PAL ${index}`,colorAttachments:[{view:last?target:output.views[0],loadOp:"clear",storeOp:"store",clearValue:{r:0,g:0,b:0,a:0}}]});
      rendering.setPipeline(pass.pipeline); rendering.setBindGroup(0,mainGroup); rendering.setBindGroup(1,samplerGroup);
      rendering.setVertexBuffer(0,this.quad); rendering.draw(4,1,last?4:0); rendering.end();
      if (!last&&output.levels>1) this.mipmaps(encoder,output); source=output;
    }
    if (this.history.length) { const back=this.history.pop(); encoder.copyTextureToTexture({texture:this.source.texture},{texture:back.texture},[frame.width,frame.height]); this.history.unshift(back); }
    this.frameCount=(this.frameCount+1)>>>0; this.wasEnabled=true;
  }
}

export class CrtPresenter {
  static async create(canvas,onFailure) {
    if (!navigator.gpu) throw new Error("WebGPU is unavailable. Use a WebGPU browser on HTTPS or localhost.");
    const adapter=await navigator.gpu.requestAdapter({powerPreference:"high-performance"}); if (!adapter) throw new Error("No WebGPU adapter is available.");
    const device=await adapter.requestDevice({requiredFeatures:adapter.features.has("float32-filterable")?["float32-filterable"]:[]});
    const context=canvas.getContext("webgpu"); if (!context) throw new Error("Unable to create the WebGPU surface.");
    const presenter=new CrtPresenter(canvas,device,context,onFailure);
    device.lost.then(info=>onFailure(`WebGPU device lost (${info.message||info.reason}).`));
    device.addEventListener("uncapturederror",event=>onFailure(`WebGPU: ${event.error.message}`)); return presenter;
  }
  constructor(canvas,device,context,onFailure) {
    this.canvas=canvas; this.device=device; this.context=context; this.format=navigator.gpu.getPreferredCanvasFormat();
    context.configure({device,format:this.format,alphaMode:"opaque"});
    this.params=new Float32Array(8); this.enabled=true; this.ready=false; this.shader=null;
    this.frames=0; this.completedFrames=0; this.inFlight=0; this.maxInFlight=0; this.measurement=null; this.submitCpuMs=0;
    this.failure=onFailure; this.lastFrameId=null; this.lastEmuMs=null;
  }
  async configure(shader,defaults) {
    if (defaults.length!==8) throw new Error("Invalid Rust CRT parameters."); this.params.set(defaults);
    if (this.ready&&shader===this.shader) { this.chain.resetPending=true; this.lastFrameId=null; this.lastEmuMs=null; return; }
    const contract=JSON.parse(shader); if (contract.kind!=="guest1702"||contract.version!==1) throw new Error("Invalid Rust native CRT contract.");
    const chain=new Guest1702(this.device,this.format); await chain.load(contract); this.chain=chain; this.shader=shader; this.ready=true;
  }
  resetHistory() { if (this.chain) this.chain.resetPending=true; this.lastFrameId=null; this.lastEmuMs=null; }
  draw(frame) {
    if (!this.ready||this.inFlight>=2) return false;
    const started=performance.now(),bounds=this.canvas.getBoundingClientRect();
    // The phosphor mask is an output-pixel effect: render at physical resolution.
    // A second nearest-neighbour CSS enlargement produces broad coloured bands.
    const factor=Math.min(window.devicePixelRatio||1,this.device.limits.maxTextureDimension2D/Math.max(1,bounds.width,bounds.height));
    const width=Math.max(1,Math.round(bounds.width*factor)),height=Math.max(1,Math.round(bounds.height*factor));
    if (this.canvas.width!==width||this.canvas.height!==height) { this.canvas.width=width; this.canvas.height=height; }
    if ((Number.isFinite(frame.frameId)&&Number.isFinite(this.lastFrameId)&&frame.frameId<this.lastFrameId)||(Number.isFinite(frame.emuMs)&&Number.isFinite(this.lastEmuMs)&&frame.emuMs<this.lastEmuMs)) this.chain.resetPending=true;
    this.lastFrameId=frame.frameId; this.lastEmuMs=frame.emuMs;
    const encoder=this.device.createCommandEncoder({label:"Commodore 1702 Guest PAL frame"});
    this.chain.render(encoder,frame,width,height,this.context.getCurrentTexture().createView(),this.enabled);
    this.device.queue.submit([encoder.finish()]); this.inFlight++; this.maxInFlight=Math.max(this.maxInFlight,this.inFlight);
    if (this.measurement) this.measurement.maxInFlight=Math.max(this.measurement.maxInFlight,this.inFlight);
    this.device.queue.onSubmittedWorkDone().then(()=>{
      this.inFlight--; this.completedFrames++;
      if (this.measurement) { const at=performance.now(); this.measurement.maxGapMs=Math.max(this.measurement.maxGapMs,at-this.measurement.lastCompletedAt); this.measurement.lastCompletedAt=at; }
    }).catch(error=>{this.inFlight--; this.failure(`WebGPU: ${error.message||error}`);});
    this.frames++; this.submitCpuMs+=performance.now()-started; return true;
  }
  beginMeasurement() { this.measurement={lastCompletedAt:performance.now(),maxGapMs:0,maxInFlight:this.inFlight}; }
  measurementGapMs() { return this.measurement?Math.max(this.measurement.maxGapMs,performance.now()-this.measurement.lastCompletedAt):0; }
}
