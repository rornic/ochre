# Steppe revamp: a biome with its own landscape

*   **Author:** Rory Nickolls
*   **Status:** Draft
*   **Date:** 2026-11-15

--------------------------------------------------------------------------------

## Overview

Ochre's steppe biome reads as recoloured grassland. This design gives it its own landscape, modelled on Mongolian steppe: wide valleys with low, broken escarpments, long ridgelines, scattered rock outcrops, open ground and a big sky.

The work is staged terrain first. A new map-level valley landform, weighted toward steppe and reusable by other biomes, carries the silhouette. A map-level exposure field then drives where grass is thin or dense. Ground wear, wind and sound follow. Every stage blends with the neighbouring biomes. Depth matches the tundra's, which took roughly seven separate mechanisms.

Source brief: `concepts_brief_steppe.md`. Six reviewers (design, codebase details, zoom-out, comprehension, implementation readiness, structure) have already tested the concept. Nothing here has been built, probed or measured yet.

## The Problem

Steppe is a `Plain` biome (`crates/ochre-base/src/biome/mod.rs:165`) with a dry tint shift, almost no trees, and the same grass, tufts and shrub blocks as grassland. The only structural feature it has, the Gullies relief, needs a slope of 0.2 to 0.3 to appear, steppe is flat, and gullies vanish at LOD, so players don't notice them. Biome-level relief is also capped at 18 blocks (`MAX_OFFSET`) and is zero once a sample cell is 16 blocks wide, so enlarging gullies cannot produce valleys or cliffs.

Players see rolling hills, identical tufts and flora, and grassland's sound. Nothing in the biome says "steppe" except its colour.

The proposed fix has three parts.
- A map-level valley landform, cut into `map.height` before the hydrology passes so rivers find the valley floors. A map-level height reaches every LOD without extra work.
- Vegetation, ground, wind and sound changes that read steppe's terrain and exposure, not just its biome name.
- Siting that avoids a circular dependency. Steppe is the output of the climate stage, and moisture depends on height, so a valley sited by steppe could change the steppe it was sited by. Valleys are weighted by a frozen pre-valley snapshot of warmth and moisture and may appear in any biome.

## Goals

*   **Valley landform.** A pure map-level function plus a parameter struct that names no biome, with one consumer (steppe). A second preset (for example a dry-desert wadi) exists only as a unit test, not wired into generation.
*   **Silhouette.** Low, long escarpments, broken along their length by a depth fade, with walls of about 40 to 70 blocks, valley floors wider than about 200 blocks (6+ cells), wall runs under about 10 cells, and long ridgelines. These figures are the reviewers' suggested envelope and are fixed by the first prototype spike.
*   **Cliff and rock walls.** Valley walls are steepened at block level into real cliff faces (gradient 1 or more, up to the existing cliff reach), with `cliff_boost` extended by valley wall cover and rock showing on the faces, and not only on mountain or tableland ground. Decided over a rock-rule-only change, which would leave walls as gentle slopes with rock streaks.
*   **No hydrology regression.** No new lakes in valley bowls, no uncontained water, and valley floors fall monotonically to an outlet. `water_audit --fine` is clean on 3+ seeds, and the seam and closed-mesh tests pass.
*   **Biome stability.** Steppe share, mountain share and the 32-seed census (no biome over 70%) stay within bounds, measured on seeds 1, 42 and 1337 with and without the valley.
*   **Steppe-aware seeds.** Valley coverage scales with a siting weight that favours predicted steppe and keeps a small share elsewhere, and a `LandformKnobs` field sets the world's overall coverage. A steppe-free seed is no worse than before, and a seed whose coverage is zero produces an identical map.
*   **Vegetation and ground structure.** Grass share, density and bare ground vary with a map-level exposure field (height relative to the local surroundings, one scalar): crests thinner with more bare earth, hollows denser, drainage lines greener. Rare trees sit only in sheltered or wetter spots. Outcrops reuse tundra's rock code, generalised to steppe.
*   **Wind and sound.** A seed-derived wind direction and gust bands for foliage on open grass, kept continuous so they do not pop at biome borders. A steppe sound feel from the synthesised wind first, a CC0 clip only if that falls short.
*   **Blending.** Every stage fades into every neighbouring biome by a continuous quantity, with no hard edge at a class boundary.
*   **LOD.** The valley silhouette and coarse tint survive at distance.
*   **Constraints held.** Deterministic per seed, `libm` for world-generation maths, p99 frame time not regressed without a recorded reason, vertex counts measured against the 1.10x chunk gate.
*   **Judged on three review seeds** (steppe-heavy, mixed, steppe-free, from a survey of about 10 seeds). Phases are iterated headlessly until the assistant judges them right, then the user is spawned on those seeds to judge and correct.

## Non-Goals

*   New crafting, animals, buildings or culturally themed decoration.
*   Seasons, new biome classes, or changes to the climate classification or census shares.
*   New block types, without a specific check.
*   LOD overhangs, and outcrops, cliff texture or flora at distance.
*   Cloud wind changes. Cloud wind must be a whole number of patterns per hour for the cloud wrap, so an arbitrary seed direction would pop hourly.
*   Fjord-grade gorge walls.
*   Sourcing recorded audio before the synthesised version has been heard.
*   Reading live weather in the terrain crate. Seed direction enters at material creation.
*   A second outcrop system.

## Open Questions

*   **Freesound rule**: The plan uses synthesised wind only and adds no recorded file. If a clip is wanted later, is CC0 the rule, given `assets/sounds/CREDITS.md` currently states every recording is CC0? Not needed for this plan.
*   **Working tree**: May the stray `flythrough.rs`, `player.rs` and `tundra-walk.flight` edits be committed or stashed before the perf baseline is taken?
*   **Headless self-check**: Which tools check the assistant's own work between user checkpoints without taking over the display? Candidates are the `worldview` and `world_map` PNGs, hillshades, `worldprobe`, `flight` reports and the water and seam audits. Grass, wind and sound can't be judged that way, so the game is launched for the user only at agreed checkpoints.
*   **Provisional numbers**: The valley period, depth range, floor width, siting weight bands, base share outside steppe, wall bias and sharpening fade are starting figures from the reviewers' envelope. The first prototype on seeds 1, 42 and 1337 fixes them.
*   **Water under sharpening**: Block-level sharpening runs after the last hydrology pass, and `docs/water.md` forbids lowering ground beside still water under its level. The plan reuses `calm` and the existing `contain` and `hold_over` held-ground path, as tableland does. Is that enough on valley floors that carry a lake or river, or does the sharpening need its own exclusion near water? Settled by the `water_audit --fine` run on the prototype.

--------------------------------------------------------------------------------

## Critical User Journeys (CUJs)

### CUJ 1: Walking a steppe valley

*   **Given**: A review seed with steppe-heavy country, a spawn on a valley floor, midday, clear sky.
*   **When**: The player walks across the floor and up a wall onto the plateau.
*   **Then**: The floor is wide and open, the walls are broken cliff faces with rock showing, the plateau is grass that thins and shows bare earth on crests and thickens in hollows, there are almost no trees, and the same ground in greyscale still reads differently from grassland.

### CUJ 2: Flying over steppe at distance

*   **Given**: The same seed, a camera at 300 blocks over the ground, moving fast.
*   **When**: Chunks and LODs stream in and out as it passes over valleys and across a steppe-to-grassland border.
*   **Then**: The valley silhouette is visible at distance, with no popping at LOD swaps, no density rings, and no hard edge at the biome border.

### CUJ 3: Generating worlds

*   **Given**: Seeds 1, 42, 1337 and one steppe-free seed.
*   **When**: The world map is generated with and without the valley stage.
*   **Then**: No new lakes sit in valley bowls, water audits and seam tests pass, steppe, mountain and census shares stay in bounds, and a seed with zero valley coverage is unchanged.

## Technical Plan (Architecture)

The change has five layers. Each layer reads only what the layers before it produced, and each fades into neighbouring biomes by a continuous quantity.

```text
worldmap::generate (existing order, new steps marked *)
  hydrology 1 -> erosion -> floor clamp
  -> fjords, cones, inselbergs, cold_uplands, karst, cirques
  -> * valleys: snapshot weight, cut height, store `valley` layer
  -> carve_passes -> landform::run -> terrace
  -> scatter -> hydrology 2 (rivers route into valley floors)
  -> landform::run -> climate::run
  -> * exposure layer (from final ground)
  -> roughness -> label

WorldTerrain (block level, per sample)
  landform_ground = smooth ground
                    + tableland/mountain stepping (existing)
                    + * valley wall sharpening (weight x calm)
  cliff_boost     = seed boost x max(mountain, tableland, * valley wall)
  * valley_wall(x, z), * exposure(x, z): new TerrainSource methods, defaulted

Chunk generation
  block_gradient (+ * valley wall rock bias) -> rock on faces
  flora_at (+ * exposure, humidity) -> tuft shares, bare ground, trees
Shaders / audio
  foliage.wesl (* longer-wavelength gust bands, seed wind direction)
  ambience Surroundings (* steppe fraction) -> wind dials
```

### As built (PR 1)

Prototyping on seeds 1, 42 and 1337 and looking at it in game changed the plan in these ways. The finished behaviour is in `docs/landforms.md` (Valleys) and `docs/terrain-perf.md`, which win over the plan below.

*   **Ground texture.** Grass tops on steppe draw their own rougher layer, `steppe_top`, chosen by the mesher per face (`docs/climate.md`, Steppe), after the owner found the grass layer's leaves too grassy for the steppe's bare ground.
*   **Order.** Valleys run after `landform::terrace`, not after the cirques, because the terraces make lakes that valleys must not cross.
*   **Distance to the line.** The field's value over its slope, so floors stay about 200 blocks wide. The first version used the value times a constant and made kilometre-wide floors that filled into lakes.
*   **Shape.** A broad slope about 200 blocks wide (`soft_edge`) with cliff bands in patches (`cliff`), at a height up the slope that varies, instead of a single 20-block face. Three bend scales, stretch 2, a ragged edge. Cliff bands, not the along-valley depth fade, give the broken look.
*   **Siting.** The weight ramps with height over the sea so a cut never reaches within 6 blocks of the sea, instead of a window. It is also zero near lakes and basins hydrology will fill (priority flood on the post-terrace ground), and within about 14 cells of a big basin (2,000 cells, 6 deep).
*   **Fill.** After the cut, shallow basins (12 blocks or less, or 12 cells or fewer) that the valleys made, or that lie mostly in a valley, are filled to their dam (`hydrology::fill_where`). A deeper one is left a closed pit: draining it by a channel through the dam cut a dry slot as deep across the plain, and filling it plugged the valley. This replaces the `fade_min` guard and the test that no lake is larger than baseline, which could not pass: valley floors do hold ponds.
*   **LOD.** LOD cells take the wall rock bias too, so colours do not change at a swap.
*   **Plains.** Added after seeing seed 3's steppe on a hill: steppe ground is flattened (`valleys::flatten`, 60-cell smoothing, 90% pull, edge softened over 40 cells, off for mountains and low coast) before the valleys are cut, and the steppe-likeness is computed once and shared by the flattening and the siting.
*   **Part 2 as built.** The exposure layer (`exposure.rs`, a byte per cell, `TerrainSource::exposure`, `GENERATOR_VERSION` 101), steppe flora by exposure with dirt on exposed crests, outcrops generalised from the tundra with a steppe share, trees only in hollows, the foliage shader's gust direction from the world's seed with a second slower gust band (`ChunkMaterial::wind`, binding 14), and synthesised steppe wind and grass hiss in `ambience/wind.rs`, whose gust is the shader's own function read at the camera and whose steppe floor depends on the camera's exposure. Steppe shrubs were cut to a tenth or so of grassland's after the first look, and its flowers were later replaced by its own muted one, and steppe has four music tracks picked in turn. Also added after the first look: steppe streams, by a dryness curve that no longer counts steppe as desert (`climate::DRY_BELOW`) and a cap on how wet a stream's banks make dry ground, see `docs/water.md`. Feather grass, a plant of the steppe's own (a new tile between seed grass and shrub, `feather_grass.png`, `ATLAS_CELLS` 38). Not built: the `GrassTint` steppe ramp, which would stop grass faces merging and cost vertices; gravel and crest stone (the owner chose dirt only); a recorded clip.
*   **One PR.** PR 1 and PR 2 are one PR on one branch, in two parts: terrain (this part) then everything else. The user accepted the terrain on seeds 1, 3, 20 and 42 before part 2 began.
*   **Tools.** `generate_with(seed, config, false)`, worldview's hillshade (`world_map`, with its `--no-valleys` and `-relief.png`, went in the engine-readiness refactor), and `worldprobe valley`.

### 1. Valley field, siting and the map-level cut

The valley is a pure function of seed and position, modelled on `landform::tableland`, with no site list. Coordinates are rotated to the world's ridge angle (the one gullies already use), domain-warped, and stretched so the along-axis wavelength is about eight times the across-axis one. A ridged noise on those coordinates gives long, roughly parallel troughs with open ends. Open ends keep the iso-lines from closing into bowls, which is the failure mode of this form.

Two masks come from the field. `m_soft` is a wide smoothstep of it, used for the map-level cut and for every LOD. `m_sharp` is a narrow smoothstep of the same field, used only at block level (section 2). Cut depth is 40 to 70 blocks and floors are flat across at least 200 blocks. To break the escarpment along its length, the depth is multiplied by `min_fraction + (1 - min_fraction) * fade`, where `fade` is a slow noise along the valley's axis, so the valley deepens and shallows in reaches. The minimum fraction (starting at 0.4) is the guard against the failure of this form: because the cut is relative to local ground, a shallow stretch between deep ones raises the floor, and the raised floor can dam water upstream into a lake. Hydrology 2's `drain` and minimum-lake rules clear small basins, and a test fails the build if valley bands add a lake larger than the no-valley baseline. If that test cannot pass at any workable minimum fraction, the fallback is to rag the wall edge instead (see Alternatives Considered).

Siting weight `w` comes from a snapshot taken just before the stage: `climate::regional_warmth` and `climate::moisture_field` on the ground as it stands after the other shapes, the same inputs `cold_uplands` and `climate::run` use. A steppe-likeness `s` is a soft band on temperature (after the height lapse) of 0.26 to 0.58 and on moisture under 0.3. Then `w = base + (1 - base) * s`, so valleys favour steppe and still appear everywhere. `w` is zero on mountain, below a lowland floor, and where the cut would leave the ground within a few blocks of the sea, so the cut never opens a sea inlet. `w` is stored as a new byte layer `valley`, read by bilinear lookup like `trees`, so block level sees the same value as the map. This is a map-file format change and bumps `GENERATOR_VERSION`. It supersedes the brief's line that the snapshot "stores nothing", which held only until block-level sharpening needed the same weight as the map cut.

The cut is `height -= depth * m_soft * w`, taken relative to the local ground and not to a fixed level, so the existing drainage direction survives. Hydrology 2 then drains any small residual basin by its normal rules. A `LandformKnobs` field `valley_share` (fresh salt `0xC4A0_0011`) scales overall coverage by seed. A seed whose share rounds to zero changes nothing.

The function takes a site selector (the weight source) and a profile struct (period, depth, width), and names no biome. Reuse by another biome is a second weight source and profile. A unit test proves it with a second preset that is not wired into generation.

### 2. Block-level wall sharpening, cliffs and rock

The map holds the soft cut, a ramp over a cell or more. At block level `landform_ground` adds the difference between the sharp and the soft cut: `depth * w * (m_soft - m_sharp)`, so the ground steepens into a face of gradient 1 or more while floor and plateau levels match the map. Because it is a pure function of position, the result is the same in every chunk with no neighbour reads. Tableland does the same with its stepped beds (`landform_ground`).

The term is scaled by `calm`, which fades to nothing near water, and by a cell-size fade so LOD cells wider than the map cell (32 blocks) keep only the soft cut. It sits inside `landform_ground` so the existing `held` and `contain` paths keep still water contained.

`cliff_boost` gains a valley wall term: wall cover is `w` times how close the point is to the sharp edge (peaking mid-wall), and the boost becomes the seed boost times the largest of mountain, tableland and wall cover. Cliff density then moves the surface up to the existing reach. A new defaulted `TerrainSource` method `valley_wall(x, z)` carries wall cover to chunk generation, and the `Arc<T>` forwarding impl must be extended or an `Arc` source silently returns the default.

Rock comes from two things. Faces steeper than 3.5 already turn to rock (`face_slope` uses the real gradient on exposed faces). A wall bias, modelled on `OUTCROP_GRADIENT` in `block_gradient`, adds to the gradient by wall cover so mid-steep faces show rock too. Both are keyed on valley cover, not biome, so they fade across biome borders by the same weight.

Risks are the seams and bevels at a 50-block face (`docs/bevels.md`) and a face taller than the 18-block `MAX_CLIFF_REACH` clamp. The profile steepens the ground, and cliff density only roughens it, so the clamp limits roughness and not wall height.

### 3. Exposure layer

After hydrology 2 and before climate, a stage computes `exposure` per map cell: the cell's height minus the mean height over a radius of about 8 cells (256 blocks), scaled by about 25 blocks and clamped to 0..1, so crests are high, hollows low and 0.5 is flat. It is stored as a byte layer and read by bilinear lookup. A defaulted `TerrainSource::exposure(x, z)` returns 0.5 elsewhere, with the `Arc` forwarding added. It is a map-level field, so it is deterministic, free of chunk seams and available to LOD code.

### 4. Vegetation, ground and outcrops

Steppe's flora branch (`flora_at`) gains an `exposure` input. It uses three continuous inputs, never the biome name alone: exposure, existing humidity, and steppe cover. The tundra heath drift pattern (`in_cluster`, `heath_drift`) is the model for broad patches, 150 to 300 blocks across, with a smaller breakup inside. On crests it picks shorter grass, thinner cover and more bare ground. In hollows it picks taller seed grass and denser tufts. Near drainage it greens the tint through humidity. Flora is three discrete blocks, so "height varies" means changing their shares.

Bare earth and gravel are block-level swaps (a few columns to dirt or gravel by exposure and tuft density), so same-block faces still merge. Grass tint patches use a steppe `GrassTint` ramp. Chunk vertex counts are measured against the 1.10x gate. Trees stay rare: steppe's 0.004 density is a cap, and trees appear only in hollows with high humidity or near drainage.

Outcrops reuse `rocks.rs`. Its four tundra-only checks become a per-biome table with a steppe entry in `share()`, and valley wall cover raises the share. Gullies are removed from steppe's `with_relief` list only. The `ReliefKind::Gullies` code and tests stay, since foothill code and docs name it.

### 5. Wind and sound

Wind direction enters at material creation from `TerrainSource::prevailing_wind()`. `foliage.wesl` gains a second, longer-wavelength travelling gust term so gust bands cross open grass. Cloud wind is untouched. `OCHRE_CLEAR_SKY` freezes live `Weather.wind`, so the shader reads the prevailing wind. Whether the change is global or steppe only is an open question, and a steppe-only effect must be continuous.

Sound extends `Surroundings` with a steppe fraction from the existing ring samples, as `desert` does. `update_wind` uses it to lengthen gust cycles, add a grass-rustle layer to the synthesised wind and leave longer quiet stretches. A recorded CC0 clip is added only if the synthesised version falls short, with its `CREDITS.md` row and loop-cut note.

### LOD, blending and determinism

| Layer | Near (full chunks) | Distant (LOD) |
| :--- | :--- | :--- |
| Valley soft cut | yes | yes, in the map height |
| Wall sharpening, cliffs, rock bias | yes | no |
| Exposure-driven flora | yes | no flora at LOD |
| Bare ground and tint patches | yes | coarse base tint only |
| Outcrops | yes | existing LOD rock cells only |
| Wind | near foliage | not applied |

Every layer is keyed on a continuous weight (valley `w`, exposure, humidity, steppe cover), so nothing has a hard edge at a class boundary. Noise comes from the existing deterministic helpers and trig and exponent maths from `libm`, so a seed gives the same world on every machine. The new stage gets a `lap()` timer and a valley and exposure column in `worldprobe`.

## Alternatives Considered

### Alternative A: Enlarge or retune Gullies

*   **Pros**: No new landform, no map-format change, and the relief code and tests already exist.
*   **Cons**: Gullies need a slope of 0.2 to 0.3 to appear (`features.rs`), steppe is flat, and biome relief is zero once a sample cell is 16 blocks wide. They also vanish at LOD.
*   **Reason for Rejection**: They go unnoticed because of gating, not size, so a bigger gully does not fix the look. The user also said gullies "weren't appearing much anyway".

### Alternative B: Biome-level relief only

*   **Pros**: Biome-blended by construction and cheap to generate.
*   **Cons**: `MAX_OFFSET` caps biome relief at 18 blocks, and relief is skipped near water.
*   **Reason for Rejection**: It cannot produce valleys or cliffs, which the user wants. A map-level height also reaches every LOD without extra work.

### Alternative C: Site valleys by a world-wide weight, as existing shapes do

*   **Pros**: Matches `weights()` in `shapes.rs`, so no new siting mechanism.
*   **Cons**: That weight is (1 - wet) * 0.5 + warm * 0.5, which favours hot dry ground.
*   **Reason for Rejection**: It would put valleys in desert and savanna and miss mid-warm steppe, which is warmth 0.26 to 0.58.

### Alternative D: Site valleys by live climate, so every valley lies inside steppe

*   **Pros**: Valleys and biome agree exactly.
*   **Cons**: Steppe is the output of `classify`, and moisture depends on height through the rain sweep, so a valley changes the steppe it was sited by. Windward walls could raise local rain and flip steppe to grassland (reasoned, not measured).
*   **Reason for Rejection**: It forces the feedback to be solved. The user chose a frozen pre-valley snapshot as the fix and accepts that valleys also appear in other biomes.

### Alternative E: Cut valleys by depth alone, with hydrology unchanged

*   **Pros**: The smallest code change.
*   **Cons**: Cuts that make closed bowls fill into lakes, which `docs/terrain-perf.md` records for tableland.
*   **Reason for Rejection**: The valley is cut into `map.height` before `terrace`, `scatter` and hydrology 2, so rivers find the floors, and the field's open-ended troughs avoid bowls.

### Alternative F: Valleys as sited capsule shapes (inselberg style)

*   **Pros**: Count set by a `LandformKnobs` field, the codebase's usual pattern, and a floor sloped to a known outlet by construction.
*   **Cons**: `BIG_APART` is 40 cells, so there are few valleys per world, which read as isolated features. Seeds with little steppe get almost none. Capsule ends can leave seams against eroded terrain.
*   **Reason for Rejection**: The user chose the pure-function field. It gives long parallel valleys and ridgelines everywhere the weight is high, with no site list and one implementation for map, block and LOD.

### Alternative G: Widen the existing drainage

*   **Pros**: Floors drain by construction, valleys branch like real river valleys, and drainage-line greenery has a natural source.
*   **Cons**: Steppe is dry and its rivers are weak, so there may be few or thin valleys. It makes river-centred valleys, not free-standing escarpments, and hydrology 2 may move the rivers.
*   **Reason for Rejection**: The user chose the pure-function field.

### Alternative H: Rock rule only for the walls

*   **Pros**: Touches no meshing or hydrology, so it carries the least risk.
*   **Cons**: A 50-block wall over a 200-block run is gradient 0.25, so walls stay steep grass slopes with rock streaks and the silhouette does not change.
*   **Reason for Rejection**: The user wants cliffs and chose block-level sharpening with `cliff_boost`. This rock rule survives as the wall bias inside that design, and is not the whole answer.

### Alternative I: Terraced benches for the walls

*   **Pros**: Reuses `terraced` and its tests, and steps and faces come with it.
*   **Cons**: It depends on the rock beds under the ground, so thin or absent beds give no walls, and steppe walls would resemble tableland.
*   **Reason for Rejection**: The user chose sharpening. Benches are a possible follow-up if the sharpened walls look wrong.

### Alternative J: Fjord-grade gorge walls

*   **Pros**: The most dramatic walls.
*   **Cons**: The user judged them too deep. They risk the mountain classifier (150+ blocks over the sea) and need the hardest water and seam work.
*   **Reason for Rejection**: The user chose a low, long, broken escarpment with walls of about 40 to 70 blocks.

### Alternative K: Exposure from chunk-local curvature, or a wider height sampler

*   **Pros**: Needs no new map layer or format change.
*   **Cons**: Chunk-local curvature seams at 16 blocks and misses crests and hollows 50 to 300 blocks across. A wider height sampler in the terrain crate costs a hot path.
*   **Reason for Rejection**: A map-level exposure layer is deterministic, free of seams and available to LOD code, and format bumps are not a concern at this stage.

### Alternative L: Patch noise only for vegetation, with no exposure field

*   **Pros**: Cheapest, and needs no terrain input.
*   **Cons**: Patches have no relation to crests, hollows or drainage, so they read as random blotches, which the brief's criteria reject.
*   **Reason for Rejection**: The user accepted the map-level exposure layer.

### Alternative M: A fragment-shader skin for bare ground, as `lod_patch.wesl` and `frost.wesl` do

*   **Pros**: No vertex cost and no effect on greedy merging.
*   **Cons**: It is a shader change for every material, and the ground stays grass in the world, with no dirt or gravel block.
*   **Reason for Rejection**: Steppe has low vertex counts, so block swaps cost little and give real dirt and gravel columns. Revisit if the 1.10x gate fails.

### Alternative N: Reuse Kopjes for steppe outcrops, or build a new outcrop system

*   **Pros**: Kopjes reuse is free, since it already sets `TerrainSample.rocky`.
*   **Cons**: Kopjes make steppe resemble savanna. A new system is a second outcrop system to maintain.
*   **Reason for Rejection**: Generalising the existing tundra rock code in `rocks.rs` to a per-biome table gives steppe its own outcrops with no new system.

### Alternative O: Delete the Gullies relief entirely

*   **Pros**: No dead code and no stale docs.
*   **Cons**: Removal touches `features.rs`, the biome list, both crate READMEs, `landforms.md`, the glossary, `terrain-perf.md` and probably `worldprobe`.
*   **Reason for Rejection**: Dropping it from steppe's `with_relief` list only gives the same player-visible result at far less cost.

### Alternative P: Seed wind direction applied to cloud wind

*   **Pros**: One wind direction for sky and ground.
*   **Cons**: Cloud wind must be a whole number of patterns per hour for a seamless wrap, so an arbitrary direction pops hourly.
*   **Reason for Rejection**: Clouds stay untouched, and only foliage reads the seed direction.

### Alternative R: Break the escarpment by ragging its edge, or by side drainage notches

*   **Pros**: Ragging the edge adds no new basins, since floor depth is constant. Side notches read as real drainage.
*   **Cons**: At map level the 64-block soft edge smooths ragging finer than a few cells, so the breaks show only near the player. Side notches need a second field and mask, are steep channels hydrology 2 may reroute, and can seam against the sharpened face.
*   **Reason for Rejection**: The user chose fading the depth along the valley, which breaks the wall at every distance. Ragging the edge is the fallback if the lake test cannot pass at a workable minimum depth fraction.

### Alternative Q: Freesound clips first

*   **Pros**: Recorded grass rustle is more natural than synthesis.
*   **Cons**: It needs sourcing, a `CREDITS.md` row and a loop cut. `CREDITS.md` currently says every recording is CC0.
*   **Reason for Rejection**: Deferred, not ruled out. The synthesised wind comes first, and a clip is added only if it falls short.

--------------------------------------------------------------------------------

## Non-Functional Requirements

*   **Performance.** The valley stage adds one `regional_warmth` and one `moisture_field` call plus a per-cell field evaluation to world generation, against the 0.3 s new-stage gate. Block-level sharpening adds a few noise evaluations per sampled column. p99 frame time must not regress without a recorded reason, vertex counts are measured against the 1.10x chunk gate, and every PR logs a dated section in `docs/terrain-perf.md`.
*   **Rollout and rollback.** Each PR bumps `GENERATOR_VERSION`, so old maps are refused and regenerate. Rollback is a revert plus a fresh version number. Branches are stacked locally and nothing is pushed until the owner says so.
*   **Determinism.** Output is identical per seed on every machine: `libm` for exponent and trig maths, the existing noise helpers, and no thread-order or hash-order dependence.
*   **Licensing.** No new recorded asset is added. Any later clip must be CC0 or the `CREDITS.md` line reworded.

### Additional Resources

*   `concepts_brief_steppe.md` and the six reviewer reports behind it.
*   `docs/water.md`, `docs/bevels.md`, `docs/landforms.md`, `docs/climate.md`, `docs/world-plan.md`, `docs/terrain-perf.md`.

### Future Improvements

*   **Terraced benches** on valley walls (reusing `terraced`), if sharpened walls look wrong.
*   **Recorded CC0 grass-rustle clip** if the synthesised wind falls short.
*   **A second valley preset** (for example a dry-desert wadi) wired into generation, once the steppe valley is accepted.
*   **Dirt and gravel ground wear** (gravel at wall feet and dry drainage) and stone on crests.
*   **A steppe-only wind effect** by camera-local uniform or per-vertex weight.

--------------------------------------------------------------------------------

## Detailed Implementation Plan

One PR in two parts, on one local branch. Part 1 is terrain and is done. Part 2 is exposure, vegetation, ground, outcrops, wind and sound, started after the owner accepted part 1's terrain. Each entry is tagged. Numbers are starting values, named as constants, and fixed by the prototype on seeds 1, 42 and 1337 (see Open Questions).

### Starting parameters

| Parameter | Starting value | Where |
| :--- | :--- | :--- |
| Valley period across the axis | 2400 blocks | `valleys.rs` |
| Along-axis stretch | 2 times the across period | `valleys.rs` |
| Cut depth | 40 to 70 blocks | `valleys.rs` |
| Along-valley depth fade | depth times (0.4 + 0.6 * fade noise), fade wavelength about 3500 blocks along the axis | `valleys.rs` |
| Floor width | at least 200 blocks flat | `valleys.rs` |
| Soft edge width (map cut) | about 200 blocks | `valleys.rs` |
| Sharp edge width (block level) | 14 to 64 blocks, by stretch | `valleys.rs` |
| Height weight | rises linearly from 6 blocks to 76 blocks over the sea, fades out from 220 to 260 | `valleys.rs` |
| Sea margin kept by the cut | 6 blocks over the sea (the height weight keeps it) | `valleys.rs` |
| Base share outside steppe | 0.25 | `valleys.rs` |
| Steppe band | warmth 0.26 to 0.58 after lapse, moisture under 0.3, soft edges 0.04 and 0.05 | `valleys.rs` |
| Sharpening fade | full under 16-block cells, none from 32 | `source.rs` |
| Wall rock bias | adds 1.0 to gradient at full wall cover | `terrain.rs` |
| Exposure radius and scale | 8 cells (256 blocks), 25 blocks | `exposure.rs` |
| Bare dirt cap | at most 12% of steppe ground | `terrain.rs` |

### Target Files to Create/Modify

#### 1. `crates/ochre-world/src/worldmap/valleys.rs` (Create, PR 1)

*   **Type / Scope**: Rust module, map-level landform.
*   **Proposed Changes**: A `ValleyProfile` struct (period, stretch, depth range, floor width, soft and sharp edge widths, lowland window, sea margin, base share) with a steppe preset. Pure functions of `(seed, profile, x, z)`: the rotated, warped, stretched ridged field, `soft_mask`, `sharp_mask`, and `depth`. `siting(map, share) -> Vec<f32>` takes the snapshot with `climate::regional_warmth` and `climate::moisture_field`, applies the height lapse, forms the steppe-likeness band, then `w = share * (base + (1 - base) * s)`, zero on mountain, outside the lowland window, and where the cut would leave under the sea margin. `run(map, share)` writes the byte layer `map.valley` from `w`, then lowers `map.height` by `depth * soft_mask * w`, relative to local ground. The function takes a weight source and a profile and names no biome. Tests: determinism and `libm`-only maths, no isolated local minima inside a valley band on seeds 1, 42 and 1337, valley bands add no lake larger than the no-valley baseline at the starting minimum depth fraction, share 0 leaves the map unchanged, the cut never leaves ground under the sea margin, siting weight bands, and a second preset (a dry-desert wadi) producing a valid, unwired valley.

#### 2. `crates/ochre-world/src/worldmap/mod.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust module, generation pipeline and map struct.
*   **Proposed Changes**: PR 1: declare `mod valleys`; add `pub valley: Vec<u8>` to `WorldMap` and its `empty` constructor; call `valleys::run(&mut map, knobs.valley_share)` after `shapes::cirques` and before `gameplay::carve_passes`, with `lap("valleys")`; bump `GENERATOR_VERSION` to 100. PR 2: declare `mod exposure`; add `pub exposure: Vec<u8>`; call `exposure::run(&mut map)` after `climate::run` and before `detail::roughness_layer`, with `lap("exposure")`; bump `GENERATOR_VERSION` to 101.

#### 3. `crates/ochre-world/src/worldmap/io.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust module, map file read and write.
*   **Proposed Changes**: Write and read `valley` (PR 1) and `exposure` (PR 2) with `put_bytes` and `r.bytes(cells)`, in the same order in both directions. The existing round-trip and wrong-version tests cover them, and a test asserts the layers survive a save and load.

#### 4. `crates/ochre-world/src/worldmap/character.rs` (Modify, PR 1)

*   **Type / Scope**: Rust module, seed-derived knobs.
*   **Proposed Changes**: Add `SALT_VALLEYS = 0xC4A0_0011` (the next free salt after cirques), a `VALLEY_SHARE` range constant, and `valley_share` on `LandformKnobs`, drawn in `from_seed`. Extend `test_landform_knobs_fill_their_range` to cover it.

#### 5. `crates/ochre-world/src/worldmap/source.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust module, `WorldTerrain` and its `TerrainSource` impl.
*   **Proposed Changes**: PR 1: in `landform_ground`, add `depth * w * (soft_mask - sharp_mask)`, scaled by `calm` and by the cell-size fade, so it sits inside the held-ground path (`held`, `contain`, `hold_over`). `w` comes from bilinear lookup of `map.valley` (like `trees`). `cliff_boost` becomes the seed boost times the largest of mountain, tableland and valley wall cover. Implement `valley_wall(x, z)`, wall cover peaking mid-wall. Update the one-line comment on `landform_ground` that says it never reads climate, since it now reads the stored `valley` layer, which was derived from a climate snapshot. Tests: `landform_ground` still ignores the live climate and biome layers, wall cover is 0 away from valleys, the sharpened face reaches gradient 1 or more on a prototype seed, and ground returns to the map height at floor and plateau. PR 2: implement `exposure(x, z)` by bilinear lookup of `map.exposure`, with a test.

#### 6. `crates/ochre-world/src/worldmap/exposure.rs` (Create, PR 2)

*   **Type / Scope**: Rust module, map-level stage.
*   **Proposed Changes**: `run(map)` computes per cell the height minus the mean over an 8-cell radius (a box blur through the existing window helpers), divided by 25 blocks and mapped to 0..1 around 0.5, written to `map.exposure` as a byte. Tests: flat ground gives 0.5, a synthetic ridge reads high on the crest and low in the hollow, and the result is deterministic.

#### 7. `crates/ochre-base/src/terrain_source.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust trait, the `TerrainSource` interface.
*   **Proposed Changes**: Add defaulted `valley_wall(&self, x, z) -> f32` (0.0, PR 1) and `exposure(&self, x, z) -> f32` (0.5, PR 2), and forward both in the `Arc<T>` impl, since the README warns an `Arc` source silently uses the default otherwise. A test confirms the `Arc` impl forwards them.

#### 8. `crates/ochre-terrain/src/generate/columns.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust module, per-column data for chunks and LOD grids.
*   **Proposed Changes**: Add `walls` (PR 1) and `exposures` (PR 2) vectors to `ChunkColumns` and the LOD `ColumnGrid`, filled from `noise.valley_wall` and `noise.exposure` where `boosts` is filled, with accessors `wall(cx, cz)` and `exposure(cx, cz)`.

#### 9. `crates/ochre-terrain/src/generate/generator/terrain.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust module, block choice.
*   **Proposed Changes**: PR 1: `block_gradient` takes a `wall` argument and adds the wall bias to the gradient, like `OUTCROP_GRADIENT`, so mid-steep faces show rock. Faces over 3.5 already turn to rock through `face_slope`. PR 2: `surface_block` swaps top grass to dirt on steppe where a broad patch noise times exposure passes a threshold, capped at 12% of steppe ground, keyed on continuous values and not the biome name, with its test.

#### 10. `crates/ochre-terrain/src/generate/generator/chunk.rs`, `lod.rs`, and `crates/ochre-terrain/src/generate/trees.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust modules, callers.
*   **Proposed Changes**: PR 1: pass `columns.wall` (or the grid's) at the five `block_gradient` call sites (`chunk.rs` lines near 120 and 264, `lod.rs` near 177 and 330, `trees.rs` near 875). PR 2: `chunk.rs` passes `columns.exposure` to `flora_at` and skips flora on swapped dirt columns. `trees.rs` gates steppe trees to low-exposure ground (hollows), with its biome-list tests extended.

#### 11. `crates/ochre-terrain/src/generate/generator/flora.rs` (Modify, PR 2)

*   **Type / Scope**: Rust module, ground flora.
*   **Proposed Changes**: `flora_at` gains an `exposure` argument. Steppe's branch shifts the shares by exposure, short grass and thin cover on crests, taller seed grass and denser tufts in hollows, and draws broad 150 to 300 block patches with finer breakup inside, modelled on `in_cluster` and `heath_drift`. Other biomes ignore the argument, and a test pins that grassland output is unchanged. New tests: crests and hollows differ, bare share stays under its cap.

#### 12. `crates/ochre-terrain/src/generate/generator/buffers.rs` (Modify, PR 2)

*   **Type / Scope**: Rust module, `GrassTint`.
*   **Proposed Changes**: A steppe ramp toning ground and tufts to straw, dusty olive and warm earth by exposure and humidity at the same four chunk corners it already samples. Cells wider than a block (LOD) keep the plain tint. A test checks the tint stays inside the palette and a bench run records the vertex change against the 1.10x gate.

#### 13. `crates/ochre-terrain/src/generate/rocks.rs` (Modify, PR 2)

*   **Type / Scope**: Rust module, rocks and outcrops.
*   **Proposed Changes**: Replace the four `== Some("tundra")` checks (near lines 177, 222, 330, 352) with a per-biome table, add a steppe entry in `share()`, and raise the share with valley wall cover. Existing tundra tests stay unchanged and a steppe test checks outcrops appear on steppe and never on a mountain, within 3 blocks of water or on steep ground.

#### 14. `crates/ochre-base/src/biome/mod.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust module, biome table.
*   **Proposed Changes**: PR 1: remove `.with_relief(&[ReliefKind::Gullies])` from steppe. The `Gullies` kind, its code and its tests stay, since foothill code names it. PR 2: retune steppe's `shifted` tint if the new `GrassTint` ramp needs a different base.

#### 15. `crates/ochre-world/src/worldmap/climate.rs` (Modify, PR 2)

*   **Type / Scope**: Rust module, tree density.
*   **Proposed Changes**: Keep steppe's `tree_density` at its present low value and add a test that no new tree lands on a crest. No change to `classify`, so census shares are unchanged by this file.

#### 16. `assets/shaders/foliage.wesl`, `assets/shaders/world.wesl`, `assets/shaders/world_prepass.wesl` (Modify, PR 2)

*   **Type / Scope**: WESL shaders, main and shadow passes.
*   **Proposed Changes**: `swayed` takes a wind vector. The fixed gust direction `(0.11, 0.07)` is replaced by the wind, and a second gust term with a longer wavelength travels along it, so bands cross open grass. Applies to all foliage in every biome. All three call sites (the main pass and the two in the prepass) pass the same wind, so leaves keep to their shadows.

#### 17. `crates/ochre-terrain/src/material.rs`, `crates/ochre-terrain/src/lib.rs`, `examples/sandbox.rs` (Modify, PR 2)

*   **Type / Scope**: Rust, material bindings and setup.
*   **Proposed Changes**: Add a `wind` field to `ShaderInputs` (default zero) and a `#[uniform(N)] wind: Vec4` to `ChunkMaterial`, where N is the next slot not used by `world.wesl`, checked when implementing (0 to 6, 8 and 11 to 13 are taken). `insert_terrain_resources` fills it from `TerrainSource::prevailing_wind()`. `sandbox.rs` is the only example that builds `ShaderInputs` by field and gets the new field. Headless runs keep the default. Cloud wind and `Weather.wind` are untouched, and `OCHRE_CLEAR_SKY` does not affect it.

#### 18. `crates/ochre-world/src/ambience/mod.rs` and `crates/ochre-world/src/ambience/wind.rs` (Modify, PR 2)

*   **Type / Scope**: Rust modules, ambience.
*   **Proposed Changes**: `Surroundings` gains a `steppe` fraction, sensed from the existing ring samples the way `desert` is. `update_wind` uses it to lengthen gust cycles, lower the base level and leave longer quiet stretches, and `WindDecoder` adds a grass-rustle layer (filtered noise) scaled by it. No new recorded file and no new `Loop`. Tests: the fraction is 0 off steppe and 1 on it, wind never drops under `GROUND_BREEZE`, and the existing wind tests still pass.

#### 19. `examples/worldprobe.rs` and `examples/world_map.rs` and `examples/README.md` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust examples, tools.
*   **Proposed Changes**: `worldprobe` gains a `valley` subcommand printing `w`, both masks and wall cover at a point and over a window, and PR 2 adds exposure. `world_map` gets a valley layer (the `layers` array grows) and PR 2 an exposure layer, for hillshade review. `examples/README.md` lists both.

#### 20. Tests that pin output: `crates/ochre-world/src/worldmap/portable_maths.rs` and `crates/ochre-terrain/src/generate/generator/determinism.rs` and `crates/ochre-terrain/src/generate/generator/tests.rs` (Modify, PR 1 and PR 2)

*   **Type / Scope**: Rust tests.
*   **Proposed Changes**: Re-pin `MAP_HASH` and the two chunk hashes in each PR, after the first deliberate run proves the change is the intended one. `valleys.rs` and `exposure.rs` sit in the directory the `libm` scan reads, so they must pass it. `tests.rs` gains a closed-mesh and seam test over a sharpened valley wall on the steppe-heavy review seed, modelled on `test_bevelled_surfaces_stay_closed` and `test_chunk_seams_leave_no_hole`, plus a test that a valley floor carrying a river or lake leaves no wet column beside a lower dry one.

#### 21. Documentation (Create/Modify, PR 1 and PR 2)

*   **Type / Scope**: Markdown.
*   **Proposed Changes**: `docs/designs/steppe.md` (Create, the finished design). `docs/landforms.md`: a Valleys entry, the changed steppe row, and the opening line that says shapes never read climate, reworded because valleys read a pre-valley warmth and moisture snapshot, as `cold_uplands` already reads warmth (PR 1). `docs/terrain-glossary.md`: Valley, Ridgeline, Outcrop and Exposure entries, and the Gully row corrected (PR 1 and PR 2). `docs/climate.md`: a Steppe subsection with its own heading, not an extension of the flora paragraph (PR 2). `docs/world-plan.md`: the built list, the stale gully text and the gates (both). `docs/terrain-perf.md`: a dated "(GENERATOR_VERSION n)" section per PR with measurements, and the stale gully lines near 190 and 734 fixed. `docs/water.md`: the stage order note for valleys and sharpening (PR 1). `docs/tectonic-mvp.md`: stale gully text if present. `crates/ochre-base/src/biome/README.md`, `crates/ochre-base/src/README.md`, `crates/ochre-world/src/worldmap/README.md` and `crates/ochre-world/src/ambience/README.md`: the changed steppe relief list, the new trait methods, layers and stage, and the `steppe` fraction.

### Verification Plan

*   **Compilation/Build**: `cargo build --release --workspace`.
*   **Unit Tests**: `cargo test --release --workspace` (expect slow runs in `generator/tests.rs` and `lod/tests.rs`).
*   **Static Analysis & Linting**: `cargo clippy --workspace --all-targets`.
*   **Water and seams**: `cargo run --release --example water_audit -- --fine` on seeds 1, 42, 1337 and the three review seeds, with zero uncontained cells and no lake or coast walls or steps. `cargo run --release --example seams` clean.
*   **Biome stability**: the 64-seed census (`scripts/census.sh` against `worldview`), no biome over 70% of land, and steppe and mountain share on seeds 1, 42 and 1337 compared with and without the valley.
*   **Performance and vertices**: `cargo run --release --example bench` before and after. PR 1 changes the world, so vertex counts are recorded as new values, not matched, and the 1.10x chunk gate is checked on steppe. `OCHRE_STAGE_TIMES` confirms the valley stage fits the 0.3 s gate. `flight` on a steppe seed gives p50, p99 and worst frame times against a baseline taken from a clean working tree.
*   **Survey and review seeds**: survey about 10 seeds with `world_map` and name three (steppe-heavy, mixed, steppe-free) before the prototype.
*   **Look**: headless first (worldview and `world_map` PNGs, hillshades, `worldprobe`) until the assistant judges each PR right. Then the owner is spawned on the three review seeds at midday and corrects. Grass, wind and sound can only be judged with the game running.
*   **Test Coverage Requirement**: every new function has a test as listed above. No coverage percentage is enforced in this repo.

--------------------------------------------------------------------------------

<!-- Provenance: Indicates initial structural drafting via EGM. -->

*Drafted with EGM*
