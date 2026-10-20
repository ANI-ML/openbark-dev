// OpenBark web mode: an AudioWorklet that taps the microphone for a
// recording. Mono frames are collected and posted about every 50 ms, so the
// page sends the engine about 20 messages a second rather than one per
// 128-frame render quantum. Each message also carries the block's peak, so
// the page can tell digital silence (a muted or blocked microphone) from a
// quiet room without scanning the samples again.
// A static file, so the page's Content-Security-Policy can stay at
// script-src 'self' (a blob: module is blocked by it).
class MicTap extends AudioWorkletProcessor {
  constructor() {
    super();
    this.frames = Math.max(128, Math.round(sampleRate / 20));
    this.buffer = new Float32Array(this.frames);
    this.filled = 0;
    this.peak = 0;
  }

  process(inputs) {
    const input = inputs[0];
    if (!input || input.length === 0) return true;
    const channels = input.length;
    const length = input[0].length;
    for (let i = 0; i < length; i++) {
      // Mix every channel down to mono (most microphones are already mono).
      let v = 0;
      for (let c = 0; c < channels; c++) v += input[c][i];
      v /= channels;
      this.buffer[this.filled++] = v;
      const a = v < 0 ? -v : v;
      if (a > this.peak) this.peak = a;
      if (this.filled === this.frames) {
        this.port.postMessage({ samples: this.buffer, peak: this.peak }, [this.buffer.buffer]);
        this.buffer = new Float32Array(this.frames);
        this.filled = 0;
        this.peak = 0;
      }
    }
    return true;
  }
}

registerProcessor("openbark-mic-tap", MicTap);
