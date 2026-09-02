/**
 * Streaming player: wasm decoder on this thread, AudioWorklet on the audio one.
 *
 * Decoding runs in small time-boxed slices between animation frames rather than
 * in one blocking pass, so playback can start after the first few blocks while
 * the rest of the file is still being decoded.
 */
import init, { Mp3Stream } from "./wasm/pimp3_wasm.js";
import wasmUrl from "./wasm/pimp3_wasm_bg.wasm?url";
import processorUrl from "./pcm-processor.js?url";

/** Decode at most this long per slice, to stay inside a frame budget. */
const DECODE_SLICE_MS = 6;
/** Stop decoding ahead once this much audio is queued. */
const TARGET_BUFFER_SECONDS = 3;
/** Start playback once this much is queued. */
const PREBUFFER_SECONDS = 0.25;

export interface StreamInfo {
  readonly sampleRateHz: number;
  readonly channelCount: number;
  readonly durationSeconds: number | undefined;
}

export interface PlayerEvents {
  onInfo(info: StreamInfo): void;
  onProgress(playedSeconds: number, bufferedSeconds: number): void;
  onEnded(): void;
  onError(message: string): void;
}

let wasmReady: Promise<void> | undefined;

function loadWasm(): Promise<void> {
  wasmReady ??= init({ module_or_path: wasmUrl }).then(() => undefined);
  return wasmReady;
}

export class Player {
  #context: AudioContext | undefined;
  #node: AudioWorkletNode | undefined;
  #stream: Mp3Stream | undefined;
  #info: StreamInfo | undefined;
  #decodeHandle: number | undefined;
  #finished = false;
  #framesQueued = 0;
  #framesPlayed = 0;

  constructor(private readonly events: PlayerEvents) {}

  get info(): StreamInfo | undefined {
    return this.#info;
  }

  /** Parse a file and prepare the audio graph. Does not start playback. */
  async load(bytes: Uint8Array): Promise<StreamInfo> {
    await loadWasm();
    this.stop();

    const stream = new Mp3Stream(bytes);
    const duration = stream.durationSeconds;
    const info: StreamInfo = {
      sampleRateHz: stream.sampleRate,
      channelCount: stream.channelCount,
      durationSeconds: Number.isNaN(duration) ? undefined : duration,
    };

    // Match the graph to the file so the browser does not resample behind us.
    const context = new AudioContext({ sampleRate: info.sampleRateHz });
    await context.audioWorklet.addModule(processorUrl);
    const node = new AudioWorkletNode(context, "pcm-processor", {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [Math.max(1, info.channelCount)],
      processorOptions: { channelCount: info.channelCount },
    });
    node.connect(context.destination);
    node.port.onmessage = (event) => this.#onWorkletMessage(event.data);

    this.#context = context;
    this.#node = node;
    this.#stream = stream;
    this.#info = info;
    this.#finished = false;
    this.#framesQueued = 0;
    this.#framesPlayed = 0;

    this.events.onInfo(info);
    this.#pump();
    return info;
  }

  async play(): Promise<void> {
    if (!this.#context || !this.#node) return;
    await this.#context.resume();
    this.#node.port.postMessage({ type: "play" });
    this.#pump();
  }

  pause(): void {
    this.#node?.port.postMessage({ type: "pause" });
  }

  /** Seek, discarding everything already queued. */
  seek(seconds: number): void {
    if (!this.#stream || !this.#node || !this.#info) return;
    try {
      this.#stream.seek(seconds);
    } catch (error) {
      this.events.onError(error instanceof Error ? error.message : "seek failed");
      return;
    }
    const framesPlayed = Math.round(seconds * this.#info.sampleRateHz);
    this.#finished = false;
    this.#framesQueued = framesPlayed;
    this.#framesPlayed = framesPlayed;
    this.#node.port.postMessage({ type: "reset", framesPlayed });
    this.#pump();
  }

  stop(): void {
    if (this.#decodeHandle !== undefined) {
      cancelAnimationFrame(this.#decodeHandle);
      this.#decodeHandle = undefined;
    }
    this.#node?.disconnect();
    void this.#context?.close();
    this.#context = undefined;
    this.#node = undefined;
    this.#stream = undefined;
  }

  #onWorkletMessage(message: { type: string; framesBuffered?: number; framesPlayed?: number }): void {
    if (!this.#info) return;
    if (message.type === "level" || message.type === "drained") {
      this.#framesPlayed = message.framesPlayed ?? this.#framesPlayed;
      const buffered = (message.framesBuffered ?? 0) / this.#info.sampleRateHz;
      this.events.onProgress(this.#framesPlayed / this.#info.sampleRateHz, buffered);
    }
    if (message.type === "drained" && this.#finished) {
      this.events.onEnded();
    }
  }

  /** Decode a time-boxed slice, then yield back to the browser. */
  #pump = (): void => {
    this.#decodeHandle = undefined;
    const stream = this.#stream;
    const node = this.#node;
    const info = this.#info;
    if (!stream || !node || !info || this.#finished) return;

    const aheadSeconds = (this.#framesQueued - this.#framesPlayed) / info.sampleRateHz;
    if (aheadSeconds < TARGET_BUFFER_SECONDS) {
      const deadline = performance.now() + DECODE_SLICE_MS;
      try {
        while (performance.now() < deadline) {
          const chunk = stream.decodeNext();
          if (!chunk) {
            this.#finished = true;
            node.port.postMessage({ type: "end" });
            break;
          }
          const samples = chunk.samples;
          this.#framesQueued += chunk.frameCount;
          node.port.postMessage({ type: "samples", samples }, [samples.buffer]);
          if ((this.#framesQueued - this.#framesPlayed) / info.sampleRateHz >= TARGET_BUFFER_SECONDS) {
            break;
          }
        }
      } catch (error) {
        this.events.onError(error instanceof Error ? error.message : "decoding failed");
        this.#finished = true;
        return;
      }
    }

    // Enough queued to start without an immediate underrun.
    if ((this.#framesQueued - this.#framesPlayed) / info.sampleRateHz >= PREBUFFER_SECONDS) {
      node.port.postMessage({ type: "ready" });
    }
    if (!this.#finished) {
      this.#decodeHandle = requestAnimationFrame(this.#pump);
    }
  };
}
