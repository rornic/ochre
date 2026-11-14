# Snow and frost as a shader skin

*   **Author:** Rory Nickolls
*   **Status:** v1 implemented, see Implementation Notes
*   **Date:** 2026-11-11

--------------------------------------------------------------------------------

## Overview

Ochre draws snow only as blocks. A column becomes `BlockType::Snow` when it is above the snow line and not too steep, and the snow lies 14 blocks deep. Nothing in the world looks cold until it is already snow. This design adds a **frost skin**: white-blue paint that the surface shader lays over up-facing ground in cold country that is not snow yet, with a thin bright rim around the patches and a faint glitter. The skin has no thickness, changes no vertices, and cannot be dug. It works on any surface, including blocks the player builds.

The first version is shader-only and driven by one temperature and one biome value taken at the camera. Weather, mist, deep snow drifts and accumulating layer blocks come later and are listed under Future Improvements.

## The Problem

The author wants the tundra, and later other biomes, to feel cold in the way Skyrim's tundra does: patches of white over brown ground, frosted rocks, a glitter on cold mornings. Today the only snow is geology. It is a block, so it cannot appear on a rock top in a biome where it was not placed, it cannot show up on a player's roof, and it cannot get thicker with weather.

A shader skin looks like the right tool, but the fragment shader today knows only the world position, the surface normal, a baked sky light, and a tile index. It does not know temperature or biome. The tundra and taiga also thaw ground that the climate alone would snow, so a skin driven by temperature alone would paint white over ground the generator turned into brown grass. The design has to give the shader a temperature and the thaw rule without adding vertex data, which is already full and which would hurt mesh merging on glacier terrain.

## Goals

*   Frost on up-facing ground in cold country that has no snow block, patchy rather than a blanket, with a thin bright rim at patch edges and a world-anchored glitter that does not flicker.
*   Driven by temperature only, using the same temperature and height rule as the snow blocks, so frost thins out and the snow begins where the blocks begin.
*   Respects the tundra and taiga thaw: the biome under the camera scales how much frost appears.
*   Works in any biome and on any surface, including player-built blocks and LOD meshes, with fine detail fading by pixel footprint rather than a distance setting.
*   No change to generated blocks or vertex counts. `GENERATOR_VERSION` stays 93, and `bench` vertex counts are identical before and after.
*   Frame time in warm country does not move. In cold country the cost is a few noise samples per frame pixel on receiving surfaces only.
*   One source of truth for thresholds, in Rust. The shader never keeps its own copy.
*   A kill switch and a debug switch so the effect can be turned off and timed.

## Non-Goals

*   Deep snow, snow layer blocks, drifts, or anything that changes a silhouette. That is a separate design (v3).
*   Weather-driven snowfall or accumulation, and a time-of-day cold offset (v2). Frost depends on temperature only, and temperature will later be driven by time of day.
*   Mist and fog changes. The cold blue-grey light already exists as `cold_color_grading` in `tonemap.rs` and `apply_cold_grade` in `player.rs`, and is not repeated. Mist is deferred (v1.1).
*   Frost on Snow, Ice, water, leaves and flora tiles. They are skipped, which also makes glaciers almost free.
*   A baked world texture (option B), per-vertex data, and a refilled coverage texture. See Alternatives Considered.
*   Persisting anything. The skin is computed every frame from position and the shared values, so nothing needs saving.

## Open Questions

*   **How wide is the cold band?** Grass columns colder than 0.25 are already snow blocks, and tundra grass is thawed on purpose below a thaw line, so the zone that is cold but not snow may be thin (roughly 0.25 to 0.35 in temperature). A `worldprobe` or `worldview` check on seed 4 and one more seed must run before tuning. If the band is thin, the frost threshold is raised so it reaches further below the snow line.
*   **How far do frost and thaw factors read wrong across a biome border?** The camera's biome multiplier is used everywhere, so for about 100 blocks into a neighbour the multiplier is the old biome's. This is accepted for v1. A visible problem in stills is the trigger to move to the baked texture.
*   **Does baked sky light reach LOD and edited blocks?** The skin multiplies by `in.uv_b.x` so caves and interiors stay clean. If LOD vertices or player-edited blocks do not carry it, frost would show under cover there and the first version stops being shader-only. This is checked in the first step of the plan.
*   **Initial thaw factors.** Tundra 0.5 and taiga 0.6 are starting guesses for screenshot tuning, not measured values.
*   **Three lapse rates.** `climate.rs` uses about 1/3500 per block, `snow_line` uses 2600 blocks per unit of warmth, and `docs/terrain-perf.md:65` still says 1/800. The skin follows `snow_line`, and the stale doc line is corrected.

## Critical User Journeys (CUJs)

### CUJ 1: Walking into the tundra

*   **Given**: A player on seed 4 walking from temperate grassland toward the tundra, with the effect on.
*   **When**: The camera's sea-level temperature falls into the frost band.
*   **Then**: Frost appears first on rock tops and bare ground in patches with a bright rim, and thickens toward the colder side. Tundra gets less frost than a mountain at the same temperature, because the biome factor scales it. Nothing flickers as the camera moves.

### CUJ 2: Climbing toward the snow line

*   **Given**: A player on a temperate mountain.
*   **When**: The player climbs.
*   **Then**: Frost starts about a band's height below the snow line, thickens with height, and hands over to snow blocks at the line. Walls and under-hangs stay clean.

### CUJ 3: A player-built tower above the snow line

*   **Given**: A player builds a tower of ordinary blocks.
*   **When**: The tower is tall enough to cross into the frost band.
*   **Then**: Its top faces take frost by their real height, with no generator involvement. Side faces and the inside of a closed room stay clean.

### CUJ 4: A cave under a snow cap

*   **Given**: A cave whose ceiling is under snow blocks.
*   **When**: The player enters it.
*   **Then**: Nothing in the cave is frosted, because baked sky light is near zero there.

### CUJ 5: Turning it off

*   **Given**: `frost = false` in `assets/settings.toml`, or the debug switch `no_frost`.
*   **When**: The game starts.
*   **Then**: The shader path is compiled out or skipped, output matches the build before this feature, and `bench` vertex counts are identical.

## Technical Plan (Architecture)

The skin is a pure function in the fragment shader. Its inputs are the fragment's real height, normal, baked sky light, tile, and world position for noise, plus a handful of values in the shared buffer every land material already carries at binding 8 (the one that holds the curvature, in `curvature.rs`). That buffer is rewritten every frame already and cloned handles share it, so adding values to it touches none of the fade variants and needs no new binding, no new material uniform, and no new texture.

```text
 Rust (ochre-base)           Rust (ochre-terrain)                 Shader (world.wesl)
 -----------------           --------------------                 -------------------
 cold.rs                     frost.rs (new system)                frost.wesl
   SNOW_LINE ...  ---------> reads camera position               frost_amount():
   snow_line(t)              terrain.temperature(x,z)               line = base + (t-0.5)*per_degree
   FROST_BAND                biomes().frost(biome)                  band edge from FROST_BAND
   frost constants           eases both (~2 s)                      * thaw factor (camera)
                             writes into the shared buffer   -->    * up-facing, * sky light
 Biome::frost() (base)       next to the curvature values           * patches (noise), rim, glitter
   tundra 0.5 taiga 0.6                                             * footprint fade
                                                                  mix into tinted colour
                                                                  (before lighting, fog)
```

**How the shader gets its inputs**
- `ochre-base/src/cold.rs` holds the thresholds that define the snow rule: snow line base height, blocks per unit of warmth, maximum temperature, and the frost band height. `terrain.rs` stops keeping its own copies and uses these, so the block rule and the skin cannot drift apart.
- A new system in `ochre-terrain` samples `TerrainSource::temperature(x, z)` and the biome at the camera at about 4 Hz, eases both over about 2 seconds so frost does not jump, and writes them into the shared buffer along with the thresholds. The temperature is the sea-level value the snow rule and the cold grade already use.
- Height is not stored. The shader computes it per fragment from `uncurved(world_position).y`, because raw `world_position.y` is bent by the curvature and would shift the frost with distance.
- `Biome` gains a `frost` method with a default of 1.0. `Plain` sets it from a builder. The tundra and taiga set values below 1.0, and the modifiers pass the inner biome's value through.

**The frost amount at a fragment (pseudocode)**
```text
above_sea    = uncurved(world_position).y - WATER_LEVEL
line         = snow_line(sea_temperature)               // same function as the snow blocks
cold         = smoothstep(line - FROST_BAND, line, above_sea)
cold         = cold * (sea_temperature < SNOW_MAX_TEMPERATURE) * thaw_factor
facing       = smoothstep(0.55, 0.85, smooth_normal.y)  // the flattened lit normal
cover        = facing * sky_light
patch        = value_noise(xz / 11) and value_noise(xz / 47), mixed
amount       = cover * cold * smoothstep(0.35, 0.65, patch + cold - 0.5)
rim          = bright thin band where patch is near the edge threshold
glitter      = world-anchored cell hash, band-limited, faded by pixel footprint
colour       = mix(colour, snow_material_colour, amount) + rim + glitter
```

**Where it goes and what it skips**
- It runs in `world.wesl` after the ambient-occlusion, grass tint and `drift` step, and before lighting and fog, so the skin is lit, shadowed and fogged like the ground under it.
- It skips Snow, Ice, water, leaf and flora tiles through a per-tile receive table that Rust writes into the same shared buffer from `Tile`, so the shader never repeats the tile list. Flora takes the amount evaluated at its base, so brown tufts do not stand on white patches.
- It reads the flattened lit normal, not the raw stored normal, so ramps and chamfers frost correctly and nothing pops at the LOD fade.
- Fine noise, the rim and the glitter fade out by pixel footprint (`block_rate`, the way `MEAN_BLOCK_RATE` already works), keeping only the large patches far away, so the same code serves full-detail chunks and LOD cells.
- When the camera's frost values are zero, the shader takes one uniform comparison and leaves, so warm country costs about nothing.

**What does not change**
- No new vertices, no vertex channel, no mesh path, no generated block, no `GENERATOR_VERSION` bump, nothing in the shadow or depth passes (`world_prepass.wesl` is untouched).
- Water and spray materials import the same buffer module, so the buffer layout change has to build and run for them too.

## Alternatives Considered

### Alternative A: Per-vertex frost value

*   **Pros**: Frost could carry any per-surface data and be exact at biome borders.
*   **Cons**: Every vertex channel is already in use (position, colour alpha as the atlas cell, `uv_b.x` as sky light, `uv_b.y` as the mode band). It would touch about six mesh paths and cannot follow weather without remeshing.
*   **Reason for Rejection**: It hurts greedy merging, which is the cost already visible on glacier terrain, and it cannot change at runtime.

### Alternative B: One value per chunk, or a texture of biome ids

*   **Pros**: Simple to bake.
*   **Cons**: A per-chunk value breaks the shared material and its fade copies and is too coarse. Biome is not temperature, and the shader would need per-biome constants copied from Rust.
*   **Reason for Rejection**: Both break the single source of truth.

### Alternative C: A world coverage texture refilled as the player moves

*   **Pros**: Could carry weather fronts and fast spatial change.
*   **Cons**: The chunk material is cloned into about 260 fade variants, and refreshing an image they all share is a performance cliff.
*   **Reason for Rejection**: Nothing in v1 changes fast enough to need it. If weather needs spatial change later, it can be revisited then.

### Alternative D: A baked static world texture (option B in the brief)

*   **Pros**: Follows real biome borders, carries the thaw bias, and reaches LOD cells exactly.
*   **Cons**: One new texture binding (14 and 15 on `ChunkMaterial`), one extra sample per fragment, and a bake of about 4 million samples that must run off the main thread, with a 1x1 stand-in image until it lands. The headless examples need a stub.
*   **Reason for Rejection**: The author chose to start with the camera-local value and measure. The baked texture is the next step if the border error shows up in stills, if the skin must reach far LOD, or if a thaw bias per map cell is needed.

### Alternative E: Thaw handled by ignoring it

*   **Pros**: No biome input at all.
*   **Cons**: Thawed tundra ground would receive the same frost as a mountain at the same temperature, so tundra would need a low global threshold and could never look properly frosty.
*   **Reason for Rejection**: The author chose a camera-local thaw factor instead.

### Alternative F: Patchy snow from the generator

*   **Pros**: Real blocks, LOD for free, no shader cost.
*   **Cons**: It needs a `GENERATOR_VERSION` bump, gives player blocks no patches, and jittering the 0.25 rule does nothing in tundra, because the thaw rule removes that snow afterward.
*   **Reason for Rejection**: The author asked for a shader-only first pass, and the generator route does not help where the feel is wanted.

### Alternative G: Layer blocks first, weather later

*   **Pros**: Gives the eventual 1 metre drift geometry sooner.
*   **Cons**: Accumulation needs the weather inputs, and layer blocks need their own design (new block types, bevel and seam rules, collision, light, LOD averaging, remeshing).
*   **Reason for Rejection**: Weather inputs come before layer blocks. Layer blocks get a separate design, and the skin's coverage will be defined as snow depth 0 to 1 block so layers replace it instead of doubling it.

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Performance**: The skin must not add a measurable cost in warm country, and in cold country must stay within the fps gate in `docs/materials.md` (1920x1080, RTX 2060, median of 3 runs). Timings are taken on a glacier route, a temperate mountain route and a tundra route, not only the jungle spawn baseline, and logged in `docs/terrain-perf.md`. A shader define `NO_FROST` (the way `STILL_LEAVES` works) allows A/B timing.
*   **Rollout and Rollback**: `RendererSettings.frost` is the kill switch, default on. Rollback is reverting the shader module, the buffer values and the new system. Nothing is stored or generated, so there is no data migration.
*   **Build safety**: There is no shader compile test, so a mistake only shows when the sandbox first draws the material. A headed run is part of the plan.

--------------------------------------------------------------------------------

### Additional Resources

*   `./concepts_brief.md`, the brainstorm and the six-reviewer analysis that this design follows.
*   `./docs/materials.md`, `./docs/climate.md`, `./docs/bevels.md`, `./docs/designs/painterly-lighting.md`.
*   `./crates/ochre-terrain/src/curvature.rs` and `./assets/shaders/curvature.wesl`, the shared buffer this design extends.
*   `./crates/ochre-terrain/src/generate/generator/terrain.rs`, where `snow_line` and the snow block rule live today.

### Future Improvements

*   **Mist (v1.1).** A few lines in `fogged()` from the existing `Surroundings.warmth` signal, judged after frost is seen in stills. Nothing may be bound in `fog.wesl`, which water and cloud pipelines share, and the camera's fog setting must not change because it drives the horizon and LOD render distance.
*   **Baked world texture (v1.5).** The move to option B if border error, thaw bias or far LOD calls for it.
*   **Weather and time of day (v2).** A cold offset and wetness through the same shared buffer. `Weather` only has `rain` today and is not seed-deterministic, so v2 needs a small design for a cold input.
*   **Layer blocks (v3).** Snow up to about 1 metre, as its own design.
*   **Sound.** A frost crunch underfoot, tied to the same amount.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

Order matters: land the tundra branch first, because `world.wesl`, `block_texture.wesl`, `tonemap.rs` and `block.rs` have uncommitted edits on `feat/tundra-uplands`, and this work edits the same files.

### Target Files to Create/Modify

#### 1. `crates/ochre-base/src/cold.rs` (Create)

*   **Type / Scope**: Rust module, constants and pure functions.
*   **Proposed Changes**: Move `SNOW_LINE` (950), `SNOW_LINE_PER_DEGREE` (2600), `SNOW_MAX_TEMPERATURE` (0.62) here from `terrain.rs`, add `FROST_BAND` (blocks below the snow line where frost begins, a starting value set after the cold-band check), `FROST_FACING`, patch scales and glitter constants, and `pub fn snow_line(temperature: f32) -> i64`. Add `pub fn frost_cold(temperature: f32, above_sea: f32) -> f32`, the same arithmetic the shader performs, for tests. Add a `Cold` struct with `fn to_floats(&self) -> [f32; N]` that the buffer writer uses, so Rust is the only place thresholds exist.

#### 2. `crates/ochre-base/src/lib.rs` (Modify)

*   **Type / Scope**: Crate root.
*   **Proposed Changes**: Declare and export `cold`.

#### 3. `crates/ochre-base/src/biome/mod.rs` (Modify)

*   **Type / Scope**: Biome trait and table.
*   **Proposed Changes**: Add `fn frost(&self) -> f32 { 1.0 }` to `Biome`, add `Biomes::frost(id)` with the base-terrain fallback of 1.0, and set `.with_frost(TUNDRA_FROST)` (0.5) and `.with_frost(TAIGA_FROST)` (0.6) where tundra and taiga are registered, next to their thaw constants.

#### 4. `crates/ochre-base/src/biome/plain.rs` (Modify)

*   **Type / Scope**: Data-driven biome.
*   **Proposed Changes**: Add a `frost: f32` field, default 1.0, a `with_frost(f32)` builder and the trait method.

#### 5. `crates/ochre-base/src/biome/modifier.rs` (Modify)

*   **Type / Scope**: `Modified` wrapper.
*   **Proposed Changes**: Forward `frost` to the inner biome, as `climate_shift` and `tree_scale` are forwarded.

#### 6. `crates/ochre-terrain/src/generate/generator/terrain.rs` (Modify)

*   **Type / Scope**: Snow block rule.
*   **Proposed Changes**: Remove its copies of the snow constants and `snow_line`, and import them from `ochre_base::cold`. Behaviour must be identical.

#### 7. `crates/ochre-terrain/src/frost.rs` (Create)

*   **Type / Scope**: Bevy plugin and system.
*   **Proposed Changes**: A `FrostPlugin` with a throttled system (about 4 Hz) that reads the camera position and the `World` terrain, samples `terrain.temperature(x, z)` and `biomes().frost(biome)`, eases both toward the target over about 2 seconds, and writes them with the `Cold` constants into the shared buffer. A zero strength is written when `RendererSettings.frost` is off. Reads the world as an `Option` so headless examples do nothing.

#### 8. `crates/ochre-terrain/src/curvature.rs` (Modify)

*   **Type / Scope**: Shared buffer owner.
*   **Proposed Changes**: Grow the buffer from 8 floats to hold the cold block after the two bends, keep the layout documented in one place, and expose a small writer used by `frost.rs`. Keep the existing curvature values and order unchanged.

#### 9. `crates/ochre-terrain/src/lib.rs` (Modify)

*   **Type / Scope**: Plugin wiring.
*   **Proposed Changes**: Declare `frost` and add `FrostPlugin` with `CurvaturePlugin` in `TerrainRenderPlugin`.

#### 10. `crates/ochre-terrain/src/settings.rs` (Modify)

*   **Type / Scope**: Settings.
*   **Proposed Changes**: Add `RendererSettings.frost: bool` (serde default true) and `DebugSettings.no_frost: bool` (default false).

#### 11. `crates/ochre-terrain/src/material.rs` (Modify)

*   **Type / Scope**: Material pipeline keys and shader defs.
*   **Proposed Changes**: Push a `NO_FROST` shader def when the debug switch is set, the way `STILL_LEAVES` is handled, so the frost branch compiles out for A/B timing. The per-tile receive table is written into the shared buffer, not as shader defs.

#### 12. `assets/shaders/curvature.wesl` (Modify)

*   **Type / Scope**: Shared shader module.
*   **Proposed Changes**: Extend `Bends` with the cold block, add accessors for sea temperature, thaw factor, the thresholds and the receive table. The existing `curved`, `uncurved` and `eye` functions keep their behaviour. Water and spray materials also build against this module and must still compile.

#### 13. `assets/shaders/frost.wesl` (Create)

*   **Type / Scope**: Shader module.
*   **Proposed Changes**: `frost_amount`, the patch noise at two scales, the rim, and the world-anchored glitter, each faded by pixel footprint. Reads only the shared buffer accessors, never biome or per-material data.

#### 14. `assets/shaders/world.wesl` (Modify)

*   **Type / Scope**: Chunk fragment shader.
*   **Proposed Changes**: Import `frost`, and after the tint and `drift` step and before lighting and fog, blend the frost colour using the flattened lit normal, baked sky light (`in.uv_b.x`) and the receive table. Evaluate flora at its base. Skip entirely under `NO_FROST` and when the camera frost strength is zero.

#### 15. `assets/settings.toml` (Modify)

*   **Type / Scope**: Shipped settings.
*   **Proposed Changes**: Add `frost = true` under `[renderer]`. The test that parses the shipped file keeps passing.

#### 16. `src/settings.rs` (Modify)

*   **Type / Scope**: Engine settings parsing and tests.
*   **Proposed Changes**: Extend the settings tests to cover `frost` and `no_frost` defaults.

#### 17. Tests (Create/Modify)

*   **Type / Scope**: Unit tests in `ochre-base`, `ochre-terrain`, and an integration check.
*   **Proposed Changes**:
    *   `cold.rs` tests: `frost_cold` is 0 above `SNOW_MAX_TEMPERATURE`, 0 below the band, rises monotonically to 1 at the snow line, and `snow_line` reproduces the old `terrain.rs` values at several temperatures.
    *   `terrain.rs`: the snow block rule gives the same blocks at a grid of temperatures, heights and slopes as before the move.
    *   `frost.rs` tests: easing reaches the target, returns to exactly zero in warm country, and writes the buffer fields in the documented order.
    *   Biome tests: `frost()` is below 1.0 for tundra and taiga, 1.0 elsewhere, and `Modified` forwards it.
    *   A text test that the shader's buffer struct has the same number and order of fields as the Rust writer, the way `MATERIAL_LAYER_TABLE` is already checked.

#### 18. `docs/materials.md`, `docs/climate.md`, `docs/terrain-perf.md` (Modify)

*   **Type / Scope**: Docs.
*   **Proposed Changes**: In `materials.md`, narrow the line saying the biome is not available to the shaders. In `climate.md`, add the frost rule and the thaw factors. In `terrain-perf.md`, correct the stale 1/800 lapse line and log the before and after timings.

#### 19. `crates/ochre-base/src/README.md`, `crates/ochre-base/src/biome/README.md`, `crates/ochre-terrain/src/README.md` (Modify)

*   **Type / Scope**: Directory contracts.
*   **Proposed Changes**: List `cold.rs`, `Biome::frost`, `frost.rs`, the new buffer fields, and the invariant that thresholds live only in `cold.rs`.

#### 20. `docs/designs/snow-and-frost.md` (Create)

*   **Type / Scope**: The finalized design.
*   **Proposed Changes**: Written from this draft once review is finished.

### Verification Plan

*   **Before building**: run `worldprobe` or `worldview` to measure the width of the cold-but-not-snow band on seed 4 and one more seed, and check whether sky light reaches LOD and edited blocks. Adjust `FROST_BAND` or stop and revise this design if either result breaks an assumption.
*   **Compilation/Build**: `cargo build --release --example sandbox --example worldview`, then a headed sandbox run, since a bad shader only fails when the material first draws.
*   **Unit Tests**: `cargo test --release --workspace`.
*   **Static Analysis & Linting**: `cargo clippy --workspace --all-targets`.
*   **Vertex counts**: `cargo run --release --example bench`, identical vertex counts before and after.
*   **Headless examples**: `lod_sim`, `flight`, `flow_sim`, `shadow_cull`, `visibility` still run, and the `shadow_cull` count is unchanged.
*   **Frame time**: the fps gate in `docs/materials.md` plus a glacier route, a temperate mountain route and a tundra route, with `NO_FROST` on and off, logged in `docs/terrain-perf.md`.
*   **Stills (acceptance artifact)**: passive screenshots at a tundra border, a taiga edge, a temperate hillside, a player-built tower above the snow line, a cave under a snow cap (must stay clean), a cliff and a chamfer, and the LOD fade boundary.

--------------------------------------------------------------------------------

## Implementation Notes (v1, 2026-11-11)

What was built, and where it differs from the plan above.

*   **The rule is one temperature, not a snow line.** The snow blocks' rule `above_sea >= 950 + (t - 0.5) * 2600` is the same as "the ground's temperature, `t - above_sea / 2600`, is below 0.1346". Frost is therefore one smooth ramp on the ground's temperature, full at 0.1346 and gone `FROST_BAND` (0.12, a temperature, about 310 blocks of height) warmer, in `ochre_base::cold::frost_cold`. It meets the snow exactly, and it covers thawed tundra and taiga, where the ground is colder than snow's but kept bare, at the biome's factor.
*   **The thaw factor is the camera's biome**, as chosen: tundra 0.5, taiga 0.6, in `Biome::frost`. The tundra here reads 0.217 at sea level, so its ground is about 74% frozen at 110 blocks up, and 0.5 of that is what shows.
*   **Constants ride the shared buffer.** `Curvature` is 16 floats (bend, last bend, cold). `shader_values` writes the camera temperature, the biome factor, on or off, the 2600 lapse, the snow temperature and the band, so the shader keeps no copy.
*   **Skipping Snow and Ice** uses two shader defs, `SNOW_CELL` and `ICE_CELL`, as `LEAF_CELL` does, not a table in the buffer. Leaf and flora tiles use the existing `is_leaf` and `is_flora`.
*   **Sky light reaches LOD and edited blocks**, since both bake it, so the cave gate works everywhere. That open question is closed.
*   **Colour is a constant cool white** (`FROST_ALBEDO`), not the snow tile's mean, which read as ice-blue puddles. The patch keeps 55% of the ground's own light and dark, so tufts and brush strokes show through.
*   **Look.** A first version with flat noise patches and a flat white tint read as plastic, with soft edges and glitter that looked like static squares. It was rebuilt. Patches are judged plate by plate (Worley cells of 0.38 blocks, in or out as a whole), so edges are jagged. Plates differ in thickness and let the ground show through, with seams between them, a rim inside the edges, and frost capped at 82%. Plants are rimed from the tip down. The glitter is jittered round points, each cell with a random facet that flashes with the sun's half-vector, so it moves with the camera. A bug in the facet normal (the y component was 0.2, so no facet ever caught the sun) and a 3D spot that a surface almost never crossed kept the first glitter nearly invisible.
*   **Checked.** `cargo test --release --workspace` passes (ochre-base 42, engine 7, gameplay 26, terrain 293, world 150), `cargo clippy --workspace --all-targets` is clean, and `bench` vertex counts are unchanged (1,440,365 chunk, 289,742 LOD). The shader compiles and draws in the sandbox. Looked at in stills on seed 4 tundra only.
*   **Not done.** No frame-time measurement (the fps gate and a glacier route), no stills at a temperate mountain, a tower or a cave, and no check of the LOD boundary. Mist, weather and layer blocks are as listed above.

## Implementation Notes (v1.1, mist, 2026-11-14)

*   **Mist is a second layer over the fog, in `fog.wesl`.** Where the camera's ground is frost-cold (the same sum as frost, but not thinned by the biome's thaw, which only bares ground) an exponential layer of density `MIST_PER_BLOCK` lies over the fog, falling by an e-fold each `MIST_HEIGHT` blocks over the ground under the camera, averaged along the line of sight, so it pools in hollows and thins as the camera climbs. The haze also goes cooler. The fog's own distances are not touched. Scaling the fog's own curve was tried first and did nothing visible at 100 to 500 blocks, since that curve is spread over the whole render distance, and `pow` of its clear share went NaN past the fog's end and drew black dashes.
*   **The ground height rides in the cold buffer.** `cold::shader_values` takes the height of the ground under the camera (the spare float, `Cold.camera.w`), sensed and eased with the temperature in `sense_frost`, with the water's surface as the floor over water.
*   **Not volumetric.** It is a per-fragment blend, not a medium with light scattering. A godrays `FogVolume` (a box of air round the camera) could take the same cold amount as its density so beams show through the mist, which is for that work.
*   **Not done:** frame time, and the values were set by eye in the tundra only.
## Implementation Notes (frost patches, 2026-11-14)

*   **The patches looked like a grid from above, and spread evenly.** Every noise octave was axis-aligned value noise on one lattice, so square cells lined up, and no scale was weighted enough to leave stretches bare. Each octave's coordinates are now turned by its own angle (`turned`) after being pushed about by slow noise (`WARP_SCALE`, `WARP_BY`), the dominant patches are 64 and 12 blocks (were 29 and 7), and a clump noise of 380 blocks (`CLUMP`) moves the cover by `CLUMPING` (2.2) so whole stretches are mostly frost or mostly bare. The average cover is a little lower (the constant in `reach`, -0.15 from -0.07).
*   Set by eye from stills of the tundra on seed 4, and not looked at on a glacier or a mountain. Plates, rim, sheen and glitter are unchanged, and so is frost's cost beyond a few more noise samples a frost fragment.

<!-- Provenance: Indicates initial structural drafting via EGM. -->

*Drafted with EGM*
