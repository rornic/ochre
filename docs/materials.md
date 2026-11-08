# Painted materials

Terrain faces whose tile has a painted material draw it from a 512px layer sampled by world position, so one texture covers many blocks and ignores block edges. Leaves and flora draw RGBA layers of the same array across their tile's own uv (see Foliage below). Only water still draws from the 16px atlas. Design: `docs/designs/painted-world-space-materials.md`.

## What is painted

`Tile::material_layer` (`ochre-base/src/block.rs`) maps a tile to a layer, and `MATERIAL_FILES` lists the layers in order, each `assets/textures/materials/<name>.png`, 512x512 opaque sRGB. Painted: Stone, Dirt, GrassTop, Sand, Snow, Limestone, Clay, Basalt, Marble, Granite, Shale, SandstoneTop, Gravel, Ice, LogBark, LogEnd and the eight compacted rocks. Grass sides draw the GrassTop layer, as the top does, tinted the same, sandstone sides draw SandstoneTop and snow sides draw Snow. Each compacted rock has its own layer, its rock's layer at 85% brightness, made by `materials.py` so it stays recognisably packed harder. `MATERIAL_LAYER_TABLE` in `block_texture.wesl` is the shader's copy by atlas cell, and a test in `material.rs` checks it against `material_layer`.

GrassTop is greyscale at the mean `GRASS_TEXTURE_MEAN` assumes, because the biome tint multiplies it. `block_mean` rows for painted tiles come from their layers (`materials.py`), and a test checks every row against its texture.

## Foliage

Leaves and ground flora are the last painted tiles: `Tile::foliage_layer` maps them to RGBA layers 24 to 30 of the materials array (`leaves`, then `tall_grass`, `fern`, `poppy`, `dandelion`, `short_grass`, `seed_grass`, in `FIRST_FLORA` order). They aren't projected by world position: the depth and shadow passes have no normals to choose a plane by, and they must cut holes exactly as the main pass does. So they keep their tile uv. The leaves layer is seamless and repeats every block, its holes the painted gaps between leaf clumps; a plant's sprite is drawn once across its quad, clamped half a texel in.

- Alpha is cut in `block_alpha_at` (`block_texture.wesl`), by the main pass, the depth prepass and the sun's shadow pass alike, at a mip level that depends on distance, `foliage_level` for the camera's view and a blurrier one for shadow maps (`cut_level` in `world_prepass.wesl`). A 512px sprite is far finer than a pixel past a block or two, so cut at full detail it would shimmer. `foliage_cut` scales alpha up as the level climbs so thin leaves and stems still pass the 0.5 cut when blurred: a meadow stays a meadow a few blocks off, and distant crowns go solid, as the shadows already did.
- Colour mips are alpha weighted (`mip_tiles`), so a hole doesn't darken what's left. Clear texels carry the colour of their neighbours, so bilinear lookups don't bleed a dark edge.
- The tinted layers (leaves, grass, short grass, seed grass, fern) are greyscale at the mean the climate tint assumes: 0.25 linear for leaves (`LEAF_TEXTURE_MEAN`), 0.5 for the grasses, 0.3 for the fern. Poppy and dandelion keep their colours. Tests check this, the sprites' clear top and sides, and the leaves' coverage.
- `foliage.py` makes them: leaves from an SDXL candidate with holes cut where it is darkest, plants as brush strokes drawn at 2x and filled with a painting's tone, since SD can't draw a lone plant on transparency.
- The atlas cells for these tiles are no longer drawn, but stay in `blocks.png` as placeholders, and `block_mean` rows come from the layers.

## How it draws

- The vertex shader (`world.wesl`) picks the plane from the normal (top faces use world xz, sides xz or zy with up as the texture's up) and writes block-unit coordinates into `uv`. They are the local position plus the chunk origin reduced modulo the period, which is exact in f32 because the period is a power of two. Atlas faces keep tile counts in `uv`. Coarse LOD cells draw their mean, and tree boxes their leaves layer.
- The fragment shader takes one array sample per painted fragment, or the layer's mean once a block is under a pixel (`MEAN_BLOCK_RATE`). Slow world-space noise (`drift`) shifts brightness and warmth to hide the repeat, with no extra sample.
- The mesher and vertex attributes are unchanged, so `bench` vertex counts do not move.

## Lighting

Every face, painted or not, is lit by the painterly model (`painterly.wesl`): wrapped sun, soft shadows, cool shadow fill, a saturation gain and a gentler AO, with a warmer haze and grade. It is on by default and described in `docs/designs/painterly-lighting.md`. `painterly_lighting = false` restores the previous lighting.

## Settings

`[renderer]` in `assets/settings.toml`: `material_period` (blocks before the texture repeats, a power of two up to 64, default 8), `material_anisotropy` (1 to 16, default 8) and `painterly_lighting` (default true). All need a restart. The sandbox reads `OCHRE_RESOLUTION=WxH` and `OCHRE_NOVSYNC=1` to read frame times at a fixed size.

## The fps gate

A change passes if the median frame time at spawn stays at 16.7 ms or lower (60fps) at 1920x1080 on the RTX 2060. Run `OCHRE_SEED=1692045686 OCHRE_RESOLUTION=1920x1080 OCHRE_NOVSYNC=1 OCHRE_STATS=1` for 80 s, three runs, and log the overlay's median in `docs/terrain-perf.md` next to the baseline. If it fails, lower anisotropy first, then paint off for LOD cells beyond a distance.

## Open

- `blocks.png` still holds every tile, painted ones included, which the mesher and `block_mean` index by cell. Shrinking it to leaves, flora and water needs a new cell order.
- A log's end grain is a world-space layer, so its rings aren't centred on the trunk, and a chamfered trunk top blends the end grain, not the bark, into the wall at its rim.
- Leaves repeat per block, so a faint block grid shows in near canopies, and tree crowns are still cubes. Painting them in world space needs normals in the shadow pass or a mesher change.
- Blended material boundaries are not built, and the biome is not available to the shaders, so painterly constants can't vary per biome yet.
