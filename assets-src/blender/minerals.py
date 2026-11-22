"""Polished minerals for stones, drawn with numpy alone: a texture's colour, in linear light,
for a stone mapped flat from above (`stones.py`). Each is the look of a mineral, not a copy of
one; numpy only, so a texture can be tried outside Blender:

    python3 -c "import minerals; ..."    (from assets-src/blender)

- Lapis lazuli: deep ultramarine, cloudy, with pale calcite blotches, a few thin pale veins (as
  sodalite has), and flecks of gold pyrite.
- Red jasper: brick red, mottled and fine-grained, with a few thin cream veins and darker
  hairline cracks.
- Tiger's eye: bands of gold, honey and dark brown, wavy, with a silky fibrous sheen along them.
- Black slate: near black, in faint layers of a lighter grey, finely grained.

`span_mm` is how many millimetres the texture covers across; every size below is in millimetres
on the stone, so a texture of another size draws the same stone."""

import numpy as np


def coords(size, span_mm):
    """Each texel's position in millimetres from the texture's middle: (x, y)."""
    t = ((np.arange(size) + 0.5) / size - 0.5) * span_mm
    return np.meshgrid(t, t)


def noise(rng, x, y, period):
    """Smooth value noise in -1..1 at (x, y) mm, varying over about `period` mm."""
    gx, gy = x / period, y / period
    ox, oy = np.floor(gx.min()) - 1, np.floor(gy.min()) - 1
    gx, gy = gx - ox, gy - oy
    n = int(max(gx.max(), gy.max())) + 3
    grid = rng.uniform(-1.0, 1.0, (n, n))
    ix, iy = np.floor(gx).astype(int), np.floor(gy).astype(int)
    fx, fy = gx - ix, gy - iy
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = grid[iy, ix], grid[iy, ix + 1]
    c, d = grid[iy + 1, ix], grid[iy + 1, ix + 1]
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy


def fbm(rng, x, y, period, octaves=4, gain=0.5):
    """Noise of several scales, the finer weaker: roughly -1..1."""
    total, weight, norm = 0.0, 1.0, 0.0
    for _ in range(octaves):
        total = total + weight * noise(rng, x, y, period)
        norm += weight
        period /= 2.0
        weight *= gain
    return total / norm


def smoothstep(lo, hi, v):
    t = np.clip((v - lo) / (hi - lo), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def mix(a, b, t):
    """Colours a and b (each (3,) or an image) mixed by t (an image)."""
    t = t[..., None]
    return np.asarray(a) * (1 - t) + np.asarray(b) * t


def veins(rng, x, y, period, width, stretch=3.0):
    """Thin lines running mostly one way, as cracks do: where a noise crosses zero, `width` (in
    noise units) wide. The noise is `stretch` times longer one way, turned a random way, so its
    zeros run along it rather than curling into loops."""
    turn = rng.uniform(0, np.pi)
    u = x * np.cos(turn) + y * np.sin(turn)
    w = -x * np.sin(turn) + y * np.cos(turn)
    v = fbm(rng, u / stretch, w, period, octaves=5, gain=0.55)
    return 1.0 - smoothstep(0.0, width, np.abs(v))


def lapis(size, span_mm, seed=21):
    rng = np.random.default_rng(seed)
    x, y = coords(size, span_mm)
    deep = np.array([0.010, 0.020, 0.17])
    bright = np.array([0.030, 0.060, 0.40])
    cloud = 0.5 + 0.5 * fbm(rng, x, y, 7.0, octaves=5)
    colour = mix(deep, bright, smoothstep(0.2, 0.8, cloud))
    # Calcite: pale grey-white blotches, soft at the edge, here and there.
    speckle = smoothstep(-0.2, 0.5, fbm(rng, x, y, 0.4, octaves=2))
    calcite = smoothstep(0.45, 0.68, fbm(rng, x, y, 3.0, octaves=5)) * speckle * 0.6
    colour = mix(colour, np.array([0.42, 0.46, 0.52]), calcite)
    # A few thin pale veins, as sodalite has, only in some places.
    where = smoothstep(0.2, 0.6, noise(rng, x, y, 9.0))
    colour = mix(colour, np.array([0.30, 0.36, 0.48]), veins(rng, x, y, 7.0, 0.02) * where * 0.7)
    # Pyrite: gold flecks, in small clusters, each a fraction of a millimetre.
    gold = np.zeros_like(x)
    for _ in range(46):
        cx, cy = rng.uniform(-span_mm / 2, span_mm / 2, 2)
        for _ in range(rng.integers(2, 9)):
            px, py = cx + rng.normal(0, 0.5), cy + rng.normal(0, 0.5)
            r = rng.uniform(0.03, 0.11)
            d = np.hypot(x - px, y - py)
            gold = np.maximum(gold, smoothstep(r, r * 0.4, d))
    colour = mix(colour, np.array([0.72, 0.52, 0.16]), gold)
    grain = rng.normal(0.0, 1.0, x.shape)
    return np.clip(colour * (1.0 + 0.04 * grain)[..., None], 0.0, 1.0)


def jasper(size, span_mm, seed=31):
    rng = np.random.default_rng(seed)
    x, y = coords(size, span_mm)
    dark = np.array([0.24, 0.026, 0.010])
    brick = np.array([0.45, 0.050, 0.016])
    orange = np.array([0.58, 0.100, 0.026])
    m = fbm(rng, x, y, 8.0, octaves=5)
    colour = mix(mix(dark, brick, smoothstep(-0.5, 0.0, m)), orange, smoothstep(0.15, 0.7, m))
    # Fine mottling, as a fine-grained stone has.
    speck = fbm(rng, x, y, 0.35, octaves=2)
    colour = colour * (1.0 + 0.10 * speck)[..., None]
    # A few cream veins, and darker hairline cracks.
    where = smoothstep(0.1, 0.5, noise(rng, x, y, 10.0))
    colour = mix(colour, np.array([0.60, 0.46, 0.32]), veins(rng, x, y, 9.0, 0.018) * where)
    colour = mix(colour, np.array([0.10, 0.015, 0.006]), veins(rng, x, y, 6.0, 0.01) * 0.5)
    grain = rng.normal(0.0, 1.0, x.shape)
    return np.clip(colour * (1.0 + 0.05 * grain)[..., None], 0.0, 1.0)


def tiger_eye(size, span_mm, seed=41):
    rng = np.random.default_rng(seed)
    x, y = coords(size, span_mm)
    # Across the bands (s) and along them (t), the bands turned and waving.
    turn = rng.uniform(0, np.pi)
    s = x * np.cos(turn) + y * np.sin(turn)
    t = -x * np.sin(turn) + y * np.cos(turn)
    s = s + 1.6 * fbm(rng, t, s * 0.3, 9.0, octaves=3)
    gold = np.array([0.50, 0.24, 0.030])
    honey = np.array([0.24, 0.095, 0.012])
    brown = np.array([0.035, 0.015, 0.005])
    # Bands a millimetre or two wide, with soft but definite edges.
    band = noise(rng, s, np.zeros_like(s), 1.5) + 0.4 * noise(rng, s, np.full_like(s, 7.0), 0.6)
    colour = mix(brown, honey, smoothstep(-0.35, -0.1, band))
    colour = mix(colour, gold, smoothstep(0.05, 0.35, band))
    # The silk: fine fibres along the bands, and a brighter sheen down the middle of each gold one.
    fibre = noise(rng, s * 14.0, t * 0.6, 1.0)
    sheen = smoothstep(0.25, 0.6, band)
    colour = colour * (1.0 + 0.12 * fibre + 0.25 * sheen)[..., None]
    return np.clip(colour, 0.0, 1.0)


def black_slate(size, span_mm, seed=51):
    rng = np.random.default_rng(seed)
    x, y = coords(size, span_mm)
    base = np.array([0.020, 0.021, 0.024])
    grey = np.array([0.048, 0.050, 0.055])
    # Faint layers along one way, waving, some stronger than others, fading in and out.
    s = y + 1.2 * fbm(rng, x, y, 10.0, octaves=3)
    layers = 0.5 + 0.5 * fbm(rng, s * 2.0, x * 0.08, 1.6, octaves=3)
    fade = 0.5 + 0.5 * noise(rng, x, y, 8.0)
    colour = mix(base, grey, 0.45 * smoothstep(0.55, 0.9, layers) * fade)
    mottle = fbm(rng, x, y, 5.0, octaves=4)
    grain = rng.normal(0.0, 1.0, x.shape)
    colour = colour * (1.0 + 0.15 * mottle + 0.06 * grain)[..., None]
    return np.clip(colour, 0.0, 1.0)


MINERALS = {
    "lapis": (1024, lapis),
    "jasper": (512, jasper),
    "tiger_eye": (1024, tiger_eye),
    "black_slate": (512, black_slate),
}
