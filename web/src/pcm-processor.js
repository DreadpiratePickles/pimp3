/**
 * AudioWorklet side of the player.
 *
 * The worklet owns a ring buffer and nothing else. Decoding happens elsewhere
 * and arrives as interleaved Float32Array blocks over the message port, so the
 * audio thread never allocates, never decodes, and never blocks — which is the
 * whole reason to use a worklet instead of a ScriptProcessor.
 */

// Roughly two seconds at 48 kHz stereo. Large enough to absorb a slow decode
// tick, small enough that seeking stays responsive.
const CAPACITY_FRAMES = 96000;

class PcmProcessor extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.channelCount = options?.processorOptions?.channelCount ?? 2;
    this.buffer = new Float32Array(CAPACITY_FRAMES * this.channelCount);
    this.readIndex = 0;
    this.writeIndex = 0;
    this.available = 0;
    this.ended = false;
    this.playing = false;
    this.framesPlayed = 0;

    this.port.onmessage = (event) => {
      const message = event.data;
      if (message.type === "samples") {
        this.enqueue(message.samples);
      } else if (message.type === "end") {
        this.ended = true;
      } else if (message.type === "play") {
        this.playing = true;
      } else if (message.type === "pause") {
        this.playing = false;
      } else if (message.type === "reset") {
        this.readIndex = 0;
        this.writeIndex = 0;
        this.available = 0;
        this.ended = false;
        this.framesPlayed = message.framesPlayed ?? 0;
      }
    };
  }

  /** Copy a block in, dropping the overflow rather than growing on the audio thread. */
  enqueue(samples) {
    const capacity = this.buffer.length;
    for (let i = 0; i < samples.length; i += 1) {
      if (this.available >= capacity) break;
      this.buffer[this.writeIndex] = samples[i];
      this.writeIndex = (this.writeIndex + 1) % capacity;
      this.available += 1;
    }
    this.reportLevel();
  }

  reportLevel() {
    this.port.postMessage({
      type: "level",
      framesBuffered: Math.floor(this.available / this.channelCount),
      framesPlayed: this.framesPlayed,
    });
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || output.length === 0) return true;
    const frames = output[0].length;
    const capacity = this.buffer.length;

    for (let frame = 0; frame < frames; frame += 1) {
      for (let channel = 0; channel < output.length; channel += 1) {
        const source = channel < this.channelCount ? channel : this.channelCount - 1;
        let value = 0;
        if (this.playing && this.available >= this.channelCount) {
          value = this.buffer[(this.readIndex + source) % capacity];
        }
        output[channel][frame] = value;
      }
      if (this.playing && this.available >= this.channelCount) {
        this.readIndex = (this.readIndex + this.channelCount) % capacity;
        this.available -= this.channelCount;
        this.framesPlayed += 1;
      }
    }

    if (this.ended && this.available < this.channelCount) {
      this.port.postMessage({ type: "drained", framesPlayed: this.framesPlayed });
      this.playing = false;
    } else {
      this.reportLevel();
    }
    return true;
  }
}

registerProcessor("pcm-processor", PcmProcessor);
