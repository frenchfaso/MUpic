// Hardware output glue only. reSID Rust runs in a separate Web Worker.
class UE2AudioProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.capacity = Math.ceil(sampleRate * 0.20) + 1;
    this.initialTarget = Math.ceil(sampleRate * 0.032);
    this.recoveryTarget = Math.ceil(sampleRate * 0.032);
    this.target = this.initialTarget;
    this.left = new Float32Array(this.capacity); this.right = new Float32Array(this.capacity);
    this.read = 0; this.write = 0; this.available = 0; this.primed = false; this.enabled = false;
    this.consumedFrames = 0; this.receivedFrames = 0; this.clockCountdown = 0;
    const ring = options.processorOptions?.ring;
    if (ring) {
      this.control = new Int32Array(ring, 0, 8); this.sharedSamples = new Float32Array(ring, 32);
      this.capacity = this.sharedSamples.length / 2;
    }
    this.port.onmessage = ({ data }) => {
      if (data.type === "clear") this.clear();
      else if (data.type === "enabled") {
        const enabled = Boolean(data.enabled);
        if (enabled !== this.enabled) {
          this.enabled = enabled;
          if (this.control) Atomics.store(this.control, 5, enabled ? 1 : 0);
          this.clear();
        }
        // A producer may only fill the cold reserve after the output accepted
        // the activation and cleared the old cursor (not merely after posting it).
        if (Number.isSafeInteger(data.request)) this.port.postMessage({ type: "audio-ready", request: data.request });
      } else if (data.type === "attach-clock") {
        this.clockPort?.close(); this.clockPort = data.port;
        this.publishClock();
      } else if (data.type === "attach-port") {
        this.audioPort?.close(); this.audioPort = data.port;
        this.audioPort.onmessage = ({ data: packet }) => {
          if (packet.type !== "samples") return;
          this.receivedFrames += Math.floor(packet.samples / 2);
          if (this.enabled) this.append(new Int16Array(packet.buffer, 0, packet.samples));
          this.audioPort.postMessage({ type: "pcm-return", buffer: packet.buffer }, [packet.buffer]);
        };
        this.audioPort.start();
      }
    };
  }
  clear() {
    if (this.control) Atomics.store(this.control, 0, Atomics.load(this.control, 1));
    this.read = 0; this.write = 0; this.available = 0; this.primed = false;
    this.target = this.initialTarget;
    this.publishClock();
  }
  publishClock() {
    this.clockPort?.postMessage({ type: "audio-clock", consumedFrames: this.consumedFrames,
      queuedFrames: this.available, receivedFrames: this.receivedFrames });
  }
  append(samples) {
    const frames = Math.floor(samples.length / 2), skip = Math.max(0, frames - this.capacity), incoming = frames - skip;
    const discard = Math.max(0, this.available + incoming - this.capacity);
    this.read = (this.read + discard) % this.capacity; this.available -= discard;
    for (let frame = skip; frame < frames; frame++) {
      this.left[this.write] = samples[frame * 2] / 32768; this.right[this.write] = samples[frame * 2 + 1] / 32768;
      this.write = (this.write + 1) % this.capacity;
    }
    this.available += incoming;
  }
  process(_inputs, outputs) {
    const channels = outputs[0];
    if (!channels?.length) return true;
    const left = channels[0], right = channels[1] || left;
    let count = 0;
    let sharedRead = 0;
    if (this.control) {
      sharedRead = Atomics.load(this.control, 0);
      this.available = (Atomics.load(this.control, 1) - sharedRead + this.capacity) % this.capacity;
    }
    if (this.enabled && !this.primed && this.available >= this.target) {
      this.primed = true;
      // A 32 ms reserve fits below the 60 ms producer lead, including its
      // unpublished 10 ms tail. Underruns recover with the same bounded reserve.
      this.target = this.recoveryTarget;
    }
    if (this.enabled && this.primed) {
      count = Math.min(left.length, this.available);
      // A quantum spans at most two contiguous ring regions. Choose the
      // transport once and wrap once, keeping division out of the sample loop.
      if (this.control) {
        let read = sharedRead, i = 0;
        const first = Math.min(count, this.capacity - read);
        for (; i < first; i++, read++) {
          left[i] = this.sharedSamples[read * 2]; right[i] = this.sharedSamples[read * 2 + 1];
        }
        if (read === this.capacity) read = 0;
        for (; i < count; i++, read++) {
          left[i] = this.sharedSamples[read * 2]; right[i] = this.sharedSamples[read * 2 + 1];
        }
        sharedRead = read;
      } else {
        let read = this.read, i = 0;
        const first = Math.min(count, this.capacity - read);
        for (; i < first; i++, read++) { left[i] = this.left[read]; right[i] = this.right[read]; }
        if (read === this.capacity) read = 0;
        for (; i < count; i++, read++) { left[i] = this.left[read]; right[i] = this.right[read]; }
        this.read = read;
      }
      if (this.control) Atomics.store(this.control, 0, sharedRead);
      this.consumedFrames += count;
      if (this.control) Atomics.add(this.control, 6, count);
      this.available -= count;
      if (count < left.length) this.primed = false;
    }
    // Fully populated quanta need no silence fill. Partial or cold quanta keep
    // the exact silent tail and underrun behavior of the previous output path.
    left.fill(0, count); if (right !== left) right.fill(0, count);
    this.clockCountdown -= left.length;
    if (this.clockPort && this.clockCountdown <= 0) {
      this.clockCountdown += Math.ceil(sampleRate * 0.016);
      this.publishClock();
    }
    return true;
  }
}
registerProcessor("ue2-audio", UE2AudioProcessor);
