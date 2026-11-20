"""What every step of the asset pipeline shares: where things are, reading and writing images, and
writing KTX2 textures. Runs in Blender's Python, which brings numpy, OpenImageIO and zstandard."""

import hashlib
import os
import struct
import tomllib
import urllib.request

import numpy as np
import OpenImageIO as oiio
import zstandard

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))
SOURCES_LIST = os.path.join(REPO, "assets-src", "sources.toml")
CACHE = os.path.join(REPO, "target", "asset-sources")
ASSETS = os.path.join(REPO, "assets")
PREVIEWS = os.path.join(REPO, "target", "asset-previews")


def sources():
    with open(SOURCES_LIST, "rb") as f:
        return tomllib.load(f)["source"]


def fetch():
    """Downloads each source not already in the cache, and checks every one against its hash."""
    os.makedirs(CACHE, exist_ok=True)
    for s in sources():
        path = os.path.join(CACHE, s["file"])
        if not os.path.exists(path) or sha256(path) != s["sha256"]:
            print(f"fetching {s['file']}")
            request = urllib.request.Request(s["url"], headers={"User-Agent": "asset-pipeline"})
            with urllib.request.urlopen(request) as response, open(path + ".part", "wb") as out:
                out.write(response.read())
            os.replace(path + ".part", path)
        got = sha256(path)
        if got != s["sha256"]:
            raise SystemExit(f"{s['file']}: SHA-256 is {got}, not {s['sha256']} as listed")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def source(name):
    return os.path.join(CACHE, name)


def output(*parts):
    path = os.path.join(ASSETS, *parts)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    return path


def read_image(path):
    """An image as float32, (height, width, channels), with values as stored (no colour change)."""
    buf = oiio.ImageBuf(path)
    if buf.has_error:
        raise SystemExit(f"{path}: {buf.geterror()}")
    return buf.get_pixels(oiio.FLOAT)


def srgb_to_linear(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def linear_to_srgb(c):
    c = np.clip(c, 0.0, 1.0)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def resize(pixels, size):
    """Resizes to `size` × `size` with OpenImageIO's default filter (a good one for shrinking)."""
    h, w, c = pixels.shape
    if (w, h) == (size, size):
        return pixels
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, c, oiio.FLOAT))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, c), np.ascontiguousarray(pixels, np.float32))
    out = oiio.ImageBufAlgo.resize(buf, roi=oiio.ROI(0, size, 0, size, 0, 1, 0, c))
    return out.get_pixels(oiio.FLOAT)


def write_jpeg(path, pixels, quality):
    """Writes values in 0..1 (already in the file's encoding: sRGB for colour) as a baseline JPEG
    with no metadata, so the same input always gives the same bytes."""
    h, w, c = pixels.shape
    data = np.clip(np.round(pixels * 255.0), 0, 255).astype(np.uint8)
    spec = oiio.ImageSpec(w, h, c, oiio.UINT8)
    spec.attribute("Compression", f"jpeg:{quality}")
    buf = oiio.ImageBuf(spec)
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, c), np.ascontiguousarray(data))
    if not buf.write(path):
        raise SystemExit(f"{path}: {buf.geterror()}")
    print(f"wrote {os.path.relpath(path, REPO)} ({os.path.getsize(path) // 1024} KiB)")


def write_png(path, pixels):
    """Writes values in 0..1 (already in the file's encoding) as an 8-bit PNG, RGB or RGBA."""
    h, w, c = pixels.shape
    data = np.clip(np.round(pixels * 255.0), 0, 255).astype(np.uint8)
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, c, oiio.UINT8))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, c), np.ascontiguousarray(data))
    if not buf.write(path):
        raise SystemExit(f"{path}: {buf.geterror()}")
    print(f"wrote {os.path.relpath(path, REPO)} ({os.path.getsize(path) // 1024} KiB)")


# KTX2, as Bevy reads it: the format from the header, each mip level's data (every face of it, in
# order +X, -X, +Y, -Y, +Z, -Z) compressed with Zstandard, and a basic data format descriptor.
VK_R16G16B16A16_SFLOAT = 97
KTX2_IDENTIFIER = bytes([0xAB, 0x4B, 0x54, 0x58, 0x20, 0x32, 0x30, 0xBB, 0x0D, 0x0A, 0x1A, 0x0A])
SUPERCOMPRESSION_ZSTD = 2


def write_ktx2_cube_rgba16f(path, levels):
    """Writes a cubemap. `levels`: largest first, each a float array (6, size, size, 3 or 4)."""
    size = levels[0].shape[1]
    datas = []
    for level in levels:
        faces = level.astype(np.float32)
        if faces.shape[-1] == 3:
            faces = np.concatenate([faces, np.ones(faces.shape[:-1] + (1,), np.float32)], -1)
        datas.append(np.ascontiguousarray(faces.astype(np.float16)).tobytes())
    compressed = [zstandard.ZstdCompressor(level=19).compress(d) for d in datas]

    # The data format descriptor: four 16-bit signed floats, R G B A, linear.
    samples = b""
    for i, channel in enumerate((0, 1, 2, 15)):
        channel_type = channel | 0x80 | 0x40  # FLOAT | SIGNED
        samples += struct.pack("<I", (i * 16) | (15 << 16) | (channel_type << 24))
        samples += struct.pack("<I", 0)  # sample position
        samples += struct.pack("<II", 0xBF800000, 0x3F800000)  # -1.0, 1.0
    block_size = 24 + len(samples)
    block = struct.pack("<IHH", 0, 2, block_size)
    block += bytes([1, 1, 1, 0])  # RGBSDA, BT.709 primaries, linear, straight alpha
    block += bytes(4)  # texel block dimensions: 1×1×1×1
    block += bytes(8)  # bytes per plane: unsized, since the levels are supercompressed
    block += samples
    dfd = struct.pack("<I", 4 + len(block)) + block

    header_size = 12 + 9 * 4 + 4 * 4 + 2 * 8
    index_size = 24 * len(levels)
    dfd_offset = header_size + index_size
    data_offset = dfd_offset + len(dfd)

    # Level data is stored smallest first; the index lists levels largest first.
    offsets = [0] * len(levels)
    at = data_offset
    for i in reversed(range(len(levels))):
        offsets[i] = at
        at += len(compressed[i])

    out = KTX2_IDENTIFIER
    out += struct.pack(
        "<9I", VK_R16G16B16A16_SFLOAT, 2, size, size, 0, 0, 6, len(levels), SUPERCOMPRESSION_ZSTD
    )
    out += struct.pack("<IIIIQQ", dfd_offset, len(dfd), 0, 0, 0, 0)
    for i in range(len(levels)):
        out += struct.pack("<QQQ", offsets[i], len(compressed[i]), len(datas[i]))
    out += dfd
    for i in reversed(range(len(levels))):
        out += compressed[i]
    with open(path, "wb") as f:
        f.write(out)
    print(f"wrote {os.path.relpath(path, REPO)} ({len(out) // 1024} KiB)")
