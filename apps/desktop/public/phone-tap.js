// OpenBark phone window: an AudioWorklet that taps both sides of a call
// (ADR-0023 section 7). Input 0 carries two channels: the clinic's microphone
// on the left, the client's voice on the right. Frames are interleaved and
// posted about every 100 ms, so the engine gets a few messages a second.
// A static file so the phone page's Content-Security-Policy can stay at
// script-src 'self'.
class CallTap extends AudioWorkletProcessor {
  constructor() {
    super();
    this.frames = Math.round(sampleRate / 10);
    this.buffer = new Float32Array(this.frames * 2);
    this.filled = 0;
  }

  process(inputs) {
    const input = inputs[0];
    if (!input || input.length === 0) return true;
    const left = input[0];
    const right = input[1] || input[0].map(() => 0);
    for (let i = 0; i < left.length; i++) {
      this.buffer[this.filled * 2] = left[i];
      this.buffer[this.filled * 2 + 1] = right[i];
      this.filled++;
      if (this.filled === this.frames) {
        this.port.postMessage(this.buffer);
        this.buffer = new Float32Array(this.frames * 2);
        this.filled = 0;
      }
    }
    return true;
  }
}

registerProcessor("openbark-call-tap", CallTap);
