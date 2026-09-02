//! pimp3 — a streaming MP3 decoder that targets WebAssembly without Emscripten.
//!
//! A rebuild of the idea behind `bashi/minimp3-wasm`: compile a decoder to wasm
//! with no Emscripten runtime. That project reached the browser as a
//! decode-the-whole-file API and its Rust successor was left unfinished. Here
//! the decoder is pure Rust, so `wasm32-unknown-unknown` needs no C toolchain,
//! and the API is a pull-based stream so audio can start before the file ends.
//!
//! ```no_run
//! use pimp3_core::Mp3Decoder;
//! # fn main() -> Result<(), pimp3_core::DecodeError> {
//! # let mp3: Vec<u8> = Vec::new();
//! let mut decoder = Mp3Decoder::new(mp3)?;
//! println!("{} Hz", decoder.info().sample_rate_hz);
//! while let Some(chunk) = decoder.decode_next()? {
//!     // chunk.samples is interleaved f32 in [-1.0, 1.0]
//!     let _ = chunk.frame_count();
//! }
//! # Ok(()) }
//! ```

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod error;
mod source;

pub use error::{DecodeError, Result};

use source::MemorySource;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{Decoder, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

/// Stream parameters, known once the first frame header is parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioInfo {
    pub sample_rate_hz: u32,
    pub channel_count: u16,
    /// Total frames, when the stream declares a length. Absent for pure
    /// streams with no Xing/Info header.
    pub total_frames: Option<u64>,
}

impl AudioInfo {
    /// Duration in seconds, when the length is known.
    pub fn duration_seconds(&self) -> Option<f64> {
        if self.sample_rate_hz == 0 {
            return None;
        }
        self.total_frames
            .map(|f| f as f64 / f64::from(self.sample_rate_hz))
    }
}

/// One decoded block: interleaved `f32` samples in `[-1.0, 1.0]`.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioChunk {
    pub samples: Vec<f32>,
    pub channel_count: u16,
    /// Frame index of the first sample, relative to the start of the stream.
    pub start_frame: u64,
}

impl AudioChunk {
    /// Samples per channel.
    pub fn frame_count(&self) -> usize {
        if self.channel_count == 0 {
            return 0;
        }
        self.samples.len() / usize::from(self.channel_count)
    }

    /// Copy one channel out of the interleaved buffer, which is the layout
    /// `AudioBuffer.copyToChannel` and `AudioWorklet` both want.
    pub fn channel(&self, index: u16) -> Vec<f32> {
        if index >= self.channel_count || self.channel_count == 0 {
            return Vec::new();
        }
        self.samples
            .iter()
            .skip(usize::from(index))
            .step_by(usize::from(self.channel_count))
            .copied()
            .collect()
    }
}

/// A pull-based MP3 decoder over an in-memory buffer.
pub struct Mp3Decoder {
    format: Box<dyn FormatReader>,
    decoder: Box<dyn Decoder>,
    track_id: u32,
    info: AudioInfo,
    next_frame: u64,
    /// Frames successfully decoded so far.
    decoded_frames: u64,
    /// Frames lost to packets the decoder itself rejected.
    decode_error_frames: u64,
    reached_end: bool,
    seeked: bool,
}

impl Mp3Decoder {
    /// Parse the stream header and prepare to decode. Does not decode audio.
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        let stream = MediaSourceStream::new(Box::new(MemorySource::new(bytes)), Default::default());
        let mut hint = Hint::new();
        hint.with_extension("mp3");
        hint.mime_type("audio/mpeg");

        let probed = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|_| DecodeError::NotMpegAudio)?;
        let format = probed.format;

        // Copy the parameters out before `format` is moved into the struct;
        // the track borrow cannot outlive that move.
        let (track_id, params) = {
            let track = format.tracks().first().ok_or(DecodeError::NoAudioTrack)?;
            (track.id, track.codec_params.clone())
        };
        let params = &params;

        let sample_rate_hz = params.sample_rate.ok_or(DecodeError::UnsupportedStream {
            detail: "stream does not declare a sample rate".into(),
        })?;
        let channel_count =
            params
                .channels
                .map(|c| c.count() as u16)
                .ok_or(DecodeError::UnsupportedStream {
                    detail: "stream does not declare channels".into(),
                })?;

        let decoder = symphonia::default::get_codecs()
            .make(params, &DecoderOptions::default())
            .map_err(|e| DecodeError::UnsupportedStream {
                detail: e.to_string(),
            })?;

        Ok(Self {
            format,
            decoder,
            track_id,
            info: AudioInfo {
                sample_rate_hz,
                channel_count,
                total_frames: params.n_frames,
            },
            next_frame: 0,
            decoded_frames: 0,
            decode_error_frames: 0,
            reached_end: false,
            seeked: false,
        })
    }

    pub fn info(&self) -> AudioInfo {
        self.info
    }

    /// Frames successfully decoded so far.
    pub fn decoded_frames(&self) -> u64 {
        self.decoded_frames
    }

    /// Frames of audio missing from the output.
    ///
    /// Corruption is usually absorbed by the demuxer, which resynchronises to
    /// the next valid frame header without the decoder ever seeing the bad
    /// bytes — so counting decoder errors alone reports zero on a visibly
    /// damaged file. The dependable signal is the length the container itself
    /// declares in its Xing/Info header, compared against what came out.
    ///
    /// That comparison only holds for a full decode from the start, so after a
    /// seek this falls back to counting decoder-stage failures alone.
    pub fn dropped_frames(&self) -> u64 {
        match self.info.total_frames {
            Some(declared) if self.reached_end && !self.seeked => {
                declared.saturating_sub(self.decoded_frames)
            }
            _ => self.decode_error_frames,
        }
    }

    /// Decode the next block, or `Ok(None)` at end of stream.
    ///
    /// A damaged packet is counted and skipped rather than ending the stream:
    /// MP3 is routinely served with truncated or spliced frames, and a player
    /// that stops at the first bad byte is worse than one that drops it.
    pub fn decode_next(&mut self) -> Result<Option<AudioChunk>> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(SymphoniaError::IoError(e))
                    if e.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    self.reached_end = true;
                    return Ok(None);
                }
                Err(SymphoniaError::ResetRequired) => {
                    self.reached_end = true;
                    return Ok(None);
                }
                Err(e) => {
                    return Err(DecodeError::Io {
                        detail: e.to_string(),
                    })
                }
            };
            if packet.track_id() != self.track_id {
                continue;
            }

            match self.decoder.decode(&packet) {
                Ok(buffer) => {
                    let spec = *buffer.spec();
                    let duration = buffer.capacity() as u64;
                    let mut sample_buffer = SampleBuffer::<f32>::new(duration, spec);
                    sample_buffer.copy_interleaved_ref(buffer);
                    let samples = sample_buffer.samples().to_vec();

                    let channel_count = spec.channels.count() as u16;
                    let start_frame = self.next_frame;
                    if channel_count > 0 {
                        let frames = (samples.len() / usize::from(channel_count)) as u64;
                        self.next_frame += frames;
                        self.decoded_frames += frames;
                    }
                    if samples.is_empty() {
                        continue;
                    }
                    return Ok(Some(AudioChunk {
                        samples,
                        channel_count,
                        start_frame,
                    }));
                }
                Err(SymphoniaError::DecodeError(_)) => {
                    self.decode_error_frames += packet.dur();
                    continue;
                }
                Err(e) => {
                    return Err(DecodeError::CorruptFrame {
                        detail: e.to_string(),
                    })
                }
            }
        }
    }

    /// Decode the remainder of the stream into one interleaved buffer.
    pub fn decode_all(&mut self) -> Result<Vec<f32>> {
        let mut out = Vec::new();
        while let Some(chunk) = self.decode_next()? {
            out.extend_from_slice(&chunk.samples);
        }
        Ok(out)
    }

    /// Move the read position to `seconds` from the start.
    pub fn seek(&mut self, seconds: f64) -> Result<()> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(DecodeError::Seek {
                detail: format!("invalid target {seconds}"),
            });
        }
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::from(seconds),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(|e| DecodeError::Seek {
                detail: e.to_string(),
            })?;
        // The decoder holds inter-frame state (the bit reservoir), which is
        // meaningless after a jump.
        self.decoder.reset();
        self.next_frame = seeked.actual_ts;
        // A deliberate jump invalidates the declared-versus-decoded comparison.
        self.seeked = true;
        self.reached_end = false;
        Ok(())
    }
}
