#!/usr/bin/env python3
"""Install out/512/<Tile>.png as assets/textures/materials/<tile>.png (snake case, `GrassTop` as `grass_top.png`), rewrite those tiles' block_mean rows in block_texture.wesl
from their mean linear colour, and write out/materials_contact.png (each layer 3x3 at 128px)."""
import re

import numpy as np
from PIL import Image, ImageEnhance

from generate import HERE, OUT, PAINTED, PAINTED_OUT, TILES

DEST = HERE / "../../../assets/textures/materials"
SHADER = HERE / "../../../assets/shaders/block_texture.wesl"
GRASS_TEXTURE_MEAN = 0.5  # crates/ochre-base/src/biome/mod.rs, the greyscale mean the grass tint assumes


def file_name(tile):
    """`GrassTop` as `grass_top`, see `MATERIAL_FILES` in crates/ochre-base/src/block.rs."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", tile).lower()


def linear(v):
    v = v / 255
    return np.where(v <= 0.04045, v / 12.92, ((v + 0.055) / 1.055) ** 2.4)


# Layers painted with features too big for what they are: tiled this many times over and brought back to 512, which keeps them
# seamless. Gravel's cobbles were a block across.
SCALE = {"Gravel": 3}
# Layers that install another's painting: shale is the grey cracked flagstone that was the stone layer's, which is a placeholder
# (`BlockType::Rock`) that a real world never shows, see docs/materials.md. Its compacted form follows it.
ALIAS = {"Shale": "Stone"}


def painting(name):
    return Image.open(PAINTED_OUT / f"{ALIAS.get(name, name)}.png")
# Layers whose colour is pulled toward grey by this much of its saturation: gravel's orange cobbles read as bright pink scree.
DESATURATE = {"Gravel": 0.5}


def tile_down(img, n):
    return Image.fromarray(np.tile(np.asarray(img.convert("RGB")), (n, n, 1))).resize((512, 512), Image.LANCZOS)


DEST.mkdir(exist_ok=True)
# a compacted rock is its rock's layer packed darker, as `derive()` in generate.py does the atlas tile
for t in TILES:
    if t["name"] in PAINTED and t.get("derive") == "shade":
        body = painting(t["body"])
        body = np.asarray(tile_down(body, SCALE[t["body"]]) if t["body"] in SCALE else body.convert("RGB"), float)
        Image.fromarray(np.clip((body * t["factor"] * np.array(t["tint"])).round(), 0, 255).astype("uint8")).save(PAINTED_OUT / f"{t['name']}.png")
means = {}
for name in PAINTED:
    img = painting(name).convert("RGB")
    assert img.size == (512, 512), f"{name} is {img.size}, layers are 512x512"
    if name in SCALE:
        img = tile_down(img, SCALE[name])
    if name in DESATURATE:
        img = ImageEnhance.Color(img).enhance(DESATURATE[name])
    means[name] = linear(np.asarray(img, float).reshape(-1, 3)).mean(0)
    if name == "GrassTop":
        assert np.ptp(np.asarray(img, int).reshape(-1, 3), axis=1).max() == 0, "GrassTop must be greyscale, the biome tint multiplies it"
        assert abs(means[name][0] - GRASS_TEXTURE_MEAN) < 0.01, f"GrassTop mean is {means[name][0]:.4f}, the tint assumes {GRASS_TEXTURE_MEAN}"
    img.save(DEST / f"{file_name(name)}.png")

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
    layer = Image.open(DEST / f"{file_name(name)}.png").resize((128, 128), Image.LANCZOS)
    sheet.paste(Image.fromarray(np.tile(np.asarray(layer), (3, 3, 1))), (i * 384, 0))
sheet.save(OUT / "materials_contact.png")
print("installed", ", ".join(PAINTED), "; seam check:", OUT / "materials_contact.png")
