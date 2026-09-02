//! WebAssembly surface for pimp3.
//!
//! The decoder is exposed as a handle with a pull API rather than a single
//! decode-everything call, so a page can start playing audio while the rest of
//! the file is still being decoded. That is the main thing `minimp3-wasm`'s
//! decode-the-whole-buffer interface could not do.

#![forbid(unsafe_code)]

use pimp3_core::Mp3Decoder;
use wasm_bindgen::prelude::*;

/// One decoded block, handed to JavaScript as interleaved `f32`.
#[wasm_bindgen]
pub struct Chunk {
    samples: Vec<f32>,
    channel_count: u16,
    start_frame: u64,
}

#[wasm_bindgen]
impl Chunk {
    /// Interleaved samples in `[-1.0, 1.0]`. Copies out of wasm memory, so the
    /// result stays valid across later decoder calls.
    #[wasm_bindgen(getter)]
    pub fn samples(&self) -> Vec<f32> {
        self.samples.clone()
    }

    #[wasm_bindgen(getter, js_name = channelCount)]
    pub fn channel_count(&self) -> u16 {
        self.channel_count
    }

    #[wasm_bindgen(getter, js_name = startFrame)]
    pub fn start_frame(&self) -> f64 {
        self.start_frame as f64
    }

    #[wasm_bindgen(getter, js_name = frameCount)]
    pub fn frame_count(&self) -> usize {
        if self.channel_count == 0 {
            return 0;
        }
        self.samples.len() / usize::from(self.channel_count)
    }
}

/// An open MP3 stream.
#[wasm_bindgen]
pub struct Mp3Stream {
    decoder: Mp3Decoder,
}

#[wasm_bindgen]
impl Mp3Stream {
    /// Parse the stream header. Throws if the bytes are not MPEG audio.
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: Vec<u8>) -> Result<Mp3Stream, JsValue> {
        Mp3Decoder::new(bytes)
            .map(|decoder| Mp3Stream { decoder })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    #[wasm_bindgen(getter, js_name = sampleRate)]
    pub fn sample_rate(&self) -> u32 {
        self.decoder.info().sample_rate_hz
    }

    #[wasm_bindgen(getter, js_name = channelCount)]
    pub fn channel_count(&self) -> u16 {
        self.decoder.info().channel_count
    }

    /// Duration in seconds, or `NaN` when the stream declares no length.
    #[wasm_bindgen(getter, js_name = durationSeconds)]
    pub fn duration_seconds(&self) -> f64 {
        self.decoder.info().duration_seconds().unwrap_or(f64::NAN)
    }

    /// Frames of audio lost to stream damage. See the Rust docs for why this is
    /// measured against the declared length rather than by counting errors.
    #[wasm_bindgen(getter, js_name = droppedFrames)]
    pub fn dropped_frames(&self) -> f64 {
        self.decoder.dropped_frames() as f64
    }

    /// Decode the next block, or `undefined` at end of stream.
    #[wasm_bindgen(js_name = decodeNext)]
    pub fn decode_next(&mut self) -> Result<Option<Chunk>, JsValue> {
        self.decoder
            .decode_next()
            .map(|maybe| {
                maybe.map(|c| Chunk {
                    samples: c.samples,
                    channel_count: c.channel_count,
                    start_frame: c.start_frame,
                })
            })
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Decode the remainder in one call, for short files or offline rendering.
    #[wasm_bindgen(js_name = decodeAll)]
    pub fn decode_all(&mut self) -> Result<Vec<f32>, JsValue> {
        self.decoder
            .decode_all()
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Jump to `seconds` from the start.
    pub fn seek(&mut self, seconds: f64) -> Result<(), JsValue> {
        self.decoder
            .seek(seconds)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
