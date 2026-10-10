/** Count completed work, independently of the ordered MUP writer. */
export class ConversionProgress {
  constructor(totalFrames) {
    if(!Number.isInteger(totalFrames)||totalFrames<1)throw new Error('Invalid conversion frame count.');
    this.totalFrames=totalFrames;this.quantized=new Set();this.encodedFrames=0;this.phase='preparing';
  }
  start() {this.phase='processing';return this.snapshot();}
  quantizedFrame(index) {
    if(!Number.isInteger(index)||index<0||index>=this.totalFrames)throw new Error('Invalid quantized frame index.');
    this.quantized.add(index);return this.snapshot();
  }
  encodedFrame(index) {
    if(index!==this.encodedFrames||!this.quantized.has(index))throw new Error('Frames must be encoded in quantized source order.');
    this.encodedFrames++;return this.snapshot();
  }
  finishing() {
    if(this.encodedFrames!==this.totalFrames)throw new Error('Cannot finish an incomplete movie.');
    this.phase='finishing';return this.snapshot();
  }
  complete() {
    if(this.phase!=='finishing')throw new Error('Validate the finished MUP before completing conversion.');
    this.phase='complete';return this.snapshot();
  }
  snapshot() {
    const quantizedFrames=this.quantized.size;
    // Color conversion dominates the work. Reserve5% for final blocks,
    // container validation and the first independently decoded preview.
    const value=this.phase==='preparing'?null:this.phase==='complete'?1:
      Math.min(0.95,0.85*quantizedFrames/this.totalFrames+0.10*this.encodedFrames/this.totalFrames);
    return {phase:this.phase,totalFrames:this.totalFrames,quantizedFrames,encodedFrames:this.encodedFrames,value};
  }
}
