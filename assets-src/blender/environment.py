"""The room's light: an HDR panorama turned into the two cubemaps Bevy's `EnvironmentMapLight`
reads, filtered here once rather than by the GPU every frame.

- Specular: one mip level per roughness, level i for perceptual roughness i / (levels - 1), each
  the panorama convolved with GGX (importance sampled, reading a blurrier copy of the panorama
  for each sample that stands for a wider cone, so few samples are enough and it stays smooth).
- Diffuse: the panorama's irradiance from nine spherical harmonics, divided by pi, as Bevy (and the
  glTF IBL sampler it was made for) expects.

Before filtering, the panorama is graded for a table top (`grade`):
- below the horizon is the room's floor, which the board would reflect where in the scene there
  is a dark table: it is made dim, the colour of the felt in shadow;
- its colours are halved in saturation, so a stone reflects a window as a highlight and not as
  the green of the trees outside it.

Bevy samples a cubemap with the world direction's z negated, so face texels are given the world
direction (x, y, -z) of the usual cube direction."""

import math

import numpy as np

import common

SPECULAR_SIZE = 256
DIFFUSE_SIZE = 32
SAMPLES = 128
# How much colour the room keeps, and how bright the table below the horizon is, against the
# average of the room above it.
SATURATION = 0.5
TABLE = np.array([0.05, 0.065, 0.065])


def grade(pixels):
    """The panorama made fit for a table top (see the module's notes)."""
    img = pixels[..., :3].astype(np.float32)
    grey = (img @ np.array([0.2126, 0.7152, 0.0722], np.float32))[..., None]
    img = grey + (img - grey) * SATURATION
    h = img.shape[0]
    lat = (0.5 - (np.arange(h) + 0.5) / h) * 180.0
    upper = img[lat > 0].reshape(-1, 3).mean(0)
    table = (TABLE * upper.mean()).astype(np.float32)
    # Fully the table from 3 degrees below the horizon; the horizon itself is kept.
    t = np.clip(-lat / 3.0, 0.0, 1.0)
    t = (t * t * (3.0 - 2.0 * t))[:, None, None]
    return img * (1.0 - t) + table[None, None, :] * t


def cube_directions(size):
    """World directions of each face's texel centres: (6, size, size, 3), faces +X -X +Y -Y +Z -Z."""
    t = (np.arange(size) + 0.5) / size * 2.0 - 1.0
    u, v = np.meshgrid(t, t)
    one = np.ones_like(u)
    faces = [
        (one, -v, -u),
        (-one, -v, u),
        (u, one, v),
        (u, -one, -v),
        (u, -v, one),
        (-u, -v, -one),
    ]
    dirs = np.stack([np.stack(f, -1) for f in faces])
    dirs[..., 2] *= -1.0
    return dirs / np.linalg.norm(dirs, axis=-1, keepdims=True)


class Panorama:
    """An equirectangular HDR image with a pyramid of blurrier copies, sampled by direction."""

    def __init__(self, pixels):
        self.levels = [pixels[..., :3].astype(np.float32)]
        while self.levels[-1].shape[0] > 1:
            p = self.levels[-1]
            h, w = p.shape[0] // 2 * 2, p.shape[1] // 2 * 2
            p = p[:h, :w]
            self.levels.append(0.25 * (p[0::2, 0::2] + p[1::2, 0::2] + p[0::2, 1::2] + p[1::2, 1::2]))
        h, w = self.levels[0].shape[:2]
        # The solid angle of one texel at the equator of the full-size image.
        self.texel_angle = (2.0 * math.pi / w) * (math.pi / h)

    def _bilinear(self, level, dirs):
        img = self.levels[level]
        h, w = img.shape[:2]
        lon = np.arctan2(dirs[..., 0], -dirs[..., 2])
        lat = np.arcsin(np.clip(dirs[..., 1], -1.0, 1.0))
        x = (0.5 + lon / (2.0 * math.pi)) * w - 0.5
        y = (0.5 - lat / math.pi) * h - 0.5
        x0 = np.floor(x).astype(np.int64)
        y0 = np.floor(y).astype(np.int64)
        fx = (x - x0)[..., None]
        fy = (y - y0)[..., None]
        xa, xb = x0 % w, (x0 + 1) % w
        ya, yb = np.clip(y0, 0, h - 1), np.clip(y0 + 1, 0, h - 1)
        top = img[ya, xa] * (1 - fx) + img[ya, xb] * fx
        bottom = img[yb, xa] * (1 - fx) + img[yb, xb] * fx
        return top * (1 - fy) + bottom * fy

    def sample(self, dirs, lod):
        """Samples every direction at one level of detail (a float, between pyramid levels)."""
        lod = min(max(lod, 0.0), len(self.levels) - 1.0)
        a = int(math.floor(lod))
        b = min(a + 1, len(self.levels) - 1)
        t = lod - a
        out = self._bilinear(a, dirs)
        if t > 0.0 and b != a:
            out = out * (1.0 - t) + self._bilinear(b, dirs) * t
        return out


def hammersley(i, n):
    bits = i
    bits = ((bits << 16) | (bits >> 16)) & 0xFFFFFFFF
    bits = (((bits & 0x55555555) << 1) | ((bits & 0xAAAAAAAA) >> 1)) & 0xFFFFFFFF
    bits = (((bits & 0x33333333) << 2) | ((bits & 0xCCCCCCCC) >> 2)) & 0xFFFFFFFF
    bits = (((bits & 0x0F0F0F0F) << 4) | ((bits & 0xF0F0F0F0) >> 4)) & 0xFFFFFFFF
    bits = (((bits & 0x00FF00FF) << 8) | ((bits & 0xFF00FF00) >> 8)) & 0xFFFFFFFF
    return i / n, bits / 4294967296.0


def specular_level(pano, size, roughness):
    normals = cube_directions(size)
    if roughness == 0.0:
        # A mirror: the panorama itself, each texel averaged over its own footprint.
        texel = 2.0 / size
        lod = math.log2(max(texel * texel / pano.texel_angle, 1.0)) / 2.0
        return pano.sample(normals, lod)
    alpha = roughness * roughness
    a2 = alpha * alpha
    up = np.where(np.abs(normals[..., 1:2]) < 0.999, [0.0, 1.0, 0.0], [1.0, 0.0, 0.0])
    tangent = np.cross(up, normals)
    tangent /= np.linalg.norm(tangent, axis=-1, keepdims=True)
    bitangent = np.cross(normals, tangent)

    total = np.zeros(normals.shape, np.float64)
    weight = 0.0
    for i in range(SAMPLES):
        u1, u2 = hammersley(i, SAMPLES)
        phi = 2.0 * math.pi * u1
        cos_t = math.sqrt((1.0 - u2) / (1.0 + (a2 - 1.0) * u2))
        sin_t = math.sqrt(1.0 - cos_t * cos_t)
        n_dot_l = 2.0 * cos_t * cos_t - 1.0
        if n_dot_l <= 0.0:
            continue
        half = (
            tangent * (sin_t * math.cos(phi))
            + bitangent * (sin_t * math.sin(phi))
            + normals * cos_t
        )
        light = 2.0 * cos_t * half - normals
        # The cone this sample stands for, against a texel of the full-size panorama.
        d = a2 / (math.pi * ((cos_t * cos_t * (a2 - 1.0) + 1.0) ** 2))
        pdf = d / 4.0
        cone = 1.0 / (SAMPLES * pdf)
        lod = 0.5 * math.log2(max(cone / pano.texel_angle, 1.0)) + 1.0
        total += pano.sample(light, lod) * n_dot_l
        weight += n_dot_l
    return (total / weight).astype(np.float32)


def sh9(d):
    x, y, z = d[..., 0], d[..., 1], d[..., 2]
    return np.stack(
        [
            0.282095 * np.ones_like(x),
            0.488603 * y,
            0.488603 * z,
            0.488603 * x,
            1.092548 * x * y,
            1.092548 * y * z,
            0.315392 * (3.0 * z * z - 1.0),
            1.092548 * x * z,
            0.546274 * (x * x - y * y),
        ],
        -1,
    )


def irradiance(pano, size):
    """The diffuse map, and the direction most light comes from."""
    img = pano.levels[3]
    h, w = img.shape[:2]
    lon = ((np.arange(w) + 0.5) / w - 0.5) * 2.0 * math.pi
    lat = (0.5 - (np.arange(h) + 0.5) / h) * math.pi
    lon, lat = np.meshgrid(lon, lat)
    dirs = np.stack([np.cos(lat) * np.sin(lon), np.sin(lat), -np.cos(lat) * np.cos(lon)], -1)
    solid = (2.0 * math.pi / w) * (math.pi / h) * np.cos(lat)
    basis = sh9(dirs)
    coeffs = np.einsum("hwk,hwc,hw->kc", basis, img, solid)
    band = np.array([math.pi] + [2.0 * math.pi / 3.0] * 3 + [math.pi / 4.0] * 5)[:, None]
    normals = cube_directions(size)
    diffuse = np.einsum("fyxk,kc->fyxc", sh9(normals), coeffs * band) / math.pi
    luminance = coeffs @ np.array([0.2126, 0.7152, 0.0722])
    main = np.array([luminance[3], luminance[1], luminance[2]])
    return np.maximum(diffuse, 0.0).astype(np.float32), main / np.linalg.norm(main)


def build(name, source_file):
    return build_from(name, grade(common.read_image(common.source(source_file))))


def build_from(name, pixels, specular_gain=1.0):
    """The two cubemaps of a panorama already fit to use: its centre is the far side of the
    table, and it goes round to the right. `specular_gain` dims what polished things reflect
    against the light they are lit by."""
    pano = Panorama(pixels)
    levels = []
    count = int(math.log2(SPECULAR_SIZE)) + 1
    for i in range(count):
        size = SPECULAR_SIZE >> i
        roughness = i / (count - 1)
        levels.append(specular_level(pano, size, roughness) * specular_gain)
        print(f"  specular level {i}: {size}², roughness {roughness:.3f}")
    common.write_ktx2_cube_rgba16f(common.output("environment", f"{name}_specular.ktx2"), levels)
    diffuse, main = irradiance(pano, DIFFUSE_SIZE)
    common.write_ktx2_cube_rgba16f(common.output("environment", f"{name}_diffuse.ktx2"), [diffuse])
    print(f"  most light comes from {np.round(main, 3).tolist()} (x, y, z; y up)")
    return levels, diffuse


def run():
    build("room", "lythwood_room_2k.hdr")
