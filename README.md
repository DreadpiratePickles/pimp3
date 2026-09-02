# pimp3

A streaming MP3 decoder compiled to WebAssembly **without Emscripten**, with
seeking, damage tolerance, and an AudioWorklet player that starts playing before
the file has finished decoding.

A rebuild of [bashi/minimp3-wasm](https://github.com/bashi/minimp3-wasm) (2020),
which proved you could compile a decoder to wasm with raw clang instead of the
Emscripten runtime. Its Rust successor, `wasm-minimp3-rs`, was left unfinished.
This is that idea carried through: pure Rust, so `wasm32-unknown-unknown` needs
no C toolchain at all.

## What it does

- Decode MP3 to interleaved `f32` — whole-file or **pull-based, block at a time**
- **Seek** to any position, with the decoder's inter-frame state correctly reset
- **Survive damaged streams** instead of stopping at the first bad byte, and
  report how much audio was lost
- Run in the browser, in a worklet-backed player, or as a native CLI

## Use it

### Web

```bash
make web && cd web && npm run preview
```

Drop in an MP3. The **Buffered ahead** figure is audio decoded but not yet
played — watching it stay positive while the track plays is the streaming
working: the audio thread is never waiting on the decoder.

The decoder runs on the main thread in time-boxed slices between animation
frames; the AudioWorklet owns only a ring buffer and never allocates or decodes.

### Command line

```bash
make cli
./target/release/pimp3 track.mp3 --output track.wav     # 32-bit float WAV
./target/release/pimp3 --info track.mp3
./target/release/pimp3 track.mp3 --seek 30 --duration 10 --output clip.wav
```

Reads stdin and writes stdout when paths are omitted, so it pipes.

### As a crate

```rust
use pimp3_core::Mp3Decoder;

let mut decoder = Mp3Decoder::new(mp3_bytes)?;
while let Some(chunk) = decoder.decode_next()? {
    // chunk.samples is interleaved f32; chunk.channel(0) de-interleaves one channel
}
```

## How it is verified

A decoder that only agrees with itself proves nothing, so correctness is pinned
two independent ways.

```bash
make test        # 11 Rust tests against LAME-encoded fixtures
make reference   # sample-for-sample comparison against ffmpeg
```

**The fixtures are pure tones**, so what the output *should* be is known
analytically. The tests assert energy at the stated frequency using a Goertzel
filter — a 440 Hz fixture has to decode to 440 Hz and not to its harmonic. The
stereo fixture carries 440 Hz on the left and 880 Hz on the right, so a swapped
or mis-strided de-interleave fails immediately.

**`make reference` compares against ffmpeg.** After aligning for encoder delay,
the residual RMS is `~2e-07` against a signal RMS of `0.336`, and correlation is
`1.000000` on every fixture. That is agreement to the limit of `f32`.

Also covered: a deliberately corrupted stream (512 bytes overwritten mid-file),
which must still decode most of its audio; truncated prefixes, which must never
panic; and non-audio input, which must be rejected with a typed error.

> A note on that alignment, since it bit during development: these fixtures are
> pure sines, and a sine correlates just as strongly with itself shifted by any
> whole number of periods. A dot-product search locks onto the wrong period and
> then reports a false mismatch. The comparison minimises squared error instead.

## Limitations

**No gapless playback yet.** LAME writes encoder delay and end padding into the
stream, and ffmpeg trims both — a 2.000 s source decodes to exactly 2.000 s
there, and to 2.038 s here. The extra is real encoder padding, not corruption,
and the samples in between match ffmpeg exactly. Honouring the LAME/Xing delay
fields is the main open item.

**It is much larger than minimp3-wasm**, and that is a deliberate trade:

| | wasm size |
|---|---|
| `minimp3-wasm` (minimp3, hand-written bindings) | ~20 KB |
| `pimp3` (Symphonia) | 380 KB, 216 KB gzipped |

minimp3 is a single-file decoder with no demuxer. Symphonia brings a real MPEG
demuxer, ID3 handling, accurate seeking and resynchronisation past damage —
which is what makes seeking and damage tolerance possible here at all. If you
need the smallest possible decode-only build, minimp3 remains the better answer.

Also not implemented: MP1/MP2, decoding inside a Web Worker (the decoder shares
the main thread today), and ReplayGain.

## Layout

```
crates/pimp3-core/   decoder: streaming, seeking, loss accounting. No I/O, no wasm.
crates/pimp3-cli/    native binary, WAV writer, zero extra dependencies
crates/pimp3-wasm/   wasm-bindgen surface
web/                 Vite + TypeScript AudioWorklet player
fixtures/            tone corpus, generator, and the ffmpeg comparison
```

`pimp3-core` is `#![forbid(unsafe_code)]` and denies `unwrap`/`expect`, so
malformed input travels through the typed `DecodeError` path.

## Build requirements

- Rust 1.82+ with the `wasm32-unknown-unknown` target
- `wasm-pack` and `binaryen` for the wasm build
- Node 20+ for the web app
- `uv`, `lame` and `ffmpeg` for the fixture and reference scripts

## Licence

MIT or Apache-2.0, at your option. The fixtures are generated tones and carry no
third-party licence. Decoding is provided by
[Symphonia](https://github.com/pdeljanov/Symphonia) (MPL-2.0).
