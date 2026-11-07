#!/usr/bin/env python3
"""Install out/512/<Tile>.png as assets/textures/materials/<Tile>.png, rewrite those tiles' block_mean rows in block_texture.wesl
from their mean linear colour, and write out/materials_contact.png (each layer 3x3 at 128px)."""
import re

import numpy as np
from PIL import Image

from generate import HERE, OUT, PAINTED, PAINTED_OUT, TILES

DEST = HERE / "../../../assets/textures/materials"
SHADER = HERE / "../../../assets/shaders/block_texture.wesl"
GRASS_TEXTURE_MEAN = 0.5  # crates/ochre-base/src/biome/mod.rs, the greyscale mean the grass tint assumes


def linear(v):
    v = v / 255
    return np.where(v <= 0.04045, v / 12.92, ((v + 0.055) / 1.055) ** 2.4)


DEST.mkdir(exist_ok=True)
means = {}
for name in PAINTED:
    img = Image.open(PAINTED_OUT / f"{name}.png").convert("RGB")
    assert img.size == (512, 512), f"{name} is {img.size}, layers are 512x512"
    means[name] = linear(np.asarray(img, float).reshape(-1, 3)).mean(0)
    if name == "GrassTop":
        assert np.ptp(np.asarray(img, int).reshape(-1, 3), axis=1).max() == 0, "GrassTop must be greyscale, the biome tint multiplies it"
        assert abs(means[name][0] - GRASS_TEXTURE_MEAN) < 0.01, f"GrassTop mean is {means[name][0]:.4f}, the tint assumes {GRASS_TEXTURE_MEAN}"
    img.save(DEST / f"{name}.png")

cell = {t["name"]: i for i, t in enumerate(TILES)}
row = -1


def mean_row(m):
    global row
    row += 1
    name = TILES[row]["name"]
    return "vec3<f32>(%.4f, %.4f, %.4f)" % tuple(means[name]) if name in means else m.group(0)


lines = [re.sub(r"vec3<f32>\([^)]*\)", mean_row, l, count=1) if l.strip().startswith("vec3<f32>(") else l for l in SHADER.read_text().splitlines()]
assert row + 1 == len(TILES), f"block_texture.wesl has {row + 1} vec3 rows, expected one per tile"
text = "\n".join(lines) + "\n"
if text != SHADER.read_text():  # an untouched shader keeps its mtime, so it isn't hot-reloaded for nothing
    SHADER.write_text(text)

sheet = Image.new("RGB", (len(PAINTED) * 384, 384))
for i, name in enumerate(PAINTED):
    layer = Image.open(DEST / f"{name}.png").resize((128, 128), Image.LANCZOS)
    sheet.paste(Image.fromarray(np.tile(np.asarray(layer), (3, 3, 1))), (i * 384, 0))
sheet.save(OUT / "materials_contact.png")
print("installed", ", ".join(PAINTED), "; seam check:", OUT / "materials_contact.png")
