# Geography colour and the feel of each country

*   **Author:** Rory Nickolls
*   **Status:** Implemented on the local branch `feat/geography-colour`, not pushed
*   **Date:** 2026-11-18

--------------------------------------------------------------------------------

## Overview

Distant ground looked the same everywhere, and every biome but the tundra felt alike. This design makes ground colour follow the lie of the land, so a player on a ridge can tell where water and sheltered, fertile ground lie, and gives each kind of country its own sun, grade, haze, shadow colour and storms, so a hot savanna, a damp jungle and a cold tundra feel different before a block is read.

It was built in two parts that share a way of working: smooth fields of the climate and the map are turned into colour and light on the CPU, and the shaders are handed a few numbers, not a new map to sample.

1.  **Geography colour.** Grass tint shifts with banks, hollows, crests and height. Steep grass breaks to dirt. Hollows lose some light. Trees read the same attributes.
2.  **The feel of each country.** One climate-driven `Atmosphere` for the camera's grade, the sun, the shadows and the haze, and a table that gives distant country its own haze. The steppe also gets huge dry storms.

## The Problem

**Distant ground had no geographic signal.** Grass colour was a per-mesh tint from climate sampled at four corners and blended bilinearly (`GrassTint::new`). At 4 km the world is drawn in level 3 and 4 LOD nodes, cells 8 to 16 blocks wide, and a level 4 node is about 256 blocks across with four samples, so a valley, a crest or a 30-block river corridor inside it could not show. Far meshes also dropped the cold's patches, coarse cells used brightness-only noise in place of `drift`, and past 1 km normals flattened toward up. Colour was the only distance cue left, and it carried almost no geography.

**Inputs existed and did not reach the colour.** Climate already cooled with height and read `map.moisture`. The map held `exposure` (height over the ground round it) and river data. None of it reached the tint, and steep grass survived where block placement left it.

**River wetting reached too far.** `map.moisture` wetted the banks of big rivers and lakes 14 cells (448 blocks) out, too wide to read as a corridor.

**The biomes felt alike.** The shaders know no biome. The tundra's cold feel was a camera grade, eased over two seconds, plus frost, mist, ground tint and snow. Nothing else had an equivalent, and a distant biome showed nothing of its own.

## Goals

*   From a ridge about 4 km off, valleys, crests and river corridors read as distinct colour regions without debug overlays. **Met on seed 1's steppe**, judged by the owner.
*   Ground-level terrain stays quiet and painterly, with `drift` on top. Rock stays governed by geology, and each biome keeps its palette.
*   Grass greened by water never reaches the palette's lush end, which belongs to the jungle.
*   Each kind of country (hot, jungle, forest, taiga, steppe, tundra, mountain, volcanic) has a feel that blends smoothly across borders, and distant country shows in the haze.
*   The tundra keeps its cold, foggy look. **Held**: a fixed tundra frame differs from the clean `main` frame by under 3/255 on average, haze apart.
*   The steppe gets huge, dry, thundering storms, about two a game week, with rain left rare.
*   No colour pop where full-detail chunks hand over to LOD nodes. **Held**: gaps of about 0.01 of a channel (`test_grass_tint_barely_changes_where_a_chunk_hands_over_to_a_node`).
*   Frame time does not move. The owner found performance fine through the early changes. Not measured for the later ones.

## Non-Goals

*   A hydrology or ecology simulation, or a generation rewrite beyond shortening river wetting and tree density.
*   Aspect, seasons, and heat shimmer, dust or sky-colour changes.
*   A biome id in a filtered texture. Everything follows smooth fields.
*   Redefining the map's `exposure`, which the steppe's trees, bare ground and rocks use.
*   Fauna, sweat and audio beyond the steppe's track, which are for later.

## Technical Plan (Architecture)

### Attributes

| Attribute | Source | How it reaches the look |
|---|---|---|
| Elevation | height over `WATER_LEVEL` | thins vegetation going up, in humidity |
| Exposure | `map.exposure`, unchanged | crests drier, hollows richer, and hollows darker |
| Moisture | `map.moisture`, river wetting cut from 14 to 8 cells | humidity, so wetter ground is greener |
| Water closeness | new byte layer, 3 cells (about 100 blocks), squared, from any river, lake or pond | banks wetter |
| Slope | the fragment shader's smooth normal | steep grass breaks to dirt |

`TerrainSource::vigour` is the humidity shift those give (banks +0.25 at most, crests and hollows plus or minus 0.2, height up to 0.3 thinner). `tint_climate` adds it to the climate's humidity, capped at `LUSH_CAP` (0.6) so water never reaches the jungle's green. `tree_lie` turns it into a multiplier on a tree cell's chance, 0.2 to 2.2 times, and `tree_bound` takes the larger of it and 1 so LOD nodes never skip a cell chunks would grow.

### Geography colour

*   `GrassTint` holds a grid of samples over a mesh, one every 16 blocks and 17 a side at most for a coarse node, where the chunks beside it sample, where a chunk keeps its four corners. The outer cell extrapolates as the corners did, so chunk edges are unchanged. Leaves and flora take the same tint.
*   Each sample also gives a shade, up to 30% less light in the deepest hollows by `exposure`, multiplied into each sculpted vertex's occlusion (`nets.rs`), so chunks and nodes agree.
*   `world.wesl` blends steep ground under grass to the dirt texture along a ragged edge, only for the grass layers (`GRASS_LAYER`, `STEPPE_LAYER`), so rock, scree and sand are untouched.
*   The haze over land gets 1.6 times its colour, easing back to the sky's by the fog's end, since the horizon's pale grey read flat.

### The feel of each country

`atmosphere.rs` holds `Feel`, seven weights (cold, hot, jungle, forest, steppe, mountain, volcanic), each 0 to 1. Climate sets the first five smoothly (`Feel::of`), `TerrainSource::mountain_cover` the mountain, and a biome's volcanic modifier the last. A row of knobs per kind gives what it does at full weight: the sun's strength and colour, the grade's white balance, saturation and contrast, the haze's tint and density, the shadow colour and strength, and how deep a storm dims the light. `Atmosphere::of` sums the rows, exactly neutral at no feel.

*   **Hot:** a hard, bright, warm sun, a harder grade, a thick warm haze. **Jungle:** the most colour, a filtered sun, a thick green haze (15 times the density, since the fog's end is tens of kilometres off, and 6 times left the trees clear at a kilometre). **Forest:** a dimmer, warmer sun and a soft close haze. **Taiga** is the forest's weight under the cold. **Steppe:** drained colour, a hard clear light, thin haze, deeper storms. **Mountain:** crisp thin air and far views. **Volcanic:** a dim red sun through grey-brown ash.
*   **Cold** keeps its own grade (`cold_color_grading`, unchanged) and its row adds a bluer, denser haze, so cold country far off shows.
*   The camera's feel is sensed four times a second and eased over 2 s (`EasedFeel`). The grade (`view.rs`), the sun and its colour (`darken_sky`), and the shadow colour follow it.
*   **Distant haze:** `ambience/haze.rs` senses the feel on 16 compass points at 1, 3 and 8 km and packs each point's haze tint and density into the shared curvature buffer. `fogged` multiplies the haze by them. The shadow colour rides in the same buffer. No binding or material changed.

### The steppe's storms

Over steppe (`steppe` in `storms.rs`, mid-warm and dry) the air-mass noise has a second, far lower bar (`FURY_BAR`). Where it clears, the whole 100 km air mass is in a spell of fury: `finish` darkens, clouds over and thunders in swells (`fury_swell`, 30 km lumps, an hour and a half long) and leaves the rain to the wet spells, so the storms are dry and the steppe's rain stays at about 0.2% of the time. Thunder comes in the swell's upper half, and each thundering lump is a core for lightning seen from afar (`fury_cores`). Nearby lightning strikes up to four times as fast at full thunder as at a storm's edge, for every storm. `Weather::gloom` drives the wind's roar and the birds hushing. Dark cloud over 0.3 sends birds and butterflies away in every biome. The steppe's dry storms play `steppe-fury.ogg` (`Mood::SteppeFury`).

### Generation

River wetting is cut from 14 to 8 cells (`MOISTURE_REACH`), swamps and valleys keeping the old reach. Below 7 the biome tidy-up leaves slivers (63 specks of biome at 6). The new `water_closeness` layer and the tree change bump `GENERATOR_VERSION` to 104, so cached maps regenerate and stored edits from 102 or 103 are refused.

## Alternatives Considered

*   **A world-space texture for ground colour.** Would carry more channels and remove corner seams, at the cost of a binding through three material paths and a hand-built mip chain. Rejected for the denser tint grid, which fixed the far view alone.
*   **Distance to water per vertex, or both a map and per vertex.** Per vertex costs meshing time and blurs at far LOD. A baked layer was chosen, quantised to 32-block cells.
*   **A biome id as an attribute, or atmosphere by a per-biome table blended by the existing margins.** The id warps and blends patchily, and the margins are about two cells wide. Atmosphere follows smooth climate instead.
*   **Redefining `exposure`, or adding `river_width` to moisture.** The first changes the steppe, and the second counts wide-water wetting twice.
*   **Fixing steep grass in generation.** It changes blocks. The shader blend does it without.
*   **Moving the tundra's grade onto the new mechanism.** It already worked and the owner loved it. It stays its own, and the new mechanism adds only its distant haze.
*   **Discrete fury storms born where their paths cross the steppe.** Only about 6% of candidates crossed a small steppe patch, so the rate hung on its size. A spell of the air mass does not.
*   **Turning the hot effects up in one step.** A first, much stronger desert turned the world amber and the sky brown, since the sun's tint feeds the sky. They were backed off.

## Not done

*   **Volcanic** is covered by unit tests only. No volcano was found to look at.
*   Shadow tint and the new feel rows for the forest, steppe, mountain and volcanic country are the author's picks, not yet judged in play.
*   **Far country's shadows** are the camera's own. The sky and cloud edge against tinted haze is not tinted, so a seam can show.
*   **The marble fix** (only grass bares to dirt) was not confirmed by a close-up of a marble side.
*   **Frame times** were not measured for the feel, shadow, haze and storm changes.
*   Two known problems were logged, not fixed: a hole in the mesh at a marble arch on seed 4, and a thin strip of grass or dirt inset from a rock's edge (`docs/issues.md`).
*   Sweat, heat audio and the other senses of a hot country.

## Verification

*   `cargo test --release --workspace` and `cargo clippy --workspace --all-targets` pass.
*   Frames from the game's own flythrough (`OCHRE_FLIGHT_FRAMES`) on the clean `main` build and this one, seed 1's steppe, a lakeside grassland, the tundra and each biome, as contact sheets.
*   `bench`: vertex counts moved with the map (537,445 chunk, 68,250 LOD), chunk meshing 441 to 552 ms and LOD meshing 97 to 122 ms over the whole branch. With the old river reach restored, chunk meshing is 443 ms with the vertex count exact, so the tint grid costs chunks nothing.
*   `weather_map` per biome: the steppe is stormy 14.6%, 6.2% and 11.4% of the time on seeds 1 to 3, against 5.8%, 1.0% and 2.9% before, with rain at 0.2%.
*   Details and numbers are in `docs/climate.md` (Geography colour and heat), `docs/designs/weather.md` and `docs/terrain-perf.md`.

## Implementation notes

*   **Terrain and map:** `worldmap/climate.rs` (reach), `water_closeness.rs`, `mod.rs`, `io.rs`, `source.rs` (`vigour`, `tint_climate`, `tree_lie`, `mountain_cover`), `portable_maths.rs`, and the `TerrainSource` methods in `ochre-base/src/terrain_source.rs`.
*   **Colour:** `generate/generator/buffers.rs` (`GrassTint` grid, `shade_at`), `nets.rs`, `trees.rs`, `material.rs` (shader defs), `world.wesl`, `fog.wesl`, `painterly.wesl`, `curvature.wesl`, `curvature.rs`.
*   **Atmosphere:** `atmosphere.rs`, `ambience/mod.rs` and `haze.rs`, `view.rs`, `weather.rs`.
*   **Storms and sound:** `storms.rs`, `weather.rs`, `ambience/wind.rs`, `music.rs`.
*   **Tools and docs:** `examples/weather_map.rs` (a stormy column, and `--storm x,z` to find a storm to stand under), `examples/worldprobe.rs` (the climate at a point), `docs/climate.md`, `docs/terrain-perf.md`, `docs/designs/weather.md`, `docs/issues.md`, `AGENTS.md`.
