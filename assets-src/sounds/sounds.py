"""The client's sound effects, synthesised: nothing recorded, nothing fetched.

    python3 assets-src/sounds/sounds.py

Each sound is a few decaying sine waves at the frequencies a struck piece of wood rings at (its
modes, not in harmony with each other, which is what makes it sound like wood rather than a
bell), over a very short burst of noise for the strike itself. Written as 16-bit WAV into
`target/sounds/`, then encoded as Ogg Vorbis into `assets/sounds/` with ffmpeg (on the host;
`-bitexact`, so the same script makes the same files). Only Python's standard library is used.
"""

import math
import random
import subprocess
import wave
from pathlib import Path

RATE = 44100
ROOT = Path(__file__).resolve().parents[2]
WORK = ROOT / "target" / "sounds"
OUT = ROOT / "assets" / "sounds"


def silence(seconds):
    return [0.0] * int(RATE * seconds)


def ring(out, at, modes):
    """Adds decaying modes, each (frequency in Hz, decay time in s, amplitude), from `at` s."""
    start = int(RATE * at)
    for freq, decay, amp in modes:
        phase = random.random() * math.tau
        n = min(len(out) - start, int(RATE * decay * 7))
        for i in range(n):
            t = i / RATE
            # A few samples' attack, so the start does not click.
            attack = min(1.0, i / 24)
            out[start + i] += attack * amp * math.exp(-t / decay) * math.sin(
                math.tau * freq * t + phase
            )


def strike(out, at, seconds, amp, brightness):
    """A burst of noise, low-passed (`brightness` 0 to 1: how much of the top it keeps)."""
    start = int(RATE * at)
    n = int(RATE * seconds)
    low = 0.0
    for i in range(min(n, len(out) - start)):
        low += brightness * (random.uniform(-1, 1) - low)
        out[start + i] += amp * low * math.exp(-i / (n / 4))


def normalised(samples, peak_db):
    peak = max(abs(s) for s in samples) or 1.0
    gain = 10 ** (peak_db / 20) / peak
    # The last 5 ms fade to nothing.
    fade = int(RATE * 0.005)
    out = [s * gain for s in samples]
    for i in range(fade):
        out[-1 - i] *= i / fade
    return out


def tick():
    """A piece picked up: a light, high tap of wood on wood, quiet."""
    out = silence(0.12)
    ring(out, 0.0, [(1870, 0.018, 1.0), (3150, 0.011, 0.55), (4720, 0.007, 0.3)])
    strike(out, 0.0, 0.003, 0.5, 0.6)
    return normalised(out, -14.0)


def clack():
    """A piece set down: a wooden piece on a wooden board, the board's body under its click."""
    out = silence(0.26)
    ring(
        out,
        0.0,
        [(205, 0.055, 0.7), (612, 0.045, 1.0), (1390, 0.03, 0.6), (2650, 0.018, 0.35)],
    )
    strike(out, 0.0, 0.005, 0.9, 0.35)
    return normalised(out, -7.0)


def chime():
    """A move played: a soft knock, then two notes of a marimba a fifth apart, rising."""
    out = silence(0.6)
    ring(out, 0.0, [(180, 0.05, 0.6), (530, 0.03, 0.4)])
    strike(out, 0.0, 0.004, 0.4, 0.3)
    for at, note in [(0.0, 523.25), (0.085, 783.99)]:
        # A marimba's bar: its fundamental, its fourth harmonic, and a little of its tenth.
        ring(out, at, [(note, 0.16, 1.0), (note * 3.93, 0.05, 0.3), (note * 9.2, 0.015, 0.08)])
    return normalised(out, -8.0)


def write(name, samples):
    WORK.mkdir(parents=True, exist_ok=True)
    OUT.mkdir(parents=True, exist_ok=True)
    wav = WORK / f"{name}.wav"
    with wave.open(str(wav), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes(
            b"".join(
                int(max(-1.0, min(1.0, s)) * 32767).to_bytes(2, "little", signed=True)
                for s in samples
            )
        )
    ogg = OUT / f"{name}.ogg"
    subprocess.run(
        [
            "ffmpeg", "-loglevel", "error", "-y", "-i", str(wav),
            "-c:a", "libvorbis", "-q:a", "5",
            "-fflags", "+bitexact", "-flags:a", "+bitexact", "-map_metadata", "-1",
            str(ogg),
        ],
        check=True,
    )
    print(f"{ogg.relative_to(ROOT)}: {len(samples) / RATE:.2f} s, {ogg.stat().st_size} bytes")


if __name__ == "__main__":
    for name, make in [("tick", tick), ("clack", clack), ("chime", chime)]:
        # Each its own noise, the same every time.
        random.seed(name)
        write(name, make())
