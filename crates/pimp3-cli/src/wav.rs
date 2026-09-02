//! Minimal RIFF/WAVE writer for 32-bit float PCM.
//!
//! Float output is deliberate: the decoder produces `f32`, and writing 16-bit
//! would add a quantisation step that the reference comparison would then have
//! to model. Keeping the samples exact makes the numpy check meaningful.

const RIFF: &[u8; 4] = b"RIFF";
const WAVE: &[u8; 4] = b"WAVE";
const FMT: &[u8; 4] = b"fmt ";
const DATA: &[u8; 4] = b"data";
const FORMAT_IEEE_FLOAT: u16 = 3;
const BITS_PER_SAMPLE: u16 = 32;

pub fn encode(samples: &[f32], sample_rate_hz: u32, channel_count: u16) -> Vec<u8> {
    let bytes_per_sample = u32::from(BITS_PER_SAMPLE / 8);
    let block_align = u32::from(channel_count) * bytes_per_sample;
    let data_len = (samples.len() as u32) * bytes_per_sample;

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(RIFF);
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(WAVE);

    out.extend_from_slice(FMT);
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&FORMAT_IEEE_FLOAT.to_le_bytes());
    out.extend_from_slice(&channel_count.to_le_bytes());
    out.extend_from_slice(&sample_rate_hz.to_le_bytes());
    out.extend_from_slice(&(sample_rate_hz * block_align).to_le_bytes());
    out.extend_from_slice(&(block_align as u16).to_le_bytes());
    out.extend_from_slice(&BITS_PER_SAMPLE.to_le_bytes());

    out.extend_from_slice(DATA);
    out.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}
