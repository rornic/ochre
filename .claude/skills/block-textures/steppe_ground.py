"""Draws the steppe's ground layer, steppe_top.png: rougher and less leafy than grass_top, grey at its mean with coloured pebbles, under the climate tint."""
import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[3]
MATERIALS = ROOT / "assets/textures/materials"
SEED = 7
# how much of each goes in, as a share of the contrast: dirt's pebbles, fine grit, grass_top's leaves, and slow patches of drier and barer ground
DIRT, GRIT, GRASS, PATCH = 0.6, 0.9, 0.3, 0.7
CONTRAST = 0.085
# linear mean of grass_top, which the tint is built for
LINEAR_MEAN = 0.4981
# how the pebbles' colour differs from the ground's, in linear light, under the steppe's golden tint (0.77, 0.58, 0.14), which zeroes blue: rust-brown stones and a few pale ones
RUST, PALE = np.array([0.72, 0.58, 0.65]), np.array([1.25, 1.3, 1.7])


def lum(path):
    a = np.asarray(Image.open(path).convert("RGB")).astype(np.float64) / 255
    return a @ [0.2126, 0.7152, 0.0722]


def wrap_blur(a, sigma):
    f = np.fft.fftfreq(a.shape[0])
    k = np.exp(-2 * (np.pi * sigma) ** 2 * (f[:, None] ** 2 + f[None, :] ** 2))
    return np.fft.ifft2(np.fft.fft2(a) * k).real


def unit(a):
    return (a - a.mean()) / a.std()


def to_linear(s):
    return np.where(s <= 0.04045, s / 12.92, ((s + 0.055) / 1.055) ** 2.4)


def make():
    rng = np.random.default_rng(SEED)
    dirt = unit(lum(MATERIALS / "dirt.png"))
    grass = unit(lum(MATERIAL_GRASS))
    grit = unit(wrap_blur(rng.standard_normal(dirt.shape), 0.9) + 0.5 * rng.standard_normal(dirt.shape))
    patch = unit(wrap_blur(rng.standard_normal(dirt.shape), 14.0))
    field = unit(DIRT * dirt + GRIT * grit + GRASS * grass + PATCH * patch)
    # shift the brightness until the linear mean is grass's, so the tint reads as bright
    offset = 0.727
    for _ in range(12):
        s = np.clip(offset + CONTRAST * field, 0, 1)
        offset += (LINEAR_MEAN - to_linear(s).mean()) * 0.8
    grey = to_linear(np.clip(offset + CONTRAST * field, 0, 1))
    # dirt's pebbles are its brighter blobs, ringed darker
    relief = unit(lum(MATERIALS / "dirt.png") - wrap_blur(lum(MATERIALS / "dirt.png"), 6.0))
    # each pebble whole, and one colour to a pebble: labelled blobs of the brighter ground, rusty or pale by a coin toss
    from scipy import ndimage
    blobs, count = ndimage.label(relief > 0.8)
    sizes = np.bincount(blobs.ravel(), minlength=count + 1)
    kept = rng.random(count + 1) < 0.6
    pebble = np.isin(blobs, np.nonzero((sizes >= 60) & kept)[0][1:])
    # grown a little, so a pebble's rim is coloured too
    blobs = ndimage.grey_dilation(np.where(pebble, blobs, 0), size=5)
    coin = rng.random(count + 1) < 0.65
    stone = ndimage.gaussian_filter((blobs > 0).astype(float), 1.2, mode="wrap")[..., None]
    rusty = ndimage.gaussian_filter(coin[blobs].astype(float) * (blobs > 0), 1.2, mode="wrap")[..., None] / np.maximum(stone, 1e-6)
    rusty = np.clip(rusty, 0, 1)
    rim = smoothstep(0.6, 1.5, -relief)[..., None]
    factor = 1 + stone * (rusty * (RUST - 1) + (1 - rusty) * (PALE - 1))
    linear = np.clip(grey[..., None] * factor * (1 - 0.18 * rim), 0, 1)
    return (to_srgb(linear) * 255).round().astype(np.uint8), linear.reshape(-1, 3).mean(0)


def smoothstep(a, b, x):
    t = np.clip((x - a) / (b - a), 0, 1)
    return t * t * (3 - 2 * t)


def to_srgb(l):
    return np.where(l <= 0.0031308, l * 12.92, 1.055 * np.power(np.maximum(l, 1e-9), 1 / 2.4) - 0.055)


MATERIAL_GRASS = MATERIALS / "grass_top.png"

if __name__ == "__main__":
    img, mean = make()
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else MATERIALS / "steppe_top.png"
    Image.fromarray(img).save(out)
    print(f"{out} linear mean {mean.round(4)}")
