"""What the garden is made of (`textures/garden/`): scanned CC0 surfaces resized as `woods.py`
does the woods, and those no scan gives, drawn here with numpy.

Scanned: tatami, hinoki planks, linen, moss, cut stone, bark, a bamboo fence, and the rocks' own
(they come with their model). Each is `<name>_{albedo,normal,arm}.jpg`.

Drawn here:
- `gravel`: fine pale gravel raked into furrows, from a scan of gravel and a furrow's shape: eight
  furrows up the picture, 8 cm apart on a mesh that maps it over 64 cm. The mesh's mapping bends
  them round the rocks (`garden.py`);
- `water_normal`: ripples that repeat, to drift over the pond;
- `cane`: a bamboo cane's length, pale, for a tint to color: its joints, and the bloom below them;
- `hedge`: small leaves, packed, for clipped shrubs;
- `maple` and `bamboo`: a spray of leaves on a twig, cut out (PNG with alpha), for the cards the
  trees' foliage is made of;
- `treeline` and `hills`: what stands beyond the fence, as pictures to put on cards.

The same seed gives the same picture."""

import math

import numpy as np

import common

# name: (source prefix, source resolution, albedo size, normal size, arm size)
SCANS = {
    "tatami": ("tatami_mat", "2k", 2048, 1024, 1024),
    "hinoki": ("hinoki_planks", "2k", 2048, 1024, 1024),
    "linen": ("rough_linen", "1k", 512, 512, 512),
    "moss": ("sparse_grass", "2k", 1024, 1024, 512),
    "stone": ("mossy_rock", "2k", 1024, 1024, 512),
    "bark": ("trident_maple_bark", "1k", 1024, 512, 512),
    "fence": ("bamboo_wall", "2k", 1024, 1024, 512),
}

# The moss is a scan of thin grass on earth: greener and deeper, it is moss from a few metres.
MOSS_GRADE = np.array([0.55, 0.95, 0.42])

FURROWS = 8
# A furrow's depth against the width of the picture (64 cm): about a centimetre.
FURROW_DEPTH = 0.017


def out(name, kind, ext="jpg"):
    return common.output("textures", "garden", f"{name}_{kind}.{ext}")


def scan(prefix, kind, res):
    return common.read_image(common.source(f"{prefix}_{kind}_{res}.jpg"))[..., :3]


def scans():
    for name, (prefix, res, albedo_size, normal_size, arm_size) in SCANS.items():
        albedo = common.srgb_to_linear(scan(prefix, "diff", res))
        albedo = common.resize(albedo, albedo_size)
        if name == "moss":
            grey = albedo.mean(-1, keepdims=True)
            albedo = (grey * 0.35 + albedo * 0.65) * MOSS_GRADE * 1.25
        common.write_jpeg(out(name, "albedo"), common.linear_to_srgb(albedo), 88)
        common.write_jpeg(out(name, "normal"), common.resize(scan(prefix, "nor_gl", res), normal_size), 90)
        common.write_jpeg(out(name, "arm"), common.resize(scan(prefix, "arm", res), arm_size), 88)


def rocks():
    """The rocks' textures come with their model, with roughness alone where a scan has `arm`."""
    folder = "rock_moss_set_01/textures/rock_moss_set_01"
    albedo = common.srgb_to_linear(common.read_image(common.source(f"{folder}_diff_2k.jpg"))[..., :3])
    common.write_jpeg(out("rock", "albedo"), common.linear_to_srgb(common.resize(albedo, 2048)), 88)
    normal = common.read_image(common.source(f"{folder}_nor_gl_2k.jpg"))[..., :3]
    common.write_jpeg(out("rock", "normal"), common.resize(normal, 1024), 90)
    rough = common.resize(common.read_image(common.source(f"{folder}_rough_2k.jpg"))[..., :1], 512)
    arm = np.concatenate([np.ones_like(rough), rough, np.zeros_like(rough)], -1)
    common.write_jpeg(out("rock", "arm"), arm, 88)


def normal_from_height(height, strength):
    """A height field (rows down the picture, repeating) as a normal map, green up."""
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * 0.5 * height.shape[1]
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * 0.5 * height.shape[0]
    n = np.stack([-dx * strength, dy * strength, np.ones_like(height)], -1)
    return n / np.linalg.norm(n, axis=-1, keepdims=True)


def encode(normal):
    return normal * 0.5 + 0.5


def gravel():
    size = 1024
    albedo = common.resize(common.srgb_to_linear(scan("gravel_floor_02", "diff", "2k")), size)
    base = common.resize(scan("gravel_floor_02", "nor_gl", "2k"), size) * 2.0 - 1.0
    arm = common.resize(scan("gravel_floor_02", "arm", "2k"), size)

    v = (np.arange(size) + 0.5) / size
    # Rounded ridges with a narrower furrow between them, as a rake's teeth leave.
    wave = np.abs(np.sin(math.pi * FURROWS * v)) ** 0.7
    height = np.repeat((wave * FURROW_DEPTH)[:, None], size, 1)
    furrow = normal_from_height(height, 1.0)
    # The gravel's own relief, laid over the furrow's slope.
    n = np.stack(
        [base[..., 0] + furrow[..., 0], base[..., 1] + furrow[..., 1], base[..., 2] * furrow[..., 2]], -1
    )
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    common.write_jpeg(out("gravel", "normal"), encode(n), 90)

    # Paler than the scan (it is white gravel), and dimmer down in the furrows.
    shade = (0.55 + 0.45 * wave)[:, None, None]
    grey = albedo.mean(-1, keepdims=True)
    pale = (grey * 0.5 + albedo * 0.5) * 1.25 * np.array([1.0, 0.985, 0.95])
    common.write_jpeg(out("gravel", "albedo"), common.linear_to_srgb(pale * shade), 88)
    arm[..., 0] *= (0.6 + 0.4 * wave)[:, None]
    common.write_jpeg(out("gravel", "arm"), arm, 88)


def periodic_noise(rng, size, octaves):
    """Smooth noise that repeats: random waves whose counts across the picture are whole numbers."""
    t = (np.arange(size) + 0.5) / size * 2.0 * math.pi
    x, y = np.meshgrid(t, t)
    total = np.zeros((size, size))
    for lo, hi, weight, count in octaves:
        for _ in range(count):
            kx, ky = rng.integers(-hi, hi + 1), rng.integers(-hi, hi + 1)
            if max(abs(kx), abs(ky)) < lo:
                continue
            total += weight * np.sin(kx * x + ky * y + rng.uniform(0, 2 * math.pi))
    return total / np.abs(total).max()


def water():
    rng = np.random.default_rng(21)
    height = periodic_noise(rng, 512, [(1, 3, 1.0, 10), (3, 8, 0.4, 24), (8, 20, 0.12, 40)])
    common.write_jpeg(out("water", "normal"), encode(normal_from_height(height, 0.016)), 92)


def cane():
    """A cane's length runs up the picture, three joints of it; it goes round the cane across."""
    w, h = 64, 1024
    rng = np.random.default_rng(5)
    v = (np.arange(h) + 0.5) / h
    joint = (v * 3.0) % 1.0
    color = np.full((h, w, 3), 0.80)
    # Just above each joint the cane is pale with bloom; the joint itself is a dark ring under a
    # light one.
    bloom = np.exp(-joint / 0.10)
    ring = np.exp(-0.5 * ((joint - 0.012) / 0.006) ** 2)
    scar = np.exp(-0.5 * ((joint - 0.985) / 0.006) ** 2)
    shade = 0.82 + 0.22 * bloom + 0.25 * ring - 0.45 * scar
    streaks = 1.0 + 0.05 * np.repeat(rng.normal(0, 1, (1, w)), h, 0)
    color *= (shade[:, None] * streaks)[..., None]
    common.write_jpeg(out("cane", "albedo"), np.clip(color, 0, 1), 90)


def stamp(color, height, cy, cx, ry, rx, angle, tint, top):
    """A leaf as an ellipse, stamped into a picture that repeats: over what is lower than it."""
    size = color.shape[0]
    r = int(max(ry, rx)) + 2
    ys, xs = np.mgrid[-r : r + 1, -r : r + 1]
    c, s = math.cos(angle), math.sin(angle)
    u = (xs * c + ys * s) / rx
    v = (-xs * s + ys * c) / ry
    d = u * u + v * v
    rows = (ys + int(cy)) % size
    cols = (xs + int(cx)) % size
    # Domed, with a crease along the midrib.
    z = top + 0.5 * (1.0 - d) - 0.12 * np.exp(-((v * 3.0) ** 2))
    mask = (d < 1.0) & (z > height[rows, cols])
    shade = (0.75 + 0.25 * (1.0 - d)) * (1.0 - 0.25 * np.exp(-((v * 5.0) ** 2)))
    color[rows[mask], cols[mask]] = tint * shade[mask][:, None]
    height[rows[mask], cols[mask]] = z[mask]


def hedge():
    size = 512
    rng = np.random.default_rng(9)
    color = np.tile(np.array([0.010, 0.022, 0.008]), (size, size, 1))
    height = np.zeros((size, size))
    for i in range(5200):
        top = i / 5200.0
        green = np.array([0.045, 0.115, 0.030]) * rng.uniform(0.55, 1.35)
        green[0] *= rng.uniform(0.7, 1.6)
        stamp(
            color,
            height,
            rng.uniform(0, size),
            rng.uniform(0, size),
            rng.uniform(9, 15),
            rng.uniform(4.5, 7),
            rng.uniform(0, math.pi),
            green,
            top * 2.0,
        )
    # Deeper between the leaves.
    depth = (height - height.min()) / (height.max() - height.min())
    color *= (0.35 + 0.65 * depth)[..., None]
    common.write_jpeg(out("hedge", "albedo"), common.linear_to_srgb(color * 2.2), 88)
    common.write_jpeg(out("hedge", "normal"), encode(normal_from_height(height, 0.004)), 90)


def fill_polygon(color, alpha, points, tint, shade=None):
    """Fills a polygon (points as (x, y) in pixels) into an RGBA picture, with soft edges."""
    pts = np.asarray(points, np.float64)
    size = alpha.shape[0]
    x0, y0 = np.floor(pts.min(0)).astype(int) - 1
    x1, y1 = np.ceil(pts.max(0)).astype(int) + 2
    x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, size), min(y1, size)
    if x1 <= x0 or y1 <= y0:
        return
    # Four samples a pixel.
    cover = np.zeros((y1 - y0, x1 - x0))
    for oy in (0.25, 0.75):
        for ox in (0.25, 0.75):
            ys, xs = np.mgrid[y0:y1, x0:x1]
            px, py = xs + ox, ys + oy
            hit = np.zeros(px.shape, bool)
            for (ax, ay), (bx, by) in zip(pts, np.roll(pts, -1, 0)):
                if ay == by:
                    continue
                crosses = (ay > py) != (by > py)
                at = ax + (py - ay) / (by - ay) * (bx - ax)
                hit ^= crosses & (px < at)
            cover += hit
    cover /= 4.0
    region = (slice(y0, y1), slice(x0, x1))
    c = np.asarray(tint)[None, None, :] * (1.0 if shade is None else shade[region][..., None])
    color[region] = color[region] * (1.0 - cover[..., None]) + c * cover[..., None]
    alpha[region] = np.maximum(alpha[region], cover)


def maple_leaf(cx, cy, size, angle, rng):
    """A maple leaf's outline: seven pointed lobes, the middle one longest, and its stalk's end."""
    lobes = [(0.0, 1.0), (40.0, 0.88), (80.0, 0.66), (122.0, 0.40)]
    tips = [(-a, l) for a, l in reversed(lobes[1:])] + lobes
    pts = []
    for i, (a, l) in enumerate(tips):
        l *= rng.uniform(0.9, 1.08)
        rad = math.radians(a)
        if i > 0:
            # The notch between this lobe and the last.
            mid = math.radians((a + tips[i - 1][0]) / 2.0)
            pts.append((0.30 * math.sin(mid), 0.30 * math.cos(mid)))
        # A lobe: narrow, widest a third of the way along.
        for da, dl in ((-7.0, 0.55), (0.0, 1.0), (7.0, 0.55)):
            r = math.radians(a + da)
            pts.append((l * dl * math.sin(r), l * dl * math.cos(r)))
    pts.append((0.0, -0.12))
    c, s = math.cos(angle), math.sin(angle)
    return [(cx + size * (x * c - y * s), cy + size * (x * s + y * c)) for x, y in pts]


def bamboo_leaf(cx, cy, size, angle):
    """A bamboo leaf: long and narrow, from its stalk at (cx, cy)."""
    pts = []
    for t, w in ((0.0, 0.0), (0.12, 0.07), (0.35, 0.10), (0.65, 0.075), (1.0, 0.0)):
        pts.append((w, t))
    pts += [(-w, t) for w, t in reversed(pts[1:-1])]
    c, s = math.cos(angle), math.sin(angle)
    return [(cx + size * (x * c - y * s), cy + size * (x * s + y * c)) for x, y in pts]


def line(color, alpha, a, b, width, tint):
    a, b = np.asarray(a, float), np.asarray(b, float)
    d = b - a
    n = np.array([-d[1], d[0]]) / max(np.linalg.norm(d), 1e-6) * width / 2.0
    fill_polygon(color, alpha, [a - n, b - n, b + n, a + n], tint)


def bleed(color, alpha, fill):
    """Gives what is cut away the leaves' own color, so smaller mip levels do not darken the
    leaves' edges with what was never seen."""
    color[...] = color * alpha[..., None] + np.asarray(fill)[None, None, :] * (1.0 - alpha[..., None])


def spray(name, size, seed, make):
    rng = np.random.default_rng(seed)
    color = np.zeros((size, size, 3))
    alpha = np.zeros((size, size))
    fill = make(rng, color, alpha, size)
    bleed(color, alpha, fill)
    rgba = np.concatenate([common.linear_to_srgb(color), alpha[..., None]], -1)
    common.write_png(out(name, "albedo", "png"), rgba)


# Autumn: mostly deep red and scarlet, some orange, a few gone gold (linear light).
MAPLE = [
    ((0.42, 0.030, 0.020), 5),
    ((0.55, 0.060, 0.022), 5),
    ((0.62, 0.130, 0.025), 3),
    ((0.70, 0.280, 0.040), 2),
    ((0.30, 0.020, 0.025), 2),
]


def maple(rng, color, alpha, size):
    twig = (0.10, 0.07, 0.05)
    # A twig from the bottom middle, forking, its leaves in pairs along it.
    stems = []

    def grow(at, angle, length, depth):
        end = (at[0] + length * math.sin(angle), at[1] - length * math.cos(angle))
        line(color, alpha, at, end, 3.0 + depth * 2.0, twig)
        steps = 5
        for i in range(1, steps + 1):
            t = i / steps
            p = (at[0] + (end[0] - at[0]) * t, at[1] + (end[1] - at[1]) * t)
            stems.append((p, angle))
        if depth > 0:
            for turn in (-0.55, 0.5):
                start = (at[0] + (end[0] - at[0]) * 0.55, at[1] + (end[1] - at[1]) * 0.55)
                grow(start, angle + turn * rng.uniform(0.8, 1.3), length * 0.62, depth - 1)

    grow((size * 0.5, size * 0.99), 0.0, size * 0.62, 3)
    palette = [c for c, weight in MAPLE for _ in range(weight)]
    rng.shuffle(stems)
    for (p, angle) in stems:
        for side in (-1.0, 1.0):
            if rng.random() < 0.12:
                continue
            a = angle + side * rng.uniform(0.5, 1.4)
            stalk = size * rng.uniform(0.035, 0.07)
            base = (p[0] + stalk * math.sin(a), p[1] - stalk * math.cos(a))
            line(color, alpha, p, base, 1.6, twig)
            tint = np.array(palette[rng.integers(len(palette))]) * rng.uniform(0.75, 1.25)
            leaf = size * rng.uniform(0.075, 0.115)
            # Upside down in the picture: y grows downwards, and a leaf points away from its stalk.
            turn = math.pi - a + rng.uniform(-0.3, 0.3)
            pts = maple_leaf(base[0], base[1], leaf, turn, rng)
            fill_polygon(color, alpha, pts, tint)
            # The midrib of each of the three long lobes, a little lighter.
            for da in (-0.7, 0.0, 0.7):
                tip = (
                    base[0] + leaf * 0.5 * math.sin(a + da * 1.0),
                    base[1] - leaf * 0.5 * math.cos(a + da * 1.0),
                )
                line(color, alpha, base, tip, 1.0, tint * 1.3)
    return (0.50, 0.07, 0.022)


def bamboo(rng, color, alpha, size):
    twig = (0.16, 0.20, 0.07)
    # Thin stalks from the bottom middle, each ending in a fan of leaves.
    for k in range(7):
        angle = rng.uniform(-0.9, 0.9)
        length = size * rng.uniform(0.35, 0.62)
        at = (size * 0.5, size * 0.98)
        end = (at[0] + length * math.sin(angle), at[1] - length * math.cos(angle))
        line(color, alpha, at, end, 3.0, twig)
        for t in (0.55, 0.8, 1.0):
            p = (at[0] + (end[0] - at[0]) * t, at[1] + (end[1] - at[1]) * t)
            for _ in range(4 if t == 1.0 else 2):
                a = angle + rng.uniform(-1.1, 1.1)
                green = np.array([0.10, 0.22, 0.045]) * rng.uniform(0.6, 1.3)
                green[0] *= rng.uniform(0.8, 1.7)
                leaf = size * rng.uniform(0.20, 0.30)
                fill_polygon(color, alpha, bamboo_leaf(p[0], p[1], leaf, math.pi - a), green)
                tip = (p[0] + leaf * 0.8 * math.sin(a), p[1] - leaf * 0.8 * math.cos(a))
                line(color, alpha, p, tip, 1.0, green * 1.25)
    return (0.10, 0.20, 0.05)


def treeline():
    """Trees seen over the fence, their crowns against the sky: a strip that repeats sideways,
    lit from the right as the sun is. Three ranks, the furthest palest; most are evergreen, some
    have turned. The crowns fill the strip's lower half: its foot stands at the fence's top."""
    w, h = 2048, 512
    rng = np.random.default_rng(31)
    color = np.zeros((h, w, 3))
    alpha = np.zeros((h, w))
    ys, xs = np.mgrid[0:h, 0:w]
    turned = [(0.30, 0.10, 0.02), (0.36, 0.20, 0.03), (0.22, 0.05, 0.02)]
    for rank, (base, tint) in enumerate(
        (
            (0.52, (0.105, 0.150, 0.130)),
            (0.70, (0.050, 0.095, 0.050)),
            (0.88, (0.022, 0.052, 0.020)),
        )
    ):
        count = 40 + rank * 10
        for _ in range(count):
            cx = rng.uniform(0, w)
            tall = rng.random() < 0.35
            ry = h * (rng.uniform(0.17, 0.30) if tall else rng.uniform(0.09, 0.16))
            rx = ry * (rng.uniform(0.35, 0.55) if tall else rng.uniform(0.9, 1.5))
            cy = h * (base + rng.uniform(-0.05, 0.05)) - (ry * 0.5 if tall else 0.0)
            own = np.asarray(tint) * rng.uniform(0.7, 1.3)
            if rank > 0 and rng.random() < 0.14:
                own = np.asarray(turned[rng.integers(len(turned))]) * (0.5 if rank == 1 else 0.75)
            # A crown is a few overlapping lumps, each of smaller clumps of leaves.
            for _ in range(7):
                ox, oy = rng.uniform(-0.55, 0.55) * rx, rng.uniform(-0.45, 0.45) * ry
                lx, ly = rx * rng.uniform(0.4, 0.7), ry * rng.uniform(0.4, 0.7)
                for wrap in (-w, 0, w):
                    x0, x1 = int(cx + ox + wrap - lx) - 1, int(cx + ox + wrap + lx) + 2
                    y0, y1 = int(cy + oy - ly) - 1, int(cy + oy + ly) + 2
                    x0, x1, y0, y1 = max(x0, 0), min(x1, w), max(y0, 0), min(y1, h)
                    if x1 <= x0 or y1 <= y0:
                        continue
                    region = (slice(y0, y1), slice(x0, x1))
                    dx = (xs[region] - (cx + ox + wrap)) / lx
                    dy = (ys[region] - (cy + oy)) / ly
                    inside = dx * dx + dy * dy < 1.0
                    # Lit from the upper right, dark underneath.
                    light = np.clip(0.70 + 0.45 * (dx * 0.55 - dy * 0.75), 0.35, 1.45)
                    c = own[None, None, :] * light[..., None]
                    color[region][inside] = c[inside]
                    alpha[region][inside] = 1.0
        # All below a rank's crowns is its trees, in shadow.
        floor = ys > h * (base + 0.04)
        color[floor] = np.asarray(tint) * 0.55
        alpha[floor] = 1.0
    # Leafy, not smooth: mottled at two sizes.
    fine = rng.normal(0, 1, (h // 2, w // 2)).repeat(2, 0).repeat(2, 1)
    coarse = rng.normal(0, 1, (h // 8, w // 8)).repeat(8, 0).repeat(8, 1)
    color *= (1.0 + 0.20 * fine + 0.16 * coarse)[..., None]
    bleed(color, alpha, (0.05, 0.09, 0.06))
    rgba = np.concatenate([common.linear_to_srgb(color), alpha[..., None]], -1)
    common.write_png(out("treeline", "albedo", "png"), rgba)


def hills():
    """Far hills in the mist: two ridges, the nearer darker, as a strip that repeats sideways."""
    w, h = 2048, 256
    rng = np.random.default_rng(41)
    color = np.zeros((h, w, 3))
    alpha = np.zeros((h, w))
    ys = np.arange(h)[:, None]
    t = np.arange(w) / w * 2.0 * math.pi
    for base, height, tint in ((0.50, 0.34, (0.42, 0.50, 0.55)), (0.72, 0.22, (0.25, 0.33, 0.36))):
        ridge = np.zeros(w)
        for k in range(1, 14):
            ridge += rng.uniform(0.3, 1.0) / k * np.sin(k * t + rng.uniform(0, 2 * math.pi))
        ridge = h * (base - height * (ridge / np.abs(ridge).max() * 0.5 + 0.5))
        below = ys > ridge[None, :]
        # Paler lower down, where the mist lies.
        fade = np.clip((ys - ridge[None, :]) / (h * 0.5), 0.0, 1.0)
        c = np.asarray(tint)[None, None, :] * (1.0 - 0.0 * fade[..., None]) + 0.25 * fade[..., None]
        color[below] = c[below]
        alpha[below] = 1.0
    bleed(color, alpha, (0.40, 0.48, 0.52))
    rgba = np.concatenate([common.linear_to_srgb(color), alpha[..., None]], -1)
    common.write_png(out("hills", "albedo", "png"), rgba)


def run():
    scans()
    rocks()
    gravel()
    water()
    cane()
    hedge()
    spray("maple", 1024, 3, maple)
    spray("bamboo", 512, 4, bamboo)
    treeline()
    hills()
