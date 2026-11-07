#!/usr/bin/env python3
"""Paint the 512px material layers with SDXL and a hand-painted seamless texture LoRA, through a running ComfyUI:
candidates to out/sdxl/<Tile>_s<seed>.png and a numbered sheet, then --pick to install one as out/512/<Tile>.png for
materials.py.

    python lora_paint.py Stone --seeds 1 2 3 4        # candidates and out/sdxl/Stone_sheet.png
    python lora_paint.py --pick Stone=3 Dirt=1        # the picked ones to out/512/
"""
import argparse
import json

import numpy as np
from PIL import Image

from generate import API, HERE, OUT, PAINTED, PAINTED_OUT, run

SDXL_OUT = OUT / "sdxl"
TRIGGER = "2d seamless hand-painted texture of "
AVOID = "photo, realistic, 3d render, perspective, border, frame, text, watermark, vignette, depth of field, blurry, noise, grain"
PROMPTS = {
    "Stone": "grey stone cobbles, flat painted planes with lighter painted edges and dark gaps, top-down",
    "Dirt": "brown dirt ground with a few small stones, soft painted patches of lighter and darker earth, top-down",
    "GrassTop": "lush green grass, painted tufts of light and dark green, top-down",
    "Sand": "pale desert sand, soft painted dune ripples, top-down",
    "Snow": "white snow, soft painted drifts with pale blue shadows, top-down",
    "Limestone": "pale cream limestone, flat painted planes with soft cracks and small pits, top-down",
    "Clay": "ochre clay ground, smooth painted patches of dry cracked earth, top-down",
    "Basalt": "dark grey basalt rock, flat painted planes with angular fractures, top-down",
    "Marble": "white marble, soft painted grey veins swirling across smooth stone, top-down",
    "Granite": "pinkish grey granite rock, painted speckles and a few cracks, top-down",
    "Shale": "dark grey shale rock, painted thin flaky layers and cracks, top-down",
    "SandstoneTop": "tan sandstone, painted soft strata and small pits, top-down",
    "Gravel": "loose gravel of mixed grey, tan and brown pebbles, painted rounded stones, top-down",
}


def build(name, seed, steps, cfg, strength, prompt=None, avoid=None):
    wf = json.loads((HERE / "block_texture_sdxl.json").read_text())
    wf["2"]["inputs"]["strength_model"] = wf["2"]["inputs"]["strength_clip"] = strength
    wf["4"]["inputs"]["text"] = TRIGGER + (prompt or PROMPTS[name])
    wf["5"]["inputs"]["text"] = AVOID + (", " + avoid if avoid else "")
    wf["7"]["inputs"].update(seed=seed, steps=steps, cfg=cfg)
    return wf


# Mean linear colour each layer is moved to when picked: a painted render is darker than the world wants, and the
# `block_mean` row (see `materials.py`) is worked out from what is installed. GrassTop is set by its tint instead.
TARGET_MEAN = {
    "Stone": 0.16, "Dirt": 0.14, "Sand": 0.45, "Snow": 0.72,
    "Limestone": 0.47, "Clay": 0.23, "Basalt": 0.06, "Marble": 0.71, "Granite": 0.34, "Shale": 0.10, "SandstoneTop": 0.48, "Gravel": 0.24,
}  # the atlas tiles' mean linear luminance, so the world keeps its tones

CONTRAST = {"Sand": 0.5}  # painted dunes read too strongly at full contrast
# how far to level the detail across the layer, 0 none to 1 fully: one big crack or blob reads as an obvious repeat in the world
EQUALISE = {"Stone": 0.5, "Limestone": 0.8, "Clay": 0.8, "Basalt": 0.5, "Marble": 0.5, "Granite": 0.8, "Shale": 0.5, "SandstoneTop": 0.8}


def linear(v):
    return np.where(v <= 0.04045, v / 12.92, ((v + 0.055) / 1.055) ** 2.4)


def srgb(v):
    return np.where(v <= 0.0031308, v * 12.92, 1.055 * np.power(np.maximum(v, 1e-9), 1 / 2.4) - 0.055)


def wrap_blur(a, k):
    """A k-pixel box blur centred on each pixel, wrapping at the edges."""
    kernel = np.zeros(a.shape)
    kernel[:k, :k] = 1 / (k * k)
    blurred = np.fft.irfft2(np.fft.rfft2(a) * np.fft.rfft2(kernel), s=a.shape)
    return np.roll(blurred, (-(k // 2), -(k // 2)), (0, 1))


def equalised(img, amount, k=96):
    """Divide the luminance detail by its local strength to the power `amount`, so busy patches calm down and quiet ones don't."""
    a = np.asarray(img, np.float32) / 255
    luma = a.mean(-1)
    mid = wrap_blur(luma, k)
    spread = np.sqrt(np.maximum(wrap_blur((luma - mid) ** 2, k), 1e-6))
    gain = (spread.mean() / spread) ** amount
    return Image.fromarray((np.clip(a + ((luma - mid) * (gain - 1))[..., None], 0, 1) * 255 + 0.5).astype(np.uint8))


def exposed(img, target, contrast=1.0):
    """Scale the picture's linear light so its mean is `target`, keeping the painting's hue, and its contrast times `contrast`."""
    a = linear(np.asarray(img, np.float32) / 255)
    a = a * (target / a.mean())
    mean = a.mean((0, 1))
    a = np.clip(mean + (a - mean) * contrast, 0, 1)
    return Image.fromarray((srgb(a) * 255 + 0.5).astype(np.uint8))


def grass_normalised(img):
    """Shift the greyscale so its mean linear colour is 0.5, which the biome tint assumes, see `materials.py`."""
    lo, hi = -0.6, 0.6
    for _ in range(40):
        mid = (lo + hi) / 2
        if linear(np.clip(img + mid, 0, 1)).mean() > 0.5:
            hi = mid
        else:
            lo = mid
    return np.clip(img + (lo + hi) / 2, 0, 1)


def greyscale_for_tint(img):
    """GrassTop is tinted by the biome, which wants a greyscale layer."""
    a = np.asarray(img.convert("L"), np.float32)[..., None] / 255
    return Image.fromarray((np.repeat(grass_normalised(a), 3, -1) * 255 + 0.5).astype(np.uint8))


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("tiles", nargs="*", default=PAINTED)
    ap.add_argument("--seeds", type=int, nargs="+", default=[1])
    ap.add_argument("--steps", type=int, default=28)
    ap.add_argument("--cfg", type=float, default=6.5)
    ap.add_argument("--strength", type=float, default=1.0, help="the LoRA's weight")
    ap.add_argument("--pick", nargs="+", metavar="TILE=SEED", help="or TILE=TAGSEED for a run made with --tag")
    ap.add_argument("--prompt", help="what to paint instead of the tile's own prompt, after the trigger words")
    ap.add_argument("--avoid", help="more to keep out of the picture")
    ap.add_argument("--tag", default="", help="names the files of a run with its own --prompt, <Tile>_<tag><seed>")
    args = ap.parse_args()
    SDXL_OUT.mkdir(parents=True, exist_ok=True)
    if args.pick:
        PAINTED_OUT.mkdir(parents=True, exist_ok=True)
        for pick in args.pick:
            name, seed = pick.split("=")
            stem = next(p for p in (f"{name}_s{seed}", f"{name}_{seed}") if (SDXL_OUT / f"{p}.png").exists())
            img = Image.open(SDXL_OUT / f"{stem}.png").convert("RGB")
            if name == "GrassTop":
                img = greyscale_for_tint(img)
            else:
                if name in EQUALISE:
                    img = equalised(img, EQUALISE[name])
                img = exposed(img, TARGET_MEAN[name], CONTRAST.get(name, 1.0))
            img.save(PAINTED_OUT / f"{name}.png")
            print("picked", name, seed)
    else:
        for name in args.tiles:
            tiles = []
            for seed in args.seeds:
                import io

                raw = Image.open(io.BytesIO(run(build(name, seed, args.steps, args.cfg, args.strength, args.prompt, args.avoid)))).convert("RGB")
                small = raw.resize((512, 512), Image.LANCZOS)
                small.save(SDXL_OUT / (f"{name}_{args.tag}{seed}.png" if args.tag else f"{name}_s{seed}.png"))
                tiles.append(small)
                print(name, "seed", seed, flush=True)
            sheet = Image.new("RGB", (384 * len(tiles), 384))
            for i, t in enumerate(tiles):
                sheet.paste(t.resize((384, 384), Image.LANCZOS), (384 * i, 0))
            sheet.save(SDXL_OUT / (f"{name}_{args.tag}_sheet.png" if args.tag else f"{name}_sheet.png"))
