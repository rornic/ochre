#!/usr/bin/env python3
"""Leaves, ground flora and shrubs as 512px RGBA painted layers: assets/textures/materials/<name>.png, see `Tile::foliage_layer`.

    python lora_paint.py Leaves --tag l --seeds 1 2 3 4 --prompt "..."   # SDXL candidates, as for any painted tile
    python foliage.py --leaves 4            # holes cut from the dark gaps of out/sdxl/Leaves_l4.png, greyscale for the tint
    python foliage.py                       # draws the flora over that paint, installs every layer, rewrites their block_mean rows

Leaves tile every block, so the layer is seamless and its holes are the painted gaps between leaf clumps. Plants are one
sprite across their quad: SD can't draw a lone plant on transparency, so their silhouettes are drawn as tapered brush strokes at
2x and filled with the painting's tone, which keeps the painted look and gives clean anti-aliased edges. Greyscale ones (grass,
fern, leaves) are tinted by the climate, so are levelled to the mean the tint assumes.
"""
import argparse
import re

import numpy as np
from PIL import Image, ImageDraw, ImageFilter
from scipy import ndimage

from generate import HERE, OUT, TILES
from lora_paint import SDXL_OUT, linear, srgb, wrap_blur

DEST = HERE / "../../../assets/textures/materials"
SHADER = HERE / "../../../assets/shaders/block_texture.wesl"
FOLIAGE_OUT = OUT / "foliage"
SIZE, SS = 512, 2
N = SIZE * SS
LEAF_MEAN = 0.25  # LEAF_TEXTURE_MEAN in crates/ochre-base/src/biome/mod.rs, linear
GRASS_MEAN = 0.5  # GRASS_TEXTURE_MEAN
FERN_MEAN = 0.3  # a darker fern, as the atlas tile was
SHRUB_MEAN = 0.4  # shrubs are a little darker than grass
HOLES = 0.3  # share of a leaf layer that is gap


def smooth(x, lo, hi):
    t = np.clip((x - lo) / (hi - lo), 0, 1)
    return t * t * (3 - 2 * t)


def level(a, mask, target):
    """Scale a greyscale layer's linear light so its opaque texels average `target`."""
    lin = linear(a / 255)
    lin = lin * (target / lin[mask].mean())
    return np.clip(srgb(np.clip(lin, 0, 1)) * 255, 0, 255)


def fill_clear(rgb, alpha):
    """Clear texels take the colour of the opaque ones round them, so bilinear lookups and mips don't bleed a dark edge in."""
    weight = wrap_blur(alpha, 25) + 1e-4
    for c in range(3):
        near = wrap_blur(rgb[..., c] * alpha, 25) / weight
        rgb[..., c] = np.where(alpha > 0.999, rgb[..., c], np.where(alpha > 0.0, rgb[..., c], near))
    return rgb


def bleed(rgb, alpha):
    """Every clear texel takes the colour of the nearest drawn one, however far, so that no mip or bilinear lookup near an edge finds black.
    `fill_clear` reaches only so far, and a big sprite with a lot of empty canvas has texels past that, which a blurred alpha cut draws at a distance."""
    solid = alpha > 0.02
    nearest = ndimage.distance_transform_edt(~solid, return_distances=False, return_indices=True)
    return np.where(solid[..., None], rgb, rgb[nearest[0], nearest[1]])


def leaves(src):
    """Holes where the painting is darkest, once its veins and edges are blurred away, so they are leaf-shaped gaps."""
    img = np.asarray(Image.open(src).convert("RGB"), np.float32)
    luma = img.mean(-1) / 255
    gap = wrap_blur(luma, 11)
    cut = np.quantile(gap, HOLES)
    alpha = smooth(gap, cut - 0.012, cut + 0.012)
    paint = luma * 255
    mid = wrap_blur(paint, 64)
    grey = mid + (paint - mid) * 0.8  # a little less contrast, so the sun and the painterly lighting do the shading
    opaque = alpha > 0.5
    grey = level(grey + 0 * alpha, opaque, LEAF_MEAN)
    rgb = fill_clear(np.repeat(grey[..., None], 3, -1), alpha)
    return np.dstack([rgb, alpha * 255]).round().clip(0, 255).astype(np.uint8)


class Canvas:
    """Strokes drawn at 2x, as colour and coverage, then brought down to 512 premultiplied."""

    def __init__(self):
        self.rgb = Image.new("RGB", (N, N), (0, 0, 0))
        self.mask = Image.new("L", (N, N), 0)
        self.rgbd, self.maskd = ImageDraw.Draw(self.rgb), ImageDraw.Draw(self.mask)

    def poly(self, pts, colour):
        # kept off the top and sides, which a clamped sprite lookup must find clear
        pts = [(min(max(x, 10), SIZE - 11) * SS, max(y, 10) * SS) for x, y in pts]
        self.rgbd.polygon(pts, fill=colour)
        self.maskd.polygon(pts, fill=255)

    def ellipse(self, cx, cy, rx, ry, colour, angle=0.0, steps=28):
        t = np.linspace(0, 2 * np.pi, steps, endpoint=False)
        ca, sa = np.cos(angle), np.sin(angle)
        self.poly([(cx + rx * np.cos(a) * ca - ry * np.sin(a) * sa, cy + rx * np.cos(a) * sa + ry * np.sin(a) * ca) for a in t], colour)

    def finish(self, paint, softness=1.4):
        """RGBA uint8: coverage softened a touch like a brush's edge, colour modulated by `paint`, a 512px painting's luminance."""
        mask = self.mask.filter(ImageFilter.GaussianBlur(softness * SS)).resize((SIZE, SIZE), Image.LANCZOS)
        alpha = np.asarray(mask, np.float32) / 255
        rgb = np.asarray(self.rgb, np.float32)
        pre = Image.fromarray(np.clip(rgb * (np.asarray(self.mask, np.float32)[..., None] / 255), 0, 255).astype(np.uint8))
        pre = np.asarray(pre.filter(ImageFilter.GaussianBlur(softness * SS)).resize((SIZE, SIZE), Image.LANCZOS), np.float32)
        out = pre / np.maximum(alpha[..., None], 1e-3)
        out = out * (0.78 + 0.44 * paint[..., None])  # brush-stroke variation, +-22%
        out = bleed(np.clip(out, 0, 255), alpha)
        return out, alpha


def curve(x0, y0, x1, y1, bend, t):
    """A quadratic bezier from (x0, y0) to (x1, y1), its middle pushed sideways by `bend`."""
    cx, cy = (x0 + x1) / 2 + bend, (y0 + y1) / 2
    return ((1 - t) ** 2 * x0 + 2 * (1 - t) * t * cx + t * t * x1, (1 - t) ** 2 * y0 + 2 * (1 - t) * t * cy + t * t * y1)


def stroke(cv, x0, y0, x1, y1, bend, w0, w1, tone0, tone1, rgb=None, steps=14):
    """A tapered brush stroke, root to tip, `tone` greys or `rgb` (root, tip) colours."""
    pts = [curve(x0, y0, x1, y1, bend, i / steps) for i in range(steps + 1)]
    for i in range(steps):
        t = (i + 0.5) / steps
        (ax, ay), (bx, by) = pts[i], pts[i + 1]
        d = np.array([bx - ax, by - ay])
        d = d / (np.linalg.norm(d) + 1e-6)
        n = np.array([-d[1], d[0]])
        wa, wb = w0 + (w1 - w0) * i / steps, w0 + (w1 - w0) * (i + 1) / steps
        if rgb is None:
            v = tone0 + (tone1 - tone0) * t
            colour = (int(v), int(v), int(v))
        else:
            colour = tuple(int(rgb[0][k] + (rgb[1][k] - rgb[0][k]) * t) for k in range(3))
        cv.poly([(ax + n[0] * wa, ay + n[1] * wa), (bx + n[0] * wb, by + n[1] * wb), (bx - n[0] * wb, by - n[1] * wb), (ax - n[0] * wa, ay - n[1] * wa)], colour)


def tall_grass(rng, cv):
    for _ in range(13):
        x = rng.uniform(50, 462)
        h = rng.uniform(300, 500)
        lean = rng.uniform(-90, 90) + (x - 256) * 0.35
        tone = rng.uniform(-18, 18)
        stroke(cv, x, 511, x + lean, 511 - h, lean * 0.35, rng.uniform(13, 21), 1.2, 110 + tone, 225 + tone)


def short_grass(rng, cv):
    for _ in range(17):
        x = rng.uniform(30, 482)
        h = rng.uniform(110, 250)
        lean = rng.uniform(-50, 50) + (x - 256) * 0.2
        tone = rng.uniform(-20, 20)
        stroke(cv, x, 511, x + lean, 511 - h, lean * 0.3, rng.uniform(12, 19), 1.2, 125 + tone, 220 + tone)


def seed_grass(rng, cv):
    short_grass(rng, cv)
    for x in rng.choice(np.arange(80, 440, 30), size=4, replace=False):
        h = rng.uniform(340, 480)
        lean = rng.uniform(-60, 60)
        stroke(cv, x, 511, x + lean, 511 - h, lean * 0.3, 7, 3, 120, 190)
        tx, ty = curve(x, 511, x + lean, 511 - h, lean * 0.3, 1.0)
        for k in range(5):  # the head: pale seeds climbing the stalk's tip
            cv.ellipse(tx + (k % 2 - 0.5) * 9, ty + 22 + k * 20, 10, 17, (238, 238, 238), angle=0.25 * (k % 2 - 0.5))


def fern(rng, cv):
    for lean in (-260, -95, 70, 255):
        top_x = 256 + lean * 0.9
        top_y = rng.uniform(70, 150) + abs(lean) * 0.25
        bend = np.sign(lean) * rng.uniform(40, 80)
        stroke(cv, 256 + np.sign(lean) * 12, 511, top_x, top_y, bend, 7, 2, 105, 150)
        for i in range(2, 16):
            t = i / 16
            x, y = curve(256 + np.sign(lean) * 12, 511, top_x, top_y, bend, t)
            nx, ny = curve(256 + np.sign(lean) * 12, 511, top_x, top_y, bend, t + 0.04)
            d = np.array([nx - x, ny - y])
            d = d / (np.linalg.norm(d) + 1e-6)
            n = np.array([-d[1], d[0]])
            length = 95 * (1 - t) ** 0.8 + 18
            for side in (-1, 1):
                tip = (x + (n[0] * side * 0.95 + d[0] * 0.55) * length, y + (n[1] * side * 0.95 + d[1] * 0.55) * length)
                stroke(cv, x, y, tip[0], tip[1], 0, 11 * (1 - t * 0.5), 1.5, 120 + 60 * t, 160 + 60 * t, steps=6)


STEM, LEAFC = (72, 122, 44), (92, 146, 52)


def flower_stem(cv, x, top, rng):
    stroke(cv, x, 511, x + rng.uniform(-30, 30), top, rng.uniform(-30, 30), 9, 5, 0, 0, rgb=((60, 104, 38), (86, 136, 50)))
    side = rng.choice((-1, 1))
    ly = 511 - (511 - top) * 0.35
    stroke(cv, x, ly, x + side * 110, ly - 70, side * 20, 18, 2, 0, 0, rgb=(LEAFC, (110, 168, 62)), steps=8)


def poppy(rng, cv):
    for x, top in ((170, 120), (350, 250)):
        flower_stem(cv, x, top + 20, rng)
        for k in range(5):  # four or five loose petals round a dark heart
            a = k * 2 * np.pi / 5 + 0.3
            cv.ellipse(x + 40 * np.cos(a), top + 40 * np.sin(a), 46, 34, (214, 40, 32) if k % 2 else (186, 28, 26), angle=a)
        cv.ellipse(x, top, 22, 22, (46, 22, 20))
        cv.ellipse(x, top, 9, 9, (24, 14, 14))


def dandelion(rng, cv):
    for x in np.linspace(70, 440, 11):  # a toothed rosette
        h = rng.uniform(60, 130) * (1.4 - abs(x - 256) / 400)
        stroke(cv, x, 511, x + rng.uniform(-40, 40), 511 - h, rng.uniform(-15, 15), 26, 2, 0, 0, rgb=((70, 118, 42), (112, 170, 60)), steps=8)
    for x, top in ((190, 150), (340, 270)):
        flower_stem(cv, x, top + 20, rng)
        for k in range(18):  # a ring of thin petals round a gold heart
            a = k * 2 * np.pi / 18
            cv.ellipse(x + 38 * np.cos(a), top + 38 * np.sin(a), 30, 8, (246, 206, 52), angle=a, steps=14)
        cv.ellipse(x, top, 26, 26, (232, 176, 24))


def leaf(cv, x, y, length, width, angle, tone, rgb=None):
    """A rounded leaf from its base at (x, y) out along `angle`, a grey of `tone` or `rgb` scaled by it."""
    colour = (int(tone),) * 3 if rgb is None else tuple(int(min(c * tone / 150, 255)) for c in rgb)
    cv.ellipse(x + np.cos(angle) * length / 2, y + np.sin(angle) * length / 2, length / 2, width / 2, colour, angle=angle, steps=22)


def shrub(rng, cv):
    """A dome of leaves: fanned out from the middle, dark deep in and low down, light at the tips and top."""
    cx, cy, rx, ry = 256, 325, 200, 188
    for _ in range(3):  # woody stems under the leaves
        x = cx + rng.uniform(-50, 50)
        stroke(cv, x, 511, x + rng.uniform(-25, 25), 430, rng.uniform(-12, 12), 14, 8, 50, 76)
    cv.ellipse(cx, cy + 30, rx * 0.7, ry * 0.75, (44, 44, 44))  # the shade deep inside, so the middle isn't hollow
    spots = []
    while len(spots) < 300:
        x, y = rng.uniform(-1, 1), rng.uniform(-1, 1)
        if x * x + y * y <= 1 and y < 0.82:
            spots.append((x, y))
    # back to front: low and inner first, so the light outer leaves lie over the dark inner ones
    spots.sort(key=lambda p: -(p[1] * 0.7 - (p[0] ** 2 + p[1] ** 2) * 0.5))
    for x, y in spots:
        out = np.arctan2(y * 0.9 - 0.3, x)  # outward and up from the dome's heart
        edge = min(np.hypot(x, y), 1.0)
        tone = 78 + 86 * (0.5 - y * 0.5) + 44 * edge + rng.uniform(-16, 16)
        leaf(cv, cx + x * rx, cy + y * ry, rng.uniform(42, 68), rng.uniform(15, 24), out + rng.uniform(-0.7, 0.7), tone)


HEATH_LEAF, HEATH_STEM, HEATH_FLOWER = (112, 92, 138), (78, 58, 74), (192, 142, 218)  # dusty violet, as a Skyrim tundra's scrub


def heath(rng, cv):
    """Low purple scrub: wiry stems arching out from the root, small violet leaves along them and lilac flowers at the tips.
    It keeps its own colour, not the climate's grass tint."""
    for k in range(14):
        a = (k / 13 - 0.5) * 2.6 + rng.uniform(-0.08, 0.08)  # fanned evenly either side of upright
        side = 1 if a >= 0 else -1
        length = rng.uniform(190, 270)
        x0 = 256 + rng.uniform(-16, 16)
        x1, y1 = 256 + np.sin(a) * length * 0.95, 511 - np.cos(a) * length * 0.7
        bend = side * rng.uniform(-20, 45)
        stroke(cv, x0, 511, x1, y1, bend, 6, 2.4, 0, 0, rgb=(HEATH_STEM, tuple(int(c * 1.3) for c in HEATH_STEM)))
        heading = np.arctan2(y1 - 511, x1 - x0)
        for i in range(4, 22):
            t = i / 21
            x, y = curve(x0, 511, x1, y1, bend, t)
            for j in (-1, 1):
                leaf(cv, x, y, rng.uniform(22, 36) * (1.15 - t * 0.45), rng.uniform(8, 12), heading + j * rng.uniform(0.6, 1.1), 100 + 70 * t + rng.uniform(-14, 14), HEATH_LEAF)
        for _ in range(5):  # clustered flowers at the tip
            cv.ellipse(x1 + rng.uniform(-26, 26), y1 + rng.uniform(-18, 14), rng.uniform(8, 13), rng.uniform(8, 13), tuple(int(c * rng.uniform(0.85, 1.1)) for c in HEATH_FLOWER))


DRAW = {"TallGrass": tall_grass, "Fern": fern, "Poppy": poppy, "Dandelion": dandelion, "ShortGrass": short_grass, "SeedGrass": seed_grass, "Shrub": shrub, "Heath": heath}
GREY = {"TallGrass": GRASS_MEAN, "Fern": FERN_MEAN, "ShortGrass": GRASS_MEAN, "SeedGrass": GRASS_MEAN, "Shrub": SHRUB_MEAN}


def file_name(tile):
    return re.sub(r"(?<!^)(?=[A-Z])", "_", tile).lower()


def paint_noise(src):
    """A painting's luminance, 0..1 around 0.5, tiled to the canvas, as the brush-stroke variation."""
    luma = np.asarray(Image.open(src).convert("L"), np.float32) / 255
    mid = wrap_blur(luma, 48)
    return np.clip(0.5 + (luma - mid) * 2.0, 0, 1)


def flora(name, seed, paint):
    cv = Canvas()
    DRAW[name](np.random.default_rng(seed), cv)
    rgb, alpha = cv.finish(paint)
    if name in GREY:
        grey = rgb.mean(-1)
        grey = level(grey, alpha > 0.5, GREY[name])
        rgb = bleed(np.repeat(grey[..., None], 3, -1), alpha)
    return np.dstack([rgb, alpha * 255]).round().clip(0, 255).astype(np.uint8)


def opaque_mean(a):
    drawn = a[..., 3] >= 128
    return linear(a[..., :3][drawn].astype(np.float32) / 255).mean(0)


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--leaves", type=int, help="the Leaves_l<seed> candidate to cut holes in")
    ap.add_argument("--seed", type=int, default=3, help="the flora's brush seed")
    ap.add_argument("--paint", default="Leaves_l1", help="a painting in out/sdxl whose tone varies the strokes")
    args = ap.parse_args()
    FOLIAGE_OUT.mkdir(parents=True, exist_ok=True)
    if args.leaves:
        Image.fromarray(leaves(SDXL_OUT / f"Leaves_l{args.leaves}.png"), "RGBA").save(FOLIAGE_OUT / "Leaves.png")
    paint = paint_noise(SDXL_OUT / f"{args.paint}.png")
    for i, name in enumerate(DRAW):
        Image.fromarray(flora(name, args.seed + i, paint), "RGBA").save(FOLIAGE_OUT / f"{name}.png")
    DEST.mkdir(exist_ok=True)
    means = {}
    for name in ["Leaves", *DRAW]:
        a = np.asarray(Image.open(FOLIAGE_OUT / f"{name}.png").convert("RGBA"))
        assert a.shape == (SIZE, SIZE, 4), f"{name} is {a.shape}, layers are 512x512"
        means[name] = opaque_mean(a)
        Image.fromarray(a, "RGBA").save(DEST / f"{file_name(name)}.png")
    row = -1

    def mean_row(m):
        global row
        row += 1
        name = TILES[row]["name"]
        return "vec3<f32>(%.4f, %.4f, %.4f)" % tuple(means[name]) if name in means else m.group(0)

    lines = [re.sub(r"vec3<f32>\([^)]*\)", mean_row, l, count=1) if l.strip().startswith("vec3<f32>(") else l for l in SHADER.read_text().splitlines()]
    assert row + 1 == len(TILES), f"block_texture.wesl has {row + 1} vec3 rows, expected one per tile"
    text = "\n".join(lines) + "\n"
    if text != SHADER.read_text():
        SHADER.write_text(text)
    # a contact sheet: each layer over mid green and sky, leaves 2x2 to show the repeat
    sheet = Image.new("RGB", (len(means) * 520, 1040), (120, 160, 200))
    for i, name in enumerate(means):
        a = Image.open(DEST / f"{file_name(name)}.png").convert("RGBA")
        for j, back in enumerate(((70, 110, 60), (150, 190, 230))):
            tile = Image.new("RGB", (512, 512), back)
            if name in GREY or name == "Leaves":  # tinted to a leafy green the way the climate would
                g = np.asarray(a, np.float32)
                g[..., :3] *= np.array([0.32, 0.75, 0.22]) / 0.35
                tile.paste(Image.fromarray(np.clip(g, 0, 255).astype(np.uint8), "RGBA"), (0, 0), Image.fromarray(np.clip(g, 0, 255).astype(np.uint8), "RGBA"))
            else:
                tile.paste(a, (0, 0), a)
            sheet.paste(tile, (i * 520, j * 520))
    sheet.save(OUT / "foliage_contact.png")
    print("installed", ", ".join(means), "; look at", OUT / "foliage_contact.png")
