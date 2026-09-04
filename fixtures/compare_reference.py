#!/usr/bin/env python3
"""Compare pimp3's output against ffmpeg's decoder.

The Rust tests prove the decoded audio is the right tone. This proves it is the
same audio a mature decoder produces, sample for sample, which is a much tighter
claim and the one that catches subtle errors in rounding, windowing or the bit
reservoir.

MP3 decoders are allowed to differ slightly, and they disagree about how much of
LAME's encoder delay to trim, so the comparison aligns the two signals first and
then requires the residual to be near the noise floor.

Run with:  uv run --with numpy python3 fixtures/compare_reference.py
"""
from __future__ import annotations

import struct
import subprocess
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "fixtures"
CLI = ROOT / "target" / "release" / "pimp3"

# Decoders may differ by a fraction of an LSB; anything at or below this is
# arithmetic noise rather than a decoding difference.
MAX_RESIDUAL_RMS = 1e-3
MIN_CORRELATION = 0.999
MAX_ALIGNMENT_SEARCH = 4096

failures: list[str] = []


def check(condition: bool, message: str) -> None:
    print(f"  [{'ok  ' if condition else 'FAIL'}] {message}")
    if not condition:
        failures.append(message)


def read_float_wav(path: Path) -> tuple[np.ndarray, int, int]:
    """Read a 32-bit float WAV as (frames, channels).

    Python's `wave` module rejects IEEE-float WAV (format tag 3), which is what
    both ffmpeg and pimp3 emit here, so the RIFF chunks are walked directly.
    Some ffmpeg builds wrap that float format in WAVE_FORMAT_EXTENSIBLE
    (tag 0xFFFE), where the real sample format is the SubFormat GUID instead;
    both spellings are accepted.
    """
    blob = path.read_bytes()
    if blob[:4] != b"RIFF" or blob[8:12] != b"WAVE":
        raise ValueError(f"{path.name}: not a RIFF/WAVE file")

    channels = rate = bits = 0
    data: bytes | None = None
    offset = 12
    while offset + 8 <= len(blob):
        chunk_id = blob[offset : offset + 4]
        (size,) = struct.unpack_from("<I", blob, offset + 4)
        body = blob[offset + 8 : offset + 8 + size]
        if chunk_id == b"fmt ":
            tag, channels, rate, _, _, bits = struct.unpack_from("<HHIIHH", body, 0)
            fmt_code = tag
            if tag == 0xFFFE and len(body) >= 26:
                # WAVE_FORMAT_EXTENSIBLE: the real format is the first two bytes
                # of the SubFormat GUID (offset 24), after cbSize/validbits/mask.
                (fmt_code,) = struct.unpack_from("<H", body, 24)
            if fmt_code != 3:
                raise ValueError(
                    f"{path.name}: expected IEEE float, got format {fmt_code} (tag {tag})")
        elif chunk_id == b"data":
            data = body
        # Chunks are word-aligned.
        offset += 8 + size + (size & 1)

    if data is None or channels == 0:
        raise ValueError(f"{path.name}: missing fmt or data chunk")
    if bits != 32:
        raise ValueError(f"{path.name}: expected 32-bit samples, got {bits}-bit")

    samples = np.frombuffer(data, dtype="<f4")
    usable = (len(samples) // channels) * channels
    return samples[:usable].reshape(-1, channels), rate, channels


def best_alignment(reference: np.ndarray, ours: np.ndarray) -> int:
    """Frames to drop from the head of `ours` to line it up with `reference`.

    The two decoders disagree about how much of LAME's encoder delay to trim, so
    the streams start at different points; see the note on gapless playback in
    the README.

    Alignment minimises squared error rather than maximising correlation. These
    fixtures are pure tones, and a sine correlates just as strongly against
    itself shifted by any whole number of periods — a dot-product search happily
    locks onto the wrong period and then reports a bogus mismatch.
    """
    span = min(len(reference), len(ours), 20_000)
    if span == 0:
        return 0
    head = reference[:span]
    limit = min(MAX_ALIGNMENT_SEARCH, max(1, len(ours) - span))
    best_offset, best_error = 0, np.inf
    for offset in range(limit):
        window = ours[offset : offset + span]
        if len(window) < span:
            break
        error = float(np.mean((window - head) ** 2))
        if error < best_error:
            best_offset, best_error = offset, error
    return best_offset


def compare(name: str) -> None:
    mp3 = FIXTURES / f"{name}.mp3"
    reference_path = FIXTURES / f"{name}.reference.wav"
    if not reference_path.exists():
        # Reference decodes are large and regenerated rather than committed.
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-i", str(mp3),
             "-f", "wav", "-acodec", "pcm_f32le", str(reference_path)],
            check=True,
        )

    ours_path = FIXTURES / f"{name}.pimp3.wav"
    subprocess.run([str(CLI), str(mp3), "--output", str(ours_path)], check=True, capture_output=True)

    ours, our_rate, our_channels = read_float_wav(ours_path)
    theirs, their_rate, their_channels = read_float_wav(reference_path)
    ours_path.unlink()

    check(our_rate == their_rate, f"{name}: sample rate {our_rate} matches ffmpeg")
    check(our_channels == their_channels, f"{name}: channel count {our_channels} matches ffmpeg")
    if our_channels != their_channels:
        return

    # Align on the first channel, then apply the same shift to all of them.
    offset = best_alignment(theirs[:, 0], ours[:, 0])
    aligned = ours[offset:]
    span = min(len(aligned), len(theirs))
    check(span > our_rate, f"{name}: {span / our_rate:.2f} s of overlapping audio to compare")
    if span <= 0:
        return

    a, b = aligned[:span], theirs[:span]
    residual_rms = float(np.sqrt(np.mean((a - b) ** 2)))
    signal_rms = float(np.sqrt(np.mean(b**2)))
    correlation = float(
        np.corrcoef(a[:, 0], b[:, 0])[0, 1]
    )

    check(
        residual_rms < MAX_RESIDUAL_RMS,
        f"{name}: residual RMS {residual_rms:.3e} below {MAX_RESIDUAL_RMS:.0e} (signal RMS {signal_rms:.3f})",
    )
    check(correlation > MIN_CORRELATION, f"{name}: correlation with ffmpeg {correlation:.6f}")


def main() -> int:
    if not CLI.exists():
        print(f"error: {CLI} not built. Run: cargo build --release -p pimp3-cli", file=sys.stderr)
        return 2

    print("pimp3 versus ffmpeg")
    for name in ("sine_440_mono_44100", "sine_stereo_44100", "sine_440_mono_22050"):
        compare(name)

    print()
    if failures:
        print(f"{len(failures)} comparison(s) FAILED")
        return 1
    print("pimp3 agrees with ffmpeg on every fixture")
    return 0


if __name__ == "__main__":
    sys.exit(main())
