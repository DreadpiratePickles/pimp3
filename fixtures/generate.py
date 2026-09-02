#!/usr/bin/env python3
"""Build the MP3 test corpus and its reference decodes.

Every fixture is a signal whose correct decoding is known analytically: pure
tones at stated frequencies. That makes the tests assert something real — "the
audio is a 440 Hz sine" — rather than "some bytes came out".

LAME encodes; ffmpeg produces the reference PCM. Both are independent of the
decoder under test, so agreement is evidence rather than self-consistency.

Run with:  uv run --with numpy python3 fixtures/generate.py
"""
from __future__ import annotations

import struct
import subprocess
import sys
import wave
from pathlib import Path

import numpy as np

OUT = Path(__file__).parent
AMPLITUDE = 0.5
BITRATE_KBPS = "128"


def write_wav(path: Path, channels: list[np.ndarray], sample_rate_hz: int) -> None:
    """16-bit PCM source for the encoder."""
    stacked = np.stack(channels, axis=-1) if len(channels) > 1 else channels[0][:, None]
    pcm = np.clip(stacked, -1.0, 1.0)
    pcm = (pcm * 32767.0).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(len(channels))
        w.setsampwidth(2)
        w.setframerate(sample_rate_hz)
        w.writeframes(pcm.tobytes())


def tone(frequency_hz: float, seconds: float, sample_rate_hz: int) -> np.ndarray:
    t = np.arange(int(seconds * sample_rate_hz), dtype=np.float64) / sample_rate_hz
    return AMPLITUDE * np.sin(2.0 * np.pi * frequency_hz * t)


def encode_mp3(wav_path: Path, mp3_path: Path) -> None:
    subprocess.run(
        ["lame", "--quiet", "-b", BITRATE_KBPS, str(wav_path), str(mp3_path)],
        check=True,
    )


def reference_decode(mp3_path: Path, reference_path: Path) -> None:
    """ffmpeg's own decode, as 32-bit float WAV, for the numpy comparison."""
    subprocess.run(
        ["ffmpeg", "-y", "-loglevel", "error", "-i", str(mp3_path),
         "-f", "wav", "-acodec", "pcm_f32le", str(reference_path)],
        check=True,
    )


def build(name: str, channels: list[np.ndarray], sample_rate_hz: int) -> None:
    wav_path = OUT / f"{name}.src.wav"
    mp3_path = OUT / f"{name}.mp3"
    ref_path = OUT / f"{name}.reference.wav"
    write_wav(wav_path, channels, sample_rate_hz)
    encode_mp3(wav_path, mp3_path)
    reference_decode(mp3_path, ref_path)
    wav_path.unlink()  # the encoder input is not needed once the mp3 exists
    print(f"  {mp3_path.name:<26} {mp3_path.stat().st_size:>7,} B   ref {ref_path.stat().st_size:>8,} B")


def main() -> int:
    for tool in ("lame", "ffmpeg"):
        if subprocess.run(["which", tool], capture_output=True).returncode != 0:
            print(f"error: {tool} is required to regenerate fixtures", file=sys.stderr)
            return 2

    print("fixtures")
    # Mono 440 Hz at CD rate: the baseline case.
    build("sine_440_mono_44100", [tone(440.0, 2.0, 44100)], 44100)
    # Distinct tone per channel, so a channel-order or interleaving bug is visible.
    build("sine_stereo_44100", [tone(440.0, 2.0, 44100), tone(880.0, 2.0, 44100)], 44100)
    # A different sample rate, to catch hard-coded 44100 assumptions.
    build("sine_440_mono_22050", [tone(440.0, 1.5, 22050)], 22050)

    # A deliberately damaged stream: the decoder must skip the corrupt region
    # and keep going rather than abort at the first bad byte.
    source = (OUT / "sine_440_mono_44100.mp3").read_bytes()
    damaged = bytearray(source)
    midpoint = len(damaged) // 2
    damaged[midpoint : midpoint + 512] = b"\xa5" * 512
    (OUT / "damaged.mp3").write_bytes(bytes(damaged))
    print(f"  {'damaged.mp3':<26} {len(damaged):>7,} B   (512 bytes overwritten mid-stream)")

    # Not audio at all, for the rejection path.
    (OUT / "not_audio.bin").write_bytes(struct.pack("<4sI", b"RIFF", 0) + b"\x00" * 256)
    print(f"  {'not_audio.bin':<26} {260:>7,} B")
    return 0


if __name__ == "__main__":
    sys.exit(main())
