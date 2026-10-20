// OpenBark web mode: an AudioWorklet that taps the microphone and posts each
// block of samples to the page, which streams them to the engine.
// A static file so the page's Content-Security-Policy can stay at
// script-src 'self' (a blob: module is refused there, which stopped web-mode
// recording from starting in 0.1.2).
class Tap extends AudioWorkletProcessor {
  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (ch) this.port.postMessage(ch.slice(0));
    return true;
  }
}
registerProcessor("openbark-tap", Tap);
