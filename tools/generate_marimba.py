#!/usr/bin/env python3
"""Generate or verify Go Together's original audio samples: the marimba-style
notes and the Round 0 duplicate-selection clack."""

from __future__ import annotations

import argparse
import math
import random
import struct
import tempfile
import wave
from pathlib import Path

SAMPLE_RATE = 44_100
DURATION_SECONDS = 0.46
CLACK_DURATION_SECONDS = 0.18
PEAK = 0.78
NOTES = {
    "a3": 220.000,
    "b3": 246.942,
    "c4": 261.626,
    "d4": 293.665,
    "e4": 329.628,
    "g4": 391.995,
    "a4": 440.000,
    "c5": 523.251,
    "d5": 587.330,
    "e5": 659.255,
    "g5": 783.991,
}


def synthesize(frequency: float, seed: int) -> list[float]:
    rng = random.Random(seed)
    samples: list[float] = []
    total = int(SAMPLE_RATE * DURATION_SECONDS)
    fade_out_start = DURATION_SECONDS - 0.025

    for index in range(total):
        time = index / SAMPLE_RATE
        attack = min(1.0, time / 0.0025)
        fade_out = min(1.0, max(0.0, (DURATION_SECONDS - time) / 0.025))
        fundamental = math.sin(2.0 * math.pi * frequency * time) * math.exp(-8.2 * time)
        second_mode = (
            0.42
            * math.sin(2.0 * math.pi * frequency * 3.99 * time + 0.35)
            * math.exp(-14.0 * time)
        )
        third_mode = (
            0.18
            * math.sin(2.0 * math.pi * frequency * 9.1 * time + 1.1)
            * math.exp(-22.0 * time)
        )
        warmth = (
            0.12
            * math.sin(2.0 * math.pi * frequency * 2.01 * time)
            * math.exp(-10.5 * time)
        )
        transient = rng.uniform(-1.0, 1.0) * 0.16 * math.exp(-85.0 * time)
        sample = (fundamental + second_mode + third_mode + warmth + transient) * attack
        if time >= fade_out_start:
            sample *= fade_out
        samples.append(sample)

    scale = PEAK / max(abs(sample) for sample in samples)
    return [sample * scale for sample in samples]


def synthesize_opening_clack() -> list[float]:
    """Create two close, inharmonic wooden impacts for duplicate feedback."""
    rng = random.Random(91_207)
    samples: list[float] = []
    total = int(SAMPLE_RATE * CLACK_DURATION_SECONDS)
    second_impact = 0.036
    fade_out_start = CLACK_DURATION_SECONDS - 0.012

    for index in range(total):
        time = index / SAMPLE_RATE
        sample = 0.0
        for onset, pitch, strength in [(0.0, 735.0, 1.0), (second_impact, 910.0, 0.72)]:
            local = time - onset
            if local < 0.0:
                continue
            attack = min(1.0, local / 0.0008)
            body = math.sin(2.0 * math.pi * pitch * local) * math.exp(-34.0 * local)
            upper = (
                0.46
                * math.sin(2.0 * math.pi * pitch * 2.73 * local + 0.4)
                * math.exp(-52.0 * local)
            )
            knock = rng.uniform(-1.0, 1.0) * 0.34 * math.exp(-120.0 * local)
            sample += strength * (body + upper + knock) * attack
        if time >= fade_out_start:
            sample *= max(0.0, (CLACK_DURATION_SECONDS - time) / 0.012)
        samples.append(sample)

    scale = PEAK / max(abs(sample) for sample in samples)
    return [sample * scale for sample in samples]


def write_wav(path: Path, samples: list[float]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    pcm = b"".join(
        struct.pack("<h", round(max(-1.0, min(1.0, sample)) * 32_767))
        for sample in samples
    )
    with wave.open(str(path), "wb") as output:
        output.setnchannels(1)
        output.setsampwidth(2)
        output.setframerate(SAMPLE_RATE)
        output.writeframes(pcm)


def generate(output: Path, *, announce: bool) -> None:
    for index, (name, frequency) in enumerate(NOTES.items()):
        write_wav(output / f"marimba_{name}.wav", synthesize(frequency, 19_031 + index))
        if announce:
            print(f"generated {name}: {frequency:.3f} Hz")
    write_wav(output / "opening_duplicate_clack.wav", synthesize_opening_clack())
    if announce:
        print("generated opening duplicate clack")


def check_assets(output: Path) -> bool:
    with tempfile.TemporaryDirectory(prefix="go-together-audio-") as temporary:
        generated = Path(temporary)
        generate(generated, announce=False)
        expected = {f"marimba_{name}.wav" for name in NOTES}
        expected.add("opening_duplicate_clack.wav")
        existing = {path.name for path in output.glob("*.wav")}
        stale = sorted(expected ^ existing)
        stale.extend(
            sorted(
                filename
                for filename in expected & existing
                if (output / filename).read_bytes()
                != (generated / filename).read_bytes()
            )
        )
    if stale:
        for filename in stale:
            print(f"out of date: assets/audio/{filename}")
        return False
    print("audio assets are up to date")
    return True


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check",
        action="store_true",
        help="verify committed WAV files without modifying them",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    root = Path(__file__).resolve().parents[1]
    output = root / "assets" / "audio"
    if args.check:
        return 0 if check_assets(output) else 1
    generate(output, announce=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
