<div align="center">

# 🎵 pimp3

### Streaming MP3 → WebAssembly, in pure Rust. No Emscripten. No C toolchain. No waiting.

**Drop in a file — it's playing before it's done loading. Yes, really. Watch the buffer.**

![tests](https://img.shields.io/badge/tests-12%20passing-brightgreen)
![wasm](https://img.shields.io/badge/wasm-216%20KB%20gzipped-blue)
![toolchain](https://img.shields.io/badge/Emscripten-not%20required-orange)
![unsafe](https://img.shields.io/badge/unsafe__code-forbidden-8A2BE2)
![verified](https://img.shields.io/badge/vs%20ffmpeg-corr%201.000000-success)
![license](https://img.shields.io/badge/license-MIT%20%7C%20Apache--2.0-informational)

</div>

---

A streaming MP3 decoder compiled to WebAssembly **without Emscripten** — because it's not 2016 and you shouldn't need a C toolchain to make a browser play audio. It seeks. It shrugs off damaged streams. It feeds an AudioWorklet player that starts playing before the file has finished decoding. It targets `wasm32-unknown-unknown` with zero C in the build, and its output agrees with ffmpeg down to the last bit of an `f32`. Not "close." *The last bit.*

This is unfinished business. [bashi/minimp3-wasm](https://github.com/bashi/minimp3-wasm) (2020) proved you could compile a decoder to wasm with raw clang instead of the Emscripten runtime — a great idea whose Rust successor, `wasm-minimp3-rs`, was left on the workbench. pimp3 is that idea carried all the way through: pure Rust, so `wasm32-unknown-unknown` needs no C toolchain at all.

## What it does
- Decodes MP3 to interleaved `f32` — whole-file if you're basic, **pull-based, block at a time** if you're streaming
- **Seeks** to any position, with the decoder's inter-frame state correctly reset (the part everyone gets wrong)
- **Chews through damaged streams** instead of dying at the first bad byte — and tells you exactly how much audio it lost
- Runs in the browser, in a worklet-backed player, or as a native CLI. Pick your poison.

## Use it
### Web
```bash
make web && cd web && npm run preview
```
Drop in an MP3. That **Buffered ahead** number is audio decoded but not yet played — watching it stay positive while the track plays is the whole flex: the audio thread is *never* waiting on the decoder. The decoder runs on the main thread in time-boxed slices between animation frames; the AudioWorklet owns only a ring buffer and never allocates or decodes. Everyone stays in their lane.

### Command line
```bash
make cli
./target/release/pimp3 track.mp3 --output track.wav     # 32-bit float WAV
./target/release/pimp3 --info track.mp3
./target/release/pimp3 track.mp3 --seek 30 --duration 10 --output clip.wav
```
Omit the paths and it reads stdin, writes stdout, and pipes like a good Unix citizen.

### As a crate
```rust
use pimp3_core::Mp3Decoder;

let mut decoder = Mp3Decoder::new(mp3_bytes)?;
while let Some(chunk) = decoder.decode_next()? {
    // chunk.samples is interleaved f32; chunk.channel(0) de-interleaves one channel
}
```

## The receipts
I don't trust my own code, and neither should you. A decoder that only agrees with itself proves nothing — so correctness is pinned two independent ways, and both of them can hurt me.
```bash
make test        # 11 Rust tests against LAME-encoded fixtures
make reference   # sample-for-sample comparison against ffmpeg
```
**The fixtures are pure tones**, which means the right answer is known *analytically* — no golden files, no vibes. The tests assert energy at the stated frequency with a Goertzel filter: a 440 Hz fixture has to decode to 440 Hz, not to its harmonic. The stereo fixture carries 440 Hz on the left and 880 Hz on the right, so a swapped or mis-strided de-interleave gets caught red-handed instantly.

**`make reference` puts it in the ring with ffmpeg.** After aligning for encoder delay: residual RMS of `~2e-07` against a signal RMS of `0.336`, correlation `1.000000` on every fixture. That is agreement to the limit of `f32`. There is no more agreement available to purchase.

Also on the docket: a deliberately corrupted stream (512 bytes overwritten mid-file) that must still decode most of its audio; truncated prefixes that must never panic; and non-audio input that must be rejected with a typed error, not a tantrum.

> War story, because it bit me: these fixtures are pure sines, and a sine correlates just as hard with itself shifted by any whole number of periods. A dot-product alignment search happily locks onto the wrong period and then screams about a mismatch that isn't there. The comparison minimises squared error instead. You're welcome.

## Where I tell on myself
**No gapless playback yet.** LAME writes encoder delay and end padding into the stream, and ffmpeg trims both — a 2.000 s source decodes to exactly 2.000 s there, and to 2.038 s here. The extra is real encoder padding, not corruption, and every sample in between matches ffmpeg exactly. Honouring the LAME/Xing delay fields is the main open item. It's on the list. The list is real.

**It is much larger than minimp3-wasm.** On purpose. Observe the trade:
| | wasm size |
|---|---|
| `minimp3-wasm` (minimp3, hand-written bindings) | ~20 KB |
| `pimp3` (Symphonia) | 380 KB, 216 KB gzipped |

minimp3 is a single-file decoder with no demuxer — beautiful, tiny, and helpless. Symphonia brings a real MPEG demuxer, ID3 handling, accurate seeking, and resynchronisation past damage — which is the *entire reason* seeking and damage tolerance exist here at all. If you need the smallest possible decode-only build, minimp3 remains the better answer, and I'll say that with a straight face.

Also not implemented: MP1/MP2, decoding inside a Web Worker (the decoder shares the main thread today), and ReplayGain.

## The floor plan
```
crates/pimp3-core/   decoder: streaming, seeking, loss accounting. No I/O, no wasm.
crates/pimp3-cli/    native binary, WAV writer, zero extra dependencies
crates/pimp3-wasm/   wasm-bindgen surface
web/                 Vite + TypeScript AudioWorklet player
fixtures/            tone corpus, generator, and the ffmpeg comparison
```
`pimp3-core` is `#![forbid(unsafe_code)]` and denies `unwrap`/`expect` — malformed input doesn't get to crash the party, it travels through the typed `DecodeError` path like everyone else.

## What you need
- Rust 1.82+ with the `wasm32-unknown-unknown` target
- `wasm-pack` and `binaryen` for the wasm build
- Node 20+ for the web app
- `uv`, `lame` and `ffmpeg` for the fixture and reference scripts

## Licence
MIT or Apache-2.0, at your option. The fixtures are generated tones and carry no third-party licence. Decoding is provided by [Symphonia](https://github.com/pdeljanov/Symphonia) (MPL-2.0).
