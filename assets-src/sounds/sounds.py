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


def thump():
    """A piece set down: soft, as a die dropped on a felt-covered table. Mostly a muffled burst
    of noise over a low body that dies at once, with nothing left ringing, and a faint bounce."""
    out = silence(0.14)
    strike(out, 0.0, 0.016, 1.0, 0.07)
    ring(out, 0.0, [(140, 0.018, 0.9), (290, 0.009, 0.35)])
    # The bounce: a tenth as loud, a moment later.
    strike(out, 0.045, 0.008, 0.18, 0.06)
    ring(out, 0.045, [(160, 0.008, 0.12)])
    return normalised(out, -11.0)


def key():
    """A move played: a key pressed on a well-lubricated mechanical keyboard. A soft, rounded
    "thock" as it bottoms out (a muffled burst over the keycap's hollow body, a few hundred Hz,
    gone at once), the faintest tick of the stem, and, a moment later, the quieter return."""
    out = silence(0.16)
    # Down: the bottom-out.
    strike(out, 0.0, 0.007, 1.0, 0.12)
    ring(out, 0.0, [(420, 0.012, 0.8), (980, 0.006, 0.3), (2400, 0.002, 0.08)])
    # Up: the keycap back at the top, softer and a little higher.
    strike(out, 0.085, 0.004, 0.22, 0.15)
    ring(out, 0.085, [(560, 0.007, 0.2), (1300, 0.003, 0.06)])
    return normalised(out, -12.0)


class Resonator:
    """A two-pole band-pass filter: a formant of a voice, at `freq` Hz, `width` Hz wide."""

    def __init__(self, freq, width):
        self.r = math.exp(-math.pi * width / RATE)
        self.width = width
        self.set(freq)
        self.y1 = self.y2 = 0.0

    def set(self, freq):
        self.c1 = 2 * self.r * math.cos(math.tau * freq / RATE)
        self.c2 = -self.r * self.r
        self.gain = 1 - self.r

    def __call__(self, x):
        y = self.gain * x + self.c1 * self.y1 + self.c2 * self.y2
        self.y2, self.y1 = self.y1, y
        return y


def voice(out, at, seconds, pitch, contour, vowels, loudness):
    """One person in a crowd: a buzz at `pitch` Hz (a glottal pulse, with some breath in it)
    through three formants. `contour(f)` and `vowels(f)` say, for `f` from 0 to 1 through the
    sound, how far the pitch has moved (a factor) and which formants (F1, F2, F3) it has;
    `loudness(f)` its level. The pitch wanders a little, as nobody holds one still."""
    start = int(RATE * at)
    n = min(int(RATE * seconds), len(out) - start)
    formants = [Resonator(f, w) for f, w in zip(vowels(0.0), (90, 110, 160))]
    phase = random.random()
    wander = 0.0
    breath = random.uniform(0.1, 0.22)
    for i in range(n):
        f = i / n
        if i % 64 == 0:
            wander += random.uniform(-0.004, 0.004)
            wander *= 0.97
            for res, freq in zip(formants, vowels(f)):
                res.set(freq)
        hz = pitch * contour(f) * (1 + wander + 0.012 * math.sin(math.tau * 5.3 * i / RATE))
        phase = (phase + hz / RATE) % 1.0
        # A glottal pulse: a quick rise and a slower fall, then breath.
        pulse = (phase / 0.4 if phase < 0.4 else (1 - phase) / 0.6) - 0.5
        x = (1 - breath) * pulse + breath * random.uniform(-1, 1)
        y = formants[0](x) * 1.0 + formants[1](x) * 0.6 + formants[2](x) * 0.25
        out[start + i] += y * loudness(f)


def claps(out, start, end, count, amp):
    """Applause: many hands, each clap short and quiet, thickest early and thinning out."""
    for _ in range(count):
        at = start + (end - start) * random.random() ** 1.6
        strike(out, at, random.uniform(0.004, 0.009), amp * random.uniform(0.3, 1.0), 0.45)


def room(samples):
    """A little of a hall: four echoes feeding back (Schroeder's combs), mixed in quietly."""
    wet = [0.0] * len(samples)
    for delay_ms, feedback in [(29.7, 0.7), (37.1, 0.68), (41.1, 0.66), (43.7, 0.64)]:
        d = int(RATE * delay_ms / 1000)
        buf = [0.0] * len(samples)
        for i, x in enumerate(samples):
            buf[i] = x + (feedback * buf[i - d] if i >= d else 0.0)
            wet[i] += buf[i]
    return [x + 0.06 * w for x, w in zip(samples, wet)]


def lerp(a, b, f):
    return a + (b - a) * f


def crowd(out, people, seconds, contour, vowels, loudness, stagger):
    """A crowd: each person their own pitch (men's and women's), level, start and length."""
    for k in range(people):
        pitch = random.uniform(95, 150) if k % 2 else random.uniform(190, 290)
        at = random.uniform(0, stagger)
        length = seconds * random.uniform(0.75, 1.0) - at
        level = random.uniform(0.6, 1.0)
        voice(out, at, length, pitch, contour, vowels, lambda f, lv=level: lv * loudness(f))


def cheer():
    """The player won: a short cheer. A crowd's "yeah!" rising and falling away, with claps."""
    out = silence(1.7)
    crowd(
        out,
        22,
        1.6,
        # Up as it starts, as a shout of joy goes, then easing down.
        lambda f: 1.0 + 0.45 * math.sin(math.pi * min(1.0, f * 2.2)) - 0.15 * f,
        # "eh" to "ah".
        lambda f: (lerp(560, 760, min(1.0, f * 3)), lerp(1900, 1250, min(1.0, f * 3)), 2600),
        lambda f: min(1.0, f * 9) * (1 - f) ** 1.6,
        0.12,
    )
    claps(out, 0.12, 1.45, 260, 0.12)
    return normalised(room(out), -10.0)


def groan():
    """The player lost: a short groan. A crowd's "aww", falling, low and soft."""
    out = silence(1.5)
    crowd(
        out,
        20,
        1.4,
        # Down all the way, as disappointment goes.
        lambda f: 1.05 - 0.4 * f,
        # "aw" to "oh".
        lambda f: (lerp(640, 480, f), lerp(1000, 820, f), 2450),
        lambda f: min(1.0, f * 4) * (1 - f) ** 1.2,
        0.2,
    )
    return normalised(room(out), -12.0)


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
    for name, make in [("tick", tick), ("thump", thump), ("key", key), ("cheer", cheer), ("groan", groan)]:
        # Each its own noise, the same every time.
        random.seed(name)
        write(name, make())
