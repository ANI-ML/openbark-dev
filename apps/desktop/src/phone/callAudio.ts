// Both sides of a call, tapped on this computer (ADR-0023 section 7).
//
// Twilio's Voice SDK hands each side to an AudioProcessor before it is sent
// (the clinic's microphone) or played (the client's voice). Each processor
// feeds one input of a channel merger: the clinic on the left, the client on
// the right. A worklet (public/phone-tap.js) interleaves them and posts about
// every 100 ms. Nothing here records in Twilio: the audio is only read here.
//
// Hold silences what plays here (a gain of 0 on the client's side), mutes
// the call, and records silence on both channels until it's taken off hold.

/** What Twilio's `device.audio.addProcessor` takes. */
export interface AudioProcessor {
  createProcessedStream(stream: MediaStream): Promise<MediaStream>;
  destroyProcessedStream(stream: MediaStream): Promise<void>;
}

export interface CallTaps {
  /** The clinic's microphone, before it's sent. */
  local: AudioProcessor;
  /** The client's voice, before it plays. */
  remote: AudioProcessor;
  sampleRate: number;
  /** Frames go to `onFrames` only while this is on. */
  setRecording(on: boolean): void;
  setHeld(on: boolean): void;
  close(): Promise<void>;
}

export async function createTaps(onFrames: (frames: Float32Array) => void): Promise<CallTaps> {
  const ctx = new AudioContext();
  await ctx.audioWorklet.addModule("/phone-tap.js");
  const merger = ctx.createChannelMerger(2);
  const tap = new AudioWorkletNode(ctx, "openbark-call-tap", {
    numberOfInputs: 1,
    channelCount: 2,
    channelCountMode: "explicit",
    channelInterpretation: "discrete",
  });
  merger.connect(tap);
  let recording = false;
  let held = false;
  tap.port.onmessage = (e: MessageEvent<Float32Array>) => {
    if (!recording) return;
    // On hold, neither side is recorded: the recording keeps its timing with silence.
    onFrames(held ? new Float32Array(e.data.length) : e.data);
  };
  const sources = new Map<MediaStream, MediaStreamAudioSourceNode>();
  const playback = ctx.createGain();

  const local: AudioProcessor = {
    async createProcessedStream(stream) {
      const src = ctx.createMediaStreamSource(stream);
      src.connect(merger, 0, 0);
      sources.set(stream, src);
      return stream;
    },
    async destroyProcessedStream(stream) {
      sources.get(stream)?.disconnect();
      sources.delete(stream);
    },
  };
  const remote: AudioProcessor = {
    async createProcessedStream(stream) {
      const src = ctx.createMediaStreamSource(stream);
      src.connect(merger, 0, 1);
      // What plays goes through a gain, so Hold can silence it.
      const out = ctx.createMediaStreamDestination();
      src.connect(playback);
      playback.connect(out);
      sources.set(stream, src);
      return out.stream;
    },
    async destroyProcessedStream(stream) {
      sources.get(stream)?.disconnect();
      sources.delete(stream);
    },
  };
  return {
    local,
    remote,
    sampleRate: ctx.sampleRate,
    setRecording(on) {
      recording = on;
      if (on) void ctx.resume();
    },
    setHeld(on) {
      held = on;
      playback.gain.value = on ? 0 : 1;
    },
    async close() {
      recording = false;
      tap.port.onmessage = null;
      sources.forEach((s) => s.disconnect());
      sources.clear();
      await ctx.close().catch(() => {});
    },
  };
}
