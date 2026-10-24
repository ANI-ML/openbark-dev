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
    // Per channel: counts once it carries signal, until a second of silence.
    this.active = [];
    this.quiet = [];
  }

  process(inputs) {
    const input = inputs[0];
    if (!input || input.length === 0) return true;
    const length = input[0].length;
    // Mix down to mono: the average of the channels that carry signal, so a
    // microphone on one input of a multi-input interface keeps its level
    // (the engine's capture does the same; most microphones are mono anyway).
    let active = [];
    for (let c = 0; c < input.length; c++) {
      let peak = 0;
      for (let i = 0; i < length; i++) peak = Math.max(peak, Math.abs(input[c][i]));
      if (peak >= 1e-4) {
        this.active[c] = true;
        this.quiet[c] = 0;
      } else {
        this.quiet[c] = (this.quiet[c] || 0) + length;
        if (this.quiet[c] >= sampleRate) this.active[c] = false;
      }
      if (this.active[c]) active.push(input[c]);
    }
    if (active.length === 0) active = input;
    const n = active.length;
    for (let i = 0; i < length; i++) {
      let v = 0;
      for (let c = 0; c < n; c++) v += active[c][i];
      v /= n;
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
