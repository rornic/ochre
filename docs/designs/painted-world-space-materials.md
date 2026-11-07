# Painted world-space materials (stage 1)

*   **Author:** Rory Nickolls
*   **Status:** Draft
*   **Date:** 2026-11-04

--------------------------------------------------------------------------------

## Overview

Ochre draws every terrain face with a 16px tile from `assets/textures/blocks.png`. Greedy meshing repeats that tile once per block across merged faces, so the block grid shows through everywhere. Stage 1 replaces that for a first set of materials with painted, seamless 512px textures sampled by world position, so one texture covers many blocks and ignores block edges.

The first set is stone, dirt, grass top, sand and snow. The texture period (how many blocks pass before it repeats) is configurable and starts at 8 blocks. Terrain stays cubic and fully voxel. The mesher, lighting, leaves, flora, water, far LODs and every block outside the first set stay as they are. If the first set looks right to the owner, the remaining blocks follow in the same stage.

This document covers stage 1 only. Painterly lighting is a separate design after stage 1 lands, and the bevel and blended boundaries wait for a re-evaluation after that. The design notes for those live in `concepts_brief.md`.

## The Problem

The terrain reads as Minecraft for three reasons. Each block face carries its own tile, so the grid is visible at every distance where tiles are resolved. The tiles are pixel art at 16px. And surface variety comes only from block type, so a plain of grass is one small tile repeated every block.

The fix is a larger, painted texture that is looked up by world position, not by block. The cost is on the GPU. The fragment stage already runs shadow cascades, cloud shadow, point-light glow and fog, and the owner's RTX 2060 holds 60 to 100fps median at spawn at 1920x1080 today. The change must stay at or above 60.

## Goals

*   The five first materials draw from 512px layers of a texture array, sampled in world space with box projection. Top faces use world xz, and side faces use xy or zy by normal. Each fragment takes one texture sample.
*   The repeat period is a setting, starts at 8 blocks, and is an integer number of blocks so the texture is continuous across block, chunk and full-detail LOD borders.
*   A slow world-space brightness and hue drift hides the repeat. It adds no texture samples.
*   Material boundaries are hard cuts along block edges.
*   Blocks outside the first set, leaves, flora, water and far LODs render exactly as before, from the existing atlas.
*   Mesher output does not change. `cargo run --release --example bench` prints identical vertex counts before and after.
*   Median frame rate at spawn, at 1920x1080 on the RTX 2060, stays at or above 60. A baseline is recorded before any engine change, and the stage-1 result is logged in `docs/terrain-perf.md`.
*   The `block-textures` skill produces the five 512px materials, writing one PNG per material that the game loads and stitches into a texture array at startup.
*   If the owner accepts the look, every remaining block texture (the eight compacted rocks, limestone, clay, basalt, marble, granite, shale, sandstone, gravel, ice and logs) moves to painted materials, and the atlas shrinks to leaves, flora and water.

## Non-Goals

*   Painterly lighting. It is stage 2, with its own design.
*   The shader-drawn bevel and blended material boundaries. Both wait for the re-evaluation after stage 2.
*   Triplanar blending. Cube faces are axis-aligned, so it reduces to box projection at three times the sample cost.
*   Stochastic tiling. It is the follow-up only if a distinctive feature still shows its repeat at spawn after the drift is in.
*   Painted leaves, flora or water, and any change to `lighting.wesl`.
*   The grass-side fringe. While it is skipped, a grass block's side draws dirt.
*   Painted texture on LOD cells larger than 4 blocks. They keep their flat mean colour, now from the painted means.
*   Non-cubic terrain, smoothed surfaces and geometric bevels.
*   Long-term backwards compatibility with the 16px look for the painted materials.

## Open Questions

*   **Near and far colour match**: Far faces use `block_mean` rows measured from the 16px tiles. Painted means that differ would pop in colour at the near and far transition.
*   **Grass-top tint**: `GRASS_TEXTURE_MEAN = 0.5` assumes a greyscale tile. A coloured painted grass top would be tinted twice, so it needs to be greyscale at that mean or take another route.
*   **Art quality**: Whether SD1.5 on a 6GB card produces a delit, low-contrast painted result at 512px. A painterly checkpoint or LoRA may be needed, and the skill bans the Plixel LoRA.
*   **Judging**: The owner judges the look. The fixed view set (seed, spawn, time of day) is not chosen yet.
*   **Unverified**: Whether `Image::new` with a `D2Array` view descriptor builds the array that `#[texture(12, dimension = "2d_array")]` expects in Bevy 0.20, and that layer-major data with each layer's mips in turn is the order it uploads. The `2d_array` fallback image when `ShaderInputs::materials` is `None` (the headless examples). Bevy's sampler and texture limits alongside the existing bindings. Whether chunk and LOD group meshes always have whole-number origins.
*   **Measurement protocol**: The sandbox runs vsynced at the default window size, so median fps there is capped by the monitor. The plan adds two env vars to the sandbox so the gate can be read unsynced at 1920x1080 from the overlay's frame time, as `docs/terrain-perf.md` already does. The owner should confirm that is how the gate is read.

## Critical User Journeys (CUJs)

### CUJ 1: Player sees painted ground at spawn

*   **Given**: The sandbox runs at 1920x1080 on a fixed seed with the period at 8 blocks.
*   **When**: The player spawns and looks at grass, dirt, stone, sand and snow.
*   **Then**: No per-block tile boundaries show inside a surface. The texture is continuous across chunk borders. Median frame rate at spawn is at least 60.

### CUJ 2: Owner tunes the repeat period

*   **Given**: The sandbox is built with the five materials.
*   **When**: The owner edits the period in `assets/settings.toml` and restarts.
*   **Then**: The texture covers a different number of blocks, with no asset rebuild.

### CUJ 3: Developer checks nothing else moved

*   **Given**: The branch with stage 1 applied and the baseline from before it.
*   **When**: The developer runs `cargo run --release --example bench` and `cargo test --release --workspace`.
*   **Then**: Vertex counts match the baseline exactly and the tests pass.

## Technical Plan (Architecture)

**Decision (owner, 2026-11-04): texture coordinates are computed in the vertex shader.** A painted face gets its world-plane coordinates in `vertex()` in `assets/shaders/world.wesl`, written into the `uv` varying. The fragment shader only samples. Mesher output does not change, so `bench` vertex counts stay identical.

**Data.** Each painted material is its own 512x512 opaque sRGB PNG in `assets/textures/materials/`, named for its tile (`Stone.png`, `Dirt.png`, `GrassTop.png`, `Sand.png`, `Snow.png`), so one material is edited, re-rolled and committed alone. A new loader in `crates/ochre-terrain/src/material.rs` decodes each file, builds its mips box-filtered in linear light, and assembles the 2D array layer by layer in `Tile::material_layer` order. Alpha weighting is not needed because the layers are opaque. Its sampler is linear for magnification, minification and mips, repeats on both axes, and takes its anisotropy from a setting. `ChunkMaterial` gets a second texture and sampler binding for it. The atlas and its `Nearest` sampler stay for everything unpainted.

**Which faces are painted.** A new `Tile::material_layer(self) -> Option<u8>` in `crates/ochre-base/src/block.rs` is the one source of truth. Stage 1 maps Stone to layer 0, Dirt to 1, GrassTop to 2, Sand to 3 and Snow to 4, and GrassSide to 1 (dirt). Every other tile returns `None`. `block_texture.wesl` holds a 35-entry copy of the table by atlas cell, with -1 for unpainted, and a test compares it with the Rust function the way `shader_means()` checks `block_mean`.

**Vertex stage.** In `vertex()`, a face whose atlas cell (read from `vertex.color.a`, as `swayed` already does) has a layer takes these steps.

1.  Choose the plane from the face normal. Top and bottom faces use (x, z). Faces with a mostly-x normal use (z, y), and mostly-z faces use (x, y), so side textures always have world up as their v axis. The mesher's own tangent order (`uv_tangent` in `greedy.rs`) is ignored for these faces.
2.  Compute each coordinate in block units as the local position plus the chunk origin reduced modulo the period. The period is a power of two, so the reduction is exact in f32 and nothing shimmers far from the origin. The origin comes from the mesh transform's translation, which is a whole number of blocks.
3.  Write the two coordinates into `out.uv`. They are measured before the swing and the bend, so no `uncurved` call is needed. Block units keep `block_rate` in `world.wesl` meaning blocks per pixel.

Faces without a layer keep their tile counts in `uv`. Leaves and flora stay in that group permanently, so `uv` keeps two meanings for them after stage 1 as well as during it.

**Fragment stage.** A painted face (the same cell lookup) takes a new path.

*   It takes the footprint from the derivatives of `uv` divided by the period, then one `textureSampleGrad` on its layer. Derivatives come from uniform control flow, as `block_footprint` does today.
*   Far faces keep the mean-colour shortcut. When `block_rate` is at least `MEAN_BLOCK_RATE`, the shader uses the layer's mean colour and skips the sample. Most pixels of a landscape are far ground, so this is what keeps the added texture cost to the near field.
*   The vertex colour (ambient occlusion and the grass tint) multiplies the sample, as now. A painted grass top must therefore be greyscale at the mean `GRASS_TEXTURE_MEAN` assumes, or it would be tinted twice.
*   Grass sides draw the painted dirt layer shaded by `-uv_b.y`, the dirt shade the mesher already stores, with no fringe. The far fade to grass top is kept, so distant slopes keep their green. Without it, hillsides would turn brown at a distance.
*   Brightness and hue drift multiplies the colour by a slow noise of world xz, which the bend does not change, using the helpers in `hash.wesl`. It adds no texture samples.

**LOD cells (owner decision, 2026-11-04).** Textured LOD cells of 2 and 4 blocks take the painted path too, so painted ground continues past the full-detail ring with no seam. Nothing extra is built for this. Cells above 4 blocks keep drawing their flat mean colour, now from the painted means. If the fps gate fails, painting these cells off beyond a distance is the first lever, and it needs a flag the vertex shader does not have today.

**Settings.** `[renderer]` gains `material_period` (power of two, default 8) and `material_anisotropy` (default 8, tunable against the fps gate). The period becomes a shader def `MATERIAL_PERIOD` through `ChunkMaterial::specialize` and joins `ChunkMaterialKey`, so changing it needs a restart. Anisotropy is baked into the array's sampler when it loads.

**Colour match.** `pack.py` rewrites the `block_mean` rows for the five painted cells from the 512px sources, so far faces, `patch_tint` and the grass fade agree with what the painted materials average to. `material.rs`'s means test keeps checking the table against the atlas, so the atlas cells for those tiles must carry the same means.

**What does not change.** The mesher and every vertex attribute, the prepass and shadow shaders (painted faces are opaque, so they never alpha-test), leaves, flora, water, and every tile outside the first set.

```text
Tile::material_layer (ochre-base)        assets/textures/materials/<Tile>.png
        |                                           |
        v                                           v
 shader layer table  ---+            material_array loader -> 2D array + mips
 (block_texture.wesl)   |                                    |
                        v                                    v
 vertex(): cell -> layer? -> plane from normal -> coords = local + origin mod period -> out.uv
                        |
                        v
 fragment(): layer? -> block_rate < MEAN_BLOCK_RATE ? array sample : layer mean
                    -> drift(world xz) -> x vertex colour -> light, fog (unchanged)
```

## Alternatives Considered

### Alternative A: Compute coordinates in the fragment shader from `uncurved` world position

*   **Pros**: No vertex-stage changes, no new use of `uv`, and the plane choice and coordinates live in one function. About a dozen arithmetic operations per fragment, with no memory reads.
*   **Cons**: Position precision limits it. The f32 ulp at x=32768 is about 0.004 blocks, a quarter texel at 64 texels per block, so the texture shimmers by that much near the world edge as the camera moves.
*   **Reason for Rejection**: The owner chose the vertex shader. It is exact anywhere in the world and puts the work on vertices, which are far fewer than fragments.

### Alternative B: Compute coordinates in the fragment shader from a local-position varying

*   **Pros**: Exact like the vertex option, and `uv` keeps its current meaning for every face.
*   **Cons**: An extra interpolated varying and a per-fragment instance lookup on a stage that is already heavy. It also changes the vertex output struct shared by the main, prepass and shadow shaders.
*   **Reason for Rejection**: It is the heaviest of the three options per fragment, and the owner chose the vertex shader.

### Alternative C: Triplanar blending of three projections

*   **Pros**: Handles surfaces at any angle without stretching.
*   **Cons**: Cube faces are axis-aligned, so it reduces to one projection per face while costing three samples.
*   **Reason for Rejection**: It gives nothing on cubes. Blending is only needed where normals are not axis-aligned, which stage 1 does not have.

### Alternative D: Grow the existing atlas with 512px tiles

*   **Pros**: One texture and one binding, and no new loader.
*   **Cons**: The atlas is a horizontal strip with hand-built mips (`mip_tiles`), an inset to stop mip bleed (`TILE_INSET`) and a `Nearest` magnifying sampler, all sized for 16px tiles. A 512px strip would be 35 tiles wide, and none of those mechanisms fit it.
*   **Reason for Rejection**: A texture array removes the strip layout and the mip-bleed handling, and the atlas stays in place for the tiles that remain on it.

### Alternative E: Use 1024px layers

*   **Pros**: Twice the texel density, at 128 texels per block with an 8-block period.
*   **Cons**: About 5.6 MB per layer with mips against about 1.4 MB, and more cache pressure at the near-field fragments, with a 60fps floor on a 2060. SD1.5 is trained at 512px, so 1024px output needs an upscale pass. At 1080p the extra density shows only within about 4 blocks of the camera.
*   **Reason for Rejection**: The cost is certain and the benefit is limited to the few blocks around the player's feet. The resolution is a pipeline parameter, so it can be revisited if the proof looks soft up close.

### Alternative F: Smoothed voxel surfaces (marching cubes, surface nets)

*   **Pros**: Removes cubic silhouettes as well as the tile grid.
*   **Cons**: Reaches the whole terrain crate, since the mesher in `generate/generator/` and the octree LODs assume cubes. The terrain would no longer be block-addressable.
*   **Reason for Rejection**: The owner wants the terrain to stay fully voxel.

### Alternative G: Paint leaves, flora and water in stage 1

*   **Pros**: A single look across the whole scene.
*   **Cons**: Leaves and flora are per-tile cutouts with their own sway and prepass handling, and water has its own shader and samples the atlas directly. Each is a separate piece of work.
*   **Reason for Rejection**: Deferred by the owner to keep stage 1 to terrain cubes.

### Alternative H: Draw unpainted blocks as the painted stone layer, desaturated and tinted by `block_mean`

*   **Pros**: Avoids 16px tiles next to painted ones on cliffs, cuts and cave walls, which are the compacted rocks.
*   **Cons**: It only approximates each rock's look, and adds a tint path to the shader that is deleted once every block is painted.
*   **Reason for Rejection**: The owner chose the old atlas for unpainted blocks during the proof, and converts every remaining block immediately if the proof is accepted.

### Alternative I: Stochastic tiling in stage 1

*   **Pros**: Hides repeats of distinctive features, such as a blotch or a crack, which drift cannot hide.
*   **Cons**: Two or three offset samples per fragment instead of one, a real cost against the 60fps floor.
*   **Reason for Rejection**: Drift costs no samples, so it goes first. Stochastic tiling follows only if a distinctive feature still shows its repeat at spawn, and it is measured against the fps gate.

### Alternative J: Keep the grass-side fringe

*   **Pros**: Grass blocks keep their green edge on cliff and cut faces.
*   **Cons**: The fringe is a greyscale overlay on dirt with its own mesher path (`tint_grass_sides`) and a shader branch, and it was drawn at 16px.
*   **Reason for Rejection**: Skipped for stage 1 by the owner. Grass sides draw painted dirt, and a painted fringe is later work.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

Work in this order. Phase 0 changes no behaviour and Phase 2 changes only what is drawn, so `bench` counts must match the Phase 0 baseline after every phase. Do not mix phases in one commit.

0.  **Baseline.** Before any change, record the numbers in "Verification Plan" and log them as a row in `docs/terrain-perf.md`.
1.  **Plumbing.** The array binding, loader, settings and layer table, with placeholder layers, drawing nothing different.
2.  **Painted path.** The vertex and fragment shader changes and the drift, on placeholder layers first.
3.  **Art.** The pipeline and the five real layers.
4.  **Gate and judging.** The fps measurement and the owner's visual review. If accepted, the remaining blocks follow (see Follow-up).

### Target Files to Create/Modify

#### 1. `crates/ochre-base/src/block.rs` (Modify)

*   **Type / Scope**: Rust library, block and tile definitions.
*   **Proposed Changes**: Add `Tile::material_layer(self) -> Option<u8>`. Stone is 0, Dirt 1, GrassTop 2, Sand 3, Snow 4, GrassSide 1 (it draws dirt while the fringe is skipped), and every other tile `None`. Add `pub const MATERIAL_FILES: [&str; 5] = ["Stone", "Dirt", "GrassTop", "Sand", "Snow"]` in layer order, the single list of what is painted and what files to load, and `pub const MATERIAL_LAYERS: usize = MATERIAL_FILES.len()`. Add tests that each tile named in `MATERIAL_FILES` has `material_layer` equal to its index, that none repeats except Dirt and GrassSide, and that `Tile::SeedGrass` and the flora, leaves and water tiles return `None`.

#### 2. `crates/ochre-terrain/src/material.rs` (Modify)

*   **Type / Scope**: Rust, the chunk material and its loaders.
*   **Proposed Changes**:
    *   Add `pub fn material_array(dir: &str, anisotropy: u16) -> Image`. For each name in `MATERIAL_FILES` it decodes `<dir>/<name>.png` as `block_atlas` does, asserts it is 512x512 RGBA8 sRGB and fully opaque, and calls `mip_tiles` on it for ten levels, which leaves that layer's mips one after another. It concatenates the layers' bytes in layer order, which is layer-major, and builds one `Image` of size 512x512x`MATERIAL_LAYERS` with ten mip levels and a `D2Array` texture view descriptor. Its sampler has linear magnification, minification and mips, repeat addressing on both axes, and `anisotropy_clamp = anisotropy`. A missing or wrong-sized file panics with its path, as `block_atlas` does. Startup does the decode and mips for every layer, which is unmeasured and should be timed in Phase 1.
    *   Add `pub materials: Option<Handle<Image>>` to `ShaderInputs`, and a `pub materials` field on `ChunkMaterial` with `#[texture(12, dimension = "2d_array")]` and `#[sampler(13)]`. Bindings 12 and 13 are used by no shader in `assets/shaders/`. 7 is free but the cloud shaders declare 9 and 10 for other materials, so 12 and 13 avoid any chance of a clash through imports.
    *   Add `pub period: u32` to `ChunkMaterial` and to `ChunkMaterialKey`, copied in `From<&ChunkMaterial>`. In `specialize`, add a `MATERIAL_PERIOD` `UInt` shader def. `TerrainMaterials::new` takes the period (see item 4), and `fade.rs` copies the base material with `..base_chunk.clone()`, so it needs no change.
    *   Tests. Add one that the array has `MATERIAL_LAYERS` layers of 512x512, ten mip levels and the data length the descriptor implies. Add one that parses the `MATERIAL_LAYER_TABLE` line in `block_texture.wesl` and compares it with `Tile::material_layer` for every tile. Change `test_block_means_match_the_atlas` so cells with a layer compare the shader row with the mean linear colour of that layer's file in `assets/textures/materials/`, and every other cell keeps comparing with `blocks.png`. `shader_means()` finds rows by lines that start with `vec3<f32>(`, so the layer table must not use that prefix.

#### 3. `crates/ochre-terrain/src/settings.rs` (Modify)

*   **Type / Scope**: Rust, renderer settings.
*   **Proposed Changes**: Add `material_period: u32` (default 8) and `material_anisotropy: u16` (default 8) to `RendererSettings` with one-line doc comments, and to its `Default`. Add `pub fn material_period(&self) -> u32` that panics with a clear message unless the value is a power of two from 1 to 64, since the exact coordinate reduction in the vertex shader needs it. Clamp anisotropy to 1 through 16.

#### 4. `crates/ochre-terrain/src/lib.rs` (Modify)

*   **Type / Scope**: Rust, `insert_terrain_resources`.
*   **Proposed Changes**: Pass `renderer.material_period()` into `TerrainMaterials::new`, whose signature gains a `period: u32` parameter. Fill the new `materials` and `period` fields of both `ChunkMaterial`s from `inputs.materials` and that period.

#### 5. `src/settings.rs` (Modify)

*   **Type / Scope**: Rust, the engine's `Settings` and its tests.
*   **Proposed Changes**: Extend `test_missing_renderer_settings_use_defaults` with the two new defaults. Extend `test_settings_file_parses` to read `material_period = 8` from `assets/settings.toml`. Add a test that a period that is not a power of two is rejected by `material_period()`.

#### 6. `assets/settings.toml` (Modify)

*   **Type / Scope**: Config.
*   **Proposed Changes**: Under `[renderer]`, add `material_period = 8` and `material_anisotropy = 8`, each with a one-line comment (blocks before a painted texture repeats, a power of two, and the anisotropic filtering taps).

#### 7. `assets/shaders/block_texture.wesl` (Modify)

*   **Type / Scope**: Shader, bindings and sampling shared by the main and prepass shaders.
*   **Proposed Changes**: Declare the array texture and its sampler at bindings 12 and 13. Add `MATERIAL_LAYER_TABLE`, 35 integers in atlas order with -1 for unpainted and the five layer numbers, and `material_layer(cell)` that returns the layer for an atlas cell (read through a local copy, as `block_mean` does for its table). Add `material_sample(uv, layer)`, which divides the block-unit coordinates by `MATERIAL_PERIOD`, takes the footprint from the derivatives of that value, and calls `textureSampleGrad` on the array. Leave `block_alpha`, `block_footprint` and `block_sample` unchanged for leaves, flora and unpainted tiles. `block_mean` rows for the five painted cells are rewritten by the art pipeline (item 14).

#### 8. `assets/shaders/world.wesl` (Modify)

*   **Type / Scope**: Shader, the main pass vertex and fragment stages.
*   **Proposed Changes**:
    *   Vertex. After the local-to-world step and before `swayed` and `curved`, look up `material_layer(vertex.color.a)`. If it is non-negative, pick the plane from the normal as in Technical Plan, compute each coordinate as the local position component plus `fract(origin / period) * period` of the mesh transform's translation, and write both into `out.uv`. Otherwise keep `out.uv = vertex.uv`.
    *   Fragment. If the face's layer is non-negative, take the painted path. When `block_rate` is at least `MEAN_BLOCK_RATE`, use the layer's `block_mean` row, otherwise `material_sample`. Set `color.a = 1`. Apply the drift to `rgb` (item 9). Skip the leaf, flora, LOD-patch and tree-box branches for painted faces, since none applies to a painted cell. For faces with `uv_b.y < 0` (grass sides), `near` is the painted dirt layer times `-uv_b.y`, with no fringe lookup. The far fade to `grass_top` stays as it is, and the grass top's mean comes from its layer row.
    *   Keep the unpainted path byte for byte as it is, so every unpainted face draws as before.

#### 9. `assets/shaders/hash.wesl` (Modify)

*   **Type / Scope**: Shader helper.
*   **Proposed Changes**: Add `value_noise(p: vec2<f32>) -> f32` that smooths `hash2` between the four surrounding cells. `world.wesl` uses it twice, at wavelengths of 37 and 91 blocks (not multiples of the period), for a brightness factor of up to plus or minus 10% and a hue shift that warms red and cools blue by up to 4%. Both amplitudes are `const`s at the top of `world.wesl` to tune at the spawn view. The input is `in.world_position.xz`, which the bend does not change.

#### 10. `crates/ochre-terrain/src/generate/generator/greedy.rs`, `mesh.rs`, `buffers.rs`, `crates/ochre-terrain/src/pack.rs`, `group.rs`, `shadow_mesh.rs`, `assets/shaders/world_prepass.wesl`, `assets/shaders/water.wesl`, `crates/ochre-terrain/src/fade.rs` (No change)

*   **Type / Scope**: Meshing, vertex packing, other passes.
*   **Proposed Changes**: None, and the verification below proves it. The mesher and every vertex attribute are untouched, painted faces are opaque so the prepass and shadow shaders never alpha-test them, water has its own bindings, and `fade.rs` copies `ChunkMaterial` with `..base`.

#### 11. `examples/sandbox.rs` (Modify)

*   **Type / Scope**: Reference game.
*   **Proposed Changes**: Load the array into `ShaderInputs.materials` with `material_array("assets/textures/materials", renderer.material_anisotropy)`. Add two env vars for the fps gate. `OCHRE_RESOLUTION=1920x1080` sets the window size. `OCHRE_NOVSYNC=1` selects `AutoNoVsync`, which today only a flythrough gets. Both default to current behaviour. The existing comment warns that an uncapped GPU can starve the desktop's compositor, so the gate runs are short and the new flag says so.

#### 12. `assets/textures/materials/Stone.png`, `Dirt.png`, `GrassTop.png`, `Sand.png`, `Snow.png` (Create)

*   **Type / Scope**: Assets, five opaque 512x512 sRGB PNGs, one per painted tile, in the directory `material_array` reads.
*   **Proposed Changes**: Phase 1 and 2 use any placeholder images (for example a flat colour per tile plus a coarse grid so the repeat and seams are visible). Phase 3 replaces them with the pipeline's output. Texels carry no baked lighting, the GrassTop layer is greyscale with a mean of `GRASS_TEXTURE_MEAN`, and the repeat period is 8 blocks.

#### 13. `.claude/skills/block-textures/generate.py` and `block_texture.json` (Modify)

*   **Type / Scope**: Python pipeline and the ComfyUI graph.
*   **Proposed Changes**: Add a painted mode, `python generate.py --painted Stone Dirt GrassTop Sand Snow`. It keeps the 512px render, replaces the pixel-art `STYLE` with a painted, low-contrast, unlit prompt, skips the 16px Lanczos downsample and four-colour quantise in `finish()`, and writes `out/512/<Tile>.png` after normalising only mean and contrast. GrassTop is converted to greyscale at the required mean. `reference()`, `derive()`, `tiled()` and `sheet()` stay for the 16px path. The `block_texture.json` changes are the checkpoint name and sampler settings only if a painterly checkpoint or LoRA is needed, which the art go/no-go decides. Add a `PAINTED` list that defines the layer order.

#### 14. `.claude/skills/block-textures/materials.py` (Create)

*   **Type / Scope**: Python, validates and installs the painted PNGs and updates the means.
*   **Proposed Changes**: Read `out/512/<Tile>.png` in `PAINTED` order, assert each is 512x512 and opaque, assert the GrassTop mean, copy each into `assets/textures/materials/<Tile>.png`, rewrite the five matching `block_mean` rows in `assets/shaders/block_texture.wesl` from the layers' mean linear colour (leaving the other 30 rows alone), and write a 3x3 contact sheet at 4 blocks per tile. `pack.py` stays for the 16px atlas and still asserts its length.

#### 15. `.claude/skills/block-textures/SKILL.md` (Modify)

*   **Type / Scope**: Skill documentation.
*   **Proposed Changes**: Add the painted path (`--painted`, `out/512/`, `materials.py`), the mean and layer-order rules, and the note that `blocks.png` stays for unpainted tiles until the follow-up. Fix the "one 16px-tall strip" statement to say the atlas covers unpainted tiles.

#### 16. `docs/materials.md` (Create)

*   **Type / Scope**: Documentation, the one home for these decisions.
*   **Proposed Changes**: Cover scope and staging, the `uv` meanings (tile counts for atlas faces, block-unit plane coordinates for painted faces), the layer table, the settings and their limits, the fps gate and its protocol, the judged view set, and the rule that the loser of each comparison is deleted in the same change as the decision. Link it from `AGENTS.md`.

#### 17. `AGENTS.md` (Modify)

*   **Type / Scope**: Project instructions.
*   **Proposed Changes**: Update the Shaders and block textures line to name `assets/textures/materials/` and `docs/materials.md`. Add `OCHRE_RESOLUTION` and `OCHRE_NOVSYNC` to the env var list.

#### 18. `docs/terrain-perf.md` (Modify)

*   **Type / Scope**: Performance log.
*   **Proposed Changes**: Add a baseline row before Phase 1 (`bench` counts, frame time medians) and a stage 1 row after Phase 3, with the reason for any p99 regression, as the file's rule requires.

### Verification Plan

<!-- Order matters: baseline first. -->

*   **Baseline (before any change)**: `cargo run --release --example bench` (default seed, then `-- 7 lod`, then the existing view set with `BENCH_AT`, per `docs/terrain-perf.md`). `cargo run --release --example sandbox` with `OCHRE_SEED=1692045686`, `OCHRE_TIME` fixed, `OCHRE_RESOLUTION=1920x1080`, `OCHRE_NOVSYNC=1`, `OCHRE_STATS=1` once the sandbox change is in. Read the overlay's median frame time and GPU pass times over three 80-second runs at spawn. The baseline is the owner's current 60 to 100fps, recorded here as numbers.
*   **Compilation/Build**: `cargo build --release --workspace`.
*   **Unit Tests**: `cargo test --release --workspace`, including the new layer-table, array and means tests.
*   **Static Analysis & Linting**: `cargo clippy --workspace --all-targets`.
*   **Mesher unchanged**: `bench` vertex counts identical to the baseline after every phase.
*   **Headless examples**: `lod_sim`, `flight` and `shadow_cull` run with `ShaderInputs::default()`, which proves the array fallback when `materials` is `None`. If the fallback fails, make the binding optional or load a 1x1 array.
*   **fps gate**: Phase 2 and 3 repeat the baseline run. The change passes if the median frame time stays at 16.7 ms or lower (60fps) and the result is logged next to the baseline. Run anisotropy at 1, 4, 8 and 16 once at the spawn view, and keep the highest that passes. If the gate fails, the levers in order are anisotropy, then painting off for LOD cells beyond a distance, then stochastic tiling stays out.
*   **Visual review (owner)**: Passive screenshots at spawn, period 8 and 16, no grabbing focus or input. Checks are no tile edges inside a surface, continuity across chunk borders and where full-detail chunks meet LOD cells, grass sides as dirt with green slopes at a distance, no colour pop at the near and far transition, and whether any distinctive feature visibly repeats. A visible repeat triggers stochastic tiling as a separate follow-up.
*   **Test Coverage Requirement**: The three new tests above, plus the unchanged existing ones.
*   **E2E/Integration**: The sandbox runs the five materials in a fixed view set, and `OCHRE_FLYTHROUGH` records a pass for the owner's review.

### Follow-up (not part of the first merge)

If the owner accepts the look: add layers and `material_layer` entries for the eight compacted rocks, limestone, clay, basalt, marble, granite, shale, sandstone, gravel, ice and log faces, add a PNG for each under `assets/textures/materials/` and its name to `MATERIAL_FILES`, shrink the atlas and `block_mean` and `LEAF_CELL`-style constants to leaves, flora and water only, delete the `materials`-less fallback path, and update `AGENTS.md`, the skill and `docs/materials.md`.

--------------------------------------------------------------------------------

<!-- Provenance: Indicates initial structural drafting via EGM. -->

*Drafted with EGM*
