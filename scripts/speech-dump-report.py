#!/usr/bin/env python3
"""Show the level of the WAVs written by GAIA_SPEECH_DUMP, one line per half second.

Use it to see whether a fade is already in the bytes the server sent (`-source.wav`) or only in
what the desktop played (`-played.wav`):

    python scripts/speech-dump-report.py "%TEMP%\\gaia-speech-dump"

Only 16-bit PCM WAVs are read, which is what the dump writes. Files are paired by their name.
"""

import math
import struct
import sys
import wave
from pathlib import Path

WINDOW_SECONDS = 0.5


def rms_windows(path: Path) -> list[float]:
    with wave.open(str(path), "rb") as wav:
        channels = wav.getnchannels()
        width = wav.getsampwidth()
        rate = wav.getframerate()
        if width != 2:
            raise ValueError(f"{path.name}: expected 16-bit PCM, got {width * 8}-bit")
        frames = wav.getnframes()
        raw = wav.readframes(frames)

    samples = struct.unpack(f"<{len(raw) // 2}h", raw)
    if channels > 1:
        samples = [
            sum(samples[i : i + channels]) / channels
            for i in range(0, len(samples) - channels + 1, channels)
        ]

    size = max(1, int(rate * WINDOW_SECONDS))
    levels = []
    for start in range(0, len(samples), size):
        chunk = samples[start : start + size]
        if not chunk:
            continue
        rms = math.sqrt(sum(sample * sample for sample in chunk) / len(chunk))
        levels.append(20 * math.log10(rms / 32768) if rms > 0 else -math.inf)
    return levels


def line(levels: list[float]) -> str:
    cells = []
    for level in levels:
        if level == -math.inf:
            cells.append("  ----")
        else:
            cells.append(f"{level:6.1f}")
    return " ".join(cells)


def main() -> int:
    folder = Path(sys.argv[1] if len(sys.argv) > 1 else Path.home() / "AppData/Local/Temp/gaia-speech-dump")
    files = sorted(folder.glob("*.wav"))
    if not files:
        print(f"No WAVs in {folder}")
        return 1

    for path in files:
        levels = rms_windows(path)
        seconds = len(levels) * WINDOW_SECONDS
        print(f"\n{path.name}  ({seconds:.1f}s RMS per {WINDOW_SECONDS:g}s, dBFS)")
        print(line(levels))
    print("\nA healthy reply sits around -20 to -30 dBFS. A line that keeps falling towards ---- is a fade.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
