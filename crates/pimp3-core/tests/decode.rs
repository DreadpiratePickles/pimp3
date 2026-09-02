//! Tests against LAME-encoded fixtures whose contents are known analytically.
//!
//! Each fixture is a pure tone, so correctness is checkable without a reference
//! decoder: the decoded audio either has its energy at the stated frequency or
//! it does not. Byte-level agreement with ffmpeg is checked separately in
//! `fixtures/compare_reference.py`, since MP3 decoders legitimately differ in
//! the last bits and in how they treat encoder delay.

use pimp3_core::{DecodeError, Mp3Decoder};

const MONO_44100: &[u8] = include_bytes!("../../../fixtures/sine_440_mono_44100.mp3");
const STEREO_44100: &[u8] = include_bytes!("../../../fixtures/sine_stereo_44100.mp3");
const MONO_22050: &[u8] = include_bytes!("../../../fixtures/sine_440_mono_22050.mp3");
const DAMAGED: &[u8] = include_bytes!("../../../fixtures/damaged.mp3");
const NOT_AUDIO: &[u8] = include_bytes!("../../../fixtures/not_audio.bin");

/// Goertzel: the energy a signal carries at one frequency. Cheaper than an FFT
/// and enough to say "this is a 440 Hz tone and not an 880 Hz one".
fn power_at(samples: &[f32], frequency_hz: f64, sample_rate_hz: u32) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let n = samples.len() as f64;
    let k = (0.5 + (n * frequency_hz) / f64::from(sample_rate_hz)).floor();
    let omega = (2.0 * std::f64::consts::PI * k) / n;
    let coeff = 2.0 * omega.cos();
    let (mut s1, mut s2) = (0.0f64, 0.0f64);
    for &sample in samples {
        let s0 = f64::from(sample) + coeff * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - coeff * s1 * s2) / (n * n)
}

/// Decode everything, returning interleaved samples alongside the stream info.
fn decode(bytes: &[u8]) -> (Vec<f32>, pimp3_core::AudioInfo, u64) {
    let mut decoder = Mp3Decoder::new(bytes.to_vec()).expect("fixture should decode");
    let info = decoder.info();
    let samples = decoder.decode_all().expect("decoding should not fail");
    (samples, info, decoder.dropped_frames())
}

/// Drop the first and last 10% to avoid the encoder's delay and padding.
fn steady_state(samples: &[f32]) -> &[f32] {
    let margin = samples.len() / 10;
    samples
        .get(margin..samples.len() - margin)
        .unwrap_or(samples)
}

#[test]
fn reports_stream_parameters() {
    let (_, info, _) = decode(MONO_44100);
    assert_eq!(info.sample_rate_hz, 44100);
    assert_eq!(info.channel_count, 1);

    let (_, stereo, _) = decode(STEREO_44100);
    assert_eq!(stereo.channel_count, 2);

    let (_, low_rate, _) = decode(MONO_22050);
    assert_eq!(low_rate.sample_rate_hz, 22050);
}

#[test]
fn decodes_a_440_hz_tone_and_not_a_harmonic() {
    let (samples, info, _) = decode(MONO_44100);
    let body = steady_state(&samples);
    let fundamental = power_at(body, 440.0, info.sample_rate_hz);
    let harmonic = power_at(body, 880.0, info.sample_rate_hz);
    let unrelated = power_at(body, 1234.0, info.sample_rate_hz);

    assert!(fundamental > 0.0, "no energy at 440 Hz");
    assert!(
        fundamental > harmonic * 100.0,
        "440 Hz ({fundamental:.6}) should dominate 880 Hz ({harmonic:.6})"
    );
    assert!(
        fundamental > unrelated * 100.0,
        "440 Hz should dominate an unrelated bin"
    );
}

#[test]
fn keeps_stereo_channels_in_the_right_order() {
    // The fixture is 440 Hz on the left and 880 Hz on the right. A swapped or
    // mis-strided de-interleave shows up immediately here.
    let mut decoder = Mp3Decoder::new(STEREO_44100.to_vec()).unwrap();
    let info = decoder.info();
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while let Some(chunk) = decoder.decode_next().unwrap() {
        left.extend_from_slice(&chunk.channel(0));
        right.extend_from_slice(&chunk.channel(1));
    }

    let left_body = steady_state(&left);
    let right_body = steady_state(&right);
    assert!(
        power_at(left_body, 440.0, info.sample_rate_hz)
            > power_at(left_body, 880.0, info.sample_rate_hz) * 50.0,
        "left channel should carry 440 Hz"
    );
    assert!(
        power_at(right_body, 880.0, info.sample_rate_hz)
            > power_at(right_body, 440.0, info.sample_rate_hz) * 50.0,
        "right channel should carry 880 Hz"
    );
}

#[test]
fn decoded_length_matches_the_encoded_duration() {
    let (samples, info, _) = decode(MONO_44100);
    let seconds = samples.len() as f64 / f64::from(info.sample_rate_hz);
    // LAME adds encoder delay and pads the final frame, so 2.0 s in becomes
    // slightly more out. A tenth of a second of slack covers both.
    assert!(
        (seconds - 2.0).abs() < 0.1,
        "expected about 2 s, decoded {seconds:.3} s"
    );
}

#[test]
fn low_sample_rate_streams_decode_at_their_own_rate() {
    let (samples, info, _) = decode(MONO_22050);
    assert_eq!(info.sample_rate_hz, 22050);
    let body = steady_state(&samples);
    assert!(
        power_at(body, 440.0, info.sample_rate_hz)
            > power_at(body, 880.0, info.sample_rate_hz) * 50.0,
        "22050 Hz stream should still be a 440 Hz tone"
    );
    let seconds = samples.len() as f64 / f64::from(info.sample_rate_hz);
    assert!(
        (seconds - 1.5).abs() < 0.1,
        "expected about 1.5 s, decoded {seconds:.3} s"
    );
}

#[test]
fn survives_a_corrupt_region_instead_of_stopping_at_it() {
    let (samples, info, dropped) = decode(DAMAGED);
    assert!(dropped > 0, "corrupted fixture should report lost frames");
    let seconds = samples.len() as f64 / f64::from(info.sample_rate_hz);
    // 512 bytes of damage costs a few frames, not the rest of the file.
    assert!(
        seconds > 1.5,
        "expected most of the audio to survive, got {seconds:.3} s"
    );
    let body = steady_state(&samples);
    assert!(
        power_at(body, 440.0, info.sample_rate_hz)
            > power_at(body, 880.0, info.sample_rate_hz) * 10.0,
        "the surviving audio should still be the original tone"
    );
}

#[test]
fn seeking_moves_the_read_position() {
    let mut decoder = Mp3Decoder::new(MONO_44100.to_vec()).unwrap();
    let rate = f64::from(decoder.info().sample_rate_hz);
    decoder.seek(1.0).expect("seek to 1 s");
    let chunk = decoder
        .decode_next()
        .unwrap()
        .expect("audio after the seek point");
    let position = chunk.start_frame as f64 / rate;
    assert!(
        (position - 1.0).abs() < 0.1,
        "expected to land near 1 s, landed at {position:.3} s"
    );

    // Whatever is decoded after seeking must still be the tone.
    let mut rest = chunk.samples;
    for _ in 0..20 {
        match decoder.decode_next().unwrap() {
            Some(next) => rest.extend_from_slice(&next.samples),
            None => break,
        }
    }
    assert!(power_at(&rest, 440.0, 44100) > power_at(&rest, 880.0, 44100) * 20.0);
}

#[test]
fn seeking_past_the_end_or_to_nonsense_is_an_error_not_a_panic() {
    let mut decoder = Mp3Decoder::new(MONO_44100.to_vec()).unwrap();
    assert!(matches!(decoder.seek(-1.0), Err(DecodeError::Seek { .. })));
    assert!(matches!(
        decoder.seek(f64::NAN),
        Err(DecodeError::Seek { .. })
    ));
    // Seeking far past the end must not panic; either outcome is acceptable.
    let _ = decoder.seek(9_999.0);
}

#[test]
fn rejects_input_that_is_not_mpeg_audio() {
    assert!(matches!(
        Mp3Decoder::new(NOT_AUDIO.to_vec()),
        Err(DecodeError::NotMpegAudio)
    ));
    assert!(matches!(
        Mp3Decoder::new(Vec::new()),
        Err(DecodeError::NotMpegAudio)
    ));
}

#[test]
fn no_truncated_prefix_can_panic() {
    // The cheap stand-in for a fuzz target: every prefix of a real stream.
    for len in (0..MONO_44100.len()).step_by(97) {
        if let Ok(mut decoder) = Mp3Decoder::new(MONO_44100[..len].to_vec()) {
            let _ = decoder.decode_all();
        }
    }
}

#[test]
fn chunked_and_whole_file_decoding_agree() {
    let (whole, _, _) = decode(MONO_44100);
    let mut decoder = Mp3Decoder::new(MONO_44100.to_vec()).unwrap();
    let mut streamed = Vec::new();
    while let Some(chunk) = decoder.decode_next().unwrap() {
        assert_eq!(
            chunk.frame_count() * usize::from(chunk.channel_count),
            chunk.samples.len()
        );
        streamed.extend_from_slice(&chunk.samples);
    }
    assert_eq!(streamed, whole, "streaming and bulk decoding diverged");
}
