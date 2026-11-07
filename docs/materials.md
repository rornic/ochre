# Painted materials

Terrain faces whose tile has a painted material draw it from a 512px layer sampled by world position, so one texture covers many blocks and ignores block edges. Everything else (the other tiles, leaves, flora, water) draws from the 16px atlas as before. Design: `docs/designs/painted-world-space-materials.md`.

## What is painted

`Tile::material_layer` (`ochre-base/src/block.rs`) maps a tile to a layer, and `MATERIAL_FILES` lists the layers in order, each `assets/textures/materials/<name>.png`, 512x512 opaque sRGB. Painted so far: Stone, Dirt, GrassTop, Sand, Snow, Limestone, Clay, Basalt, Marble, Granite, Shale, SandstoneTop and Gravel. Grass sides draw the GrassTop layer, as the top does, tinted the same, and sandstone sides draw SandstoneTop. The compacted rocks stay in the atlas. `MATERIAL_LAYER_TABLE` in `block_texture.wesl` is the shader's copy by atlas cell, and a test in `material.rs` checks it against `material_layer`.

GrassTop is greyscale at the mean `GRASS_TEXTURE_MEAN` assumes, because the biome tint multiplies it. `block_mean` rows for painted tiles come from their layers (`materials.py`), and a test checks every row against its texture.

## How it draws

- The vertex shader (`world.wesl`) picks the plane from the normal (top faces use world xz, sides xz or zy with up as the texture's up) and writes block-unit coordinates into `uv`. They are the local position plus the chunk origin reduced modulo the period, which is exact in f32 because the period is a power of two. Atlas faces keep tile counts in `uv`. Coarse LOD cells and tree boxes (`uv_b.y > 0.5`) draw their mean.
- The fragment shader takes one array sample per painted fragment, or the layer's mean once a block is under a pixel (`MEAN_BLOCK_RATE`). Slow world-space noise (`drift`) shifts brightness and warmth to hide the repeat, with no extra sample.
- The mesher and vertex attributes are unchanged, so `bench` vertex counts do not move.

## Settings

`[renderer]` in `assets/settings.toml`: `material_period` (blocks before the texture repeats, a power of two up to 64, default 8) and `material_anisotropy` (1 to 16, default 8). Both need a restart. The sandbox reads `OCHRE_RESOLUTION=WxH` and `OCHRE_NOVSYNC=1` to read frame times at a fixed size.

## The fps gate

The change passes if the median frame time at spawn stays at 16.7 ms or lower (60fps) at 1920x1080 on the RTX 2060. Run `OCHRE_SEED=1692045686 OCHRE_RESOLUTION=1920x1080 OCHRE_NOVSYNC=1 OCHRE_STATS=1` for 80 s, three runs, and log the overlay's median in `docs/terrain-perf.md` next to the baseline. If it fails, lower anisotropy first, then paint off for LOD cells beyond a distance.

## Open

- Remaining blocks (compacted rocks, ice, logs) move to materials if the look is accepted, and the atlas then shrinks to leaves, flora and water.
- Bevel, blended boundaries and painterly lighting are separate, later stages.
