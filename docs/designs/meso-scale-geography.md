# Meso-scale geography: unique places from coupled systems

*   **Author:** Rory Nickolls
*   **Status:** Draft
*   **Date:** 2026-11-18

--------------------------------------------------------------------------------

**Status, 2026-11-18:** slice 1 is built (scorer, baseline, viewer layers, spike). What it found and the decision for slice 2 are in `docs/meso-slice1-decision.md`, which supersedes this document where they differ.

## Overview

Ochre's macro scale (colour, valleys, weather) and micro scale (textures) work. The meso scale, about 1 to 10 km, has little to steer by. A player on a ridge sees terrain that is rough and similar in every direction, and walking through it finds nothing worth stopping for. This design adds a meso-scale layer to every biome, starting with steppe and tundra, then mountains, in which the systems the game already has (rock, water, climate, plants, weather, landforms) are coupled so that, now and then, a unique and recognisable place comes out of them. A scoring tool comes first, so we can see what the world already produces and tune from data.

The reference is Breath of the Wild: a few unmistakable shapes to navigate by, smooth ground between them, and two or three landmarks visible from most places.

## The Problem

**Mountains are rough terraces.** The ground is stepped into rock beds and cut by noise into cliffs of at most 18 blocks. Walking up a range is jumping one terrace until the next appears, and nothing between them stands out. A player can climb almost any cliff, because a cliff made of small ledges is a staircase: a jump rises about 1.3 blocks and a step is 1, so any ledge up to about 2.3 blocks can be climbed.

**Little at the meso scale reads from distance.** At 4 km the world is drawn in LOD cells of 8 to 32 blocks, and past that only the 32-block map cell survives. The per-biome relief features are at most 18 blocks and are zero from 16-block cells, so they cannot be seen from a ridge. The landmark-class forms that exist (cold uplands at 55 to 95 blocks, inselbergs, tower karst, cirques, volcanic cones) number about 100 and several have a count of zero on some seeds. To read at 4 km a landmark needs to be at least about 130 to 190 blocks across with 250 to 400 blocks of relief over the local ground. That figure is an angular-size estimate, not measured in the game.

**Steppe and tundra are the least likely to produce anything.** Steppe is flattened on purpose (`valleys::flatten` pulls it 90% toward a 60-cell smoothing), and tundra has nothing taller than 18 blocks except a stamped cold upland. They are also the most developed biomes, so they are where the lack shows most.

**The systems barely drive each other's shape.** Rock feeds only erosion, stream power and the block drawn. Marble is weaker than granite in the erosion constants, and about a third of land is marble, so it is common, not distinctive. Terraces exist only on sedimentary and volcanic rock. Landforms never read climate, because water is routed after them, so a sheltered basin cannot change the biome.

**Proposed solution.** Build a scorer first, as a tool with no change to the map, to find and rank the world's most striking places per biome and seed. Then couple the systems with real thresholds so strong places occur naturally, site landmarks on steppe and tundra after the flattening, and rework mountains with steep, shedding walls and deeply carved valleys, after a spike on what the mesher can hold.

## Goals

*   **A scorer, built first.** A tool that ranks the most distinctive places per biome and seed, reading the terrain a player sees, with no layer added to the map and no `GENERATOR_VERSION` change in the first slice.
*   **Distinctive, of either sign.** A landmark is a place unlike the ground round it, high or low. It can be a peak, a butte, a bowl or a valley. The scorer measures relief of both signs.
*   **Readable from 4 km.** A landmark is at least about 130 to 190 blocks across with 250 to 400 blocks of relief over its surroundings, so the 32-block map cell carries it through the far LODs.
*   **A density band, not a count.** Landmarks are neither too sparse nor too busy. Some places have fewer and some more. The band's numbers are set from the scorer's data.
*   **Steppe and tundra keep their character.** Their plains stay plain, and landmarks stand out of them, sited after the flattening or exempt from it.
*   **Shelter changes the real climate.** Between ranges you can get a different biome. Shelter is the existing exposure measure at wider windows, in the climate pass, so biome, trees, weather and wind follow one source.
*   **Mountains shed the player.** Steep walls (over 66 degrees) leave no tiny ledge to stand on, so the player slides off instead of finding footing, and deep valleys give respite and a way across.
*   **Rock matters.** Rock type changes how steeply a face holds, how faceted the surface-net mesh is, and how water and erosion treat it, so a range of granite and one of marble look different.
*   **Judged by data first.** A fixed seed set and scripted ground-level viewpoints decide, for slice 1, whether conjunctions are strong and frequent enough, before any terrain change.
*   **Generation cost.** Each new stage is measured. The 0.3 second budget can rise modestly, since generation is up front.

## Non-Goals

*   Reachability guarantees. Some places may be out of reach on foot.
*   Place names. They may be generated later.
*   Massive waterfalls in the first slice. They need water-rule changes and a river designed with its cliff.
*   A rollout across every biome as a commitment. Steppe, tundra and mountains are the plan, and the rest follow the same mechanism with new predicates, only if the first slices pass.
*   Swamp and coast in the first batches. Volcanic ground is in scope.
*   A saved scorer layer, a changed world height, or a third valley profile before `run` and `run_alpine` are generalised.
*   Climbing or gliding. Traversal stays walking, jumping and stepping up.

## Open Questions

*   **Success-test numbers.** The density band, the number N of distinct landmarks visible within 4 km of a ridge point, and the distance M between stop-worthy features on a scripted walk.
*   **What the mesher holds.** Whether it can hold a 20 to 150 block near-vertical face at LOD 3, 4 and 5, or softens it to a slope, and how ground contact should let a player stand on a near-vertical face. Settled by a funded spike.
*   **Granite cores in marble.** Whether a granite core ringed by marble already occurs, or the geology gives one rock per range. The brief and the review disagree, and it is checked in the spike.
*   **First scorer terms.** Which terms rank steppe and tundra places, and whether they read the map or `WorldTerrain` heights.

## Critical User Journeys (CUJs)

### CUJ 1: Picking a landmark from a ridge

*   **Given**: A fixed seed, the player on a ridge or a high spawn point on steppe, tundra or in a range, midday, clear sky.
*   **When**: The player looks out over about 4 km.
*   **Then**: Distinct landmarks stand out (a mass, a butte, a bowl, a valley mouth), enough that the player can choose one to walk to. The count falls inside the density band and varies from place to place.

### CUJ 2: Walking and finding something worth stopping for

*   **Given**: The player walks a path of a few kilometres through one biome.
*   **When**: They keep going without a goal.
*   **Then**: A distinct feature appears often enough that the walk is not samey, and no two nearby features share the same composition.

### CUJ 3: A mountain wall the player cannot walk up

*   **Given**: A range with steep terraces.
*   **When**: The player tries to jump up a wall of more than 66 degrees.
*   **Then**: They slide off and find no footing on a tiny ledge. Gentler ground, valleys and gaps give routes, and some tops are out of reach.

### CUJ 4: Reviewing the world's striking places

*   **Given**: The scorer's layer in the world viewer on a seed.
*   **When**: The owner turns it on, pans and clicks a place.
*   **Then**: The top places per biome are marked, with each term that scored them and the rock there, so scoring can be tuned from data.

## Technical Plan (Architecture)

### Two phases in the pipeline

Landforms never read climate, because water is routed after them, and that rule lets water stand on the ground it will find. The design keeps it. Every stage that changes height reads only a snapshot of climate taken earlier, not live climate, as `valleys::favoured_layer` does now. Stages that read climate (colour, weather, feel, sound) change no height.

```text
  cause fields (before water)                          effect fields (after climate)
  rock, height, slope, range,        height-shaping    hydrology    climate           vegetation, colour,
  one shared climate snapshot  --->  archetype stage -> routing  --> + wide-window --> weather, feel, sound
                                     (rule table)                   exposure
                                          |
                    scorer reads WorldTerrain heights (what a player sees) -> viewer layer
```

### 1. The scorer (slice 1, no change to the map)

*   **Maths** is one pure module in `ochre-world` (a new `worldmap/meso/` submodule, new files only), reading `WorldTerrain` heights and not `map.height`, since mountain terraces are applied at block and LOD level. It never runs inside `generate`, so it never enters `MAP_HASH`.
*   **Terms:** relief of either sign over the local ground at three windows (about 8, 32 and 96 cells), steepness, rock, water and exposure. Scores are ranked per biome and seed, and a place needs two or more independent causes, co-located, in the tail. Near-duplicates are suppressed within 2 to 3 km and the top set is type-diverse.
*   **A null test** shifts one layer 50 km, to separate designed correlation from chance.
*   **Deterministic:** `libm` only, per-row reductions merged in row order, `total_cmp` sorting, byte-identical across thread counts.
*   **Front end (chosen: the world viewer).** A `worldview` layer and an endpoint returning the top places, with the logic in its own file and not in `worldview.rs`. The viewer holds one seed at a time, so a baseline across seeds is a small script that requests each seed. The viewer also gains the layers it lacks (`valley`, `alpine`, `exposure`, `water_closeness`, geology) so inputs can be seen.
*   **Config:** it generates with the game's character config (`character::world_config`), not `WorldConfig::default()`, and records which in its output.

### 2. Shelter as exposure at wider windows

`exposure::run` becomes a function of a window size. The saved 8-cell `exposure` byte layer is unchanged, because steppe trees, bare earth, rocks and wind depend on it. The same measure, height against the land average, runs at about 32 and 96 cells just before the climate pass and is read there directly, with no new stored layer. The climate pass adds shelter terms to temperature and humidity, so biome, trees, weather and the feel weights follow one source. Pockets must be at least about 0.4 km² to survive the biome tidy-up. Wind strength can follow the wide read too. Exposure needs only the final height, which is settled before climate.

### 3. Landmarks from a rule table (slice 2 onward)

*   **Decision (owner, B):** a data table of rows, one per biome and landform, naming the map layers read, the thresholds, a smoothstep window per input, a weight, and seeded ranges on every knob.
*   **Archetypes are built from primitives that combine:** a dip, a raised mass, a ridge, a terrace and a valley, each a height profile written once. Variety comes from which primitives a row combines, in what order and with what ranges. A new primitive is added only when the scorer shows a recognisable gap.
*   Landmarks are set into the map's heights, at least about 130 to 190 blocks across with 250 to 400 blocks of relief of either sign, so the 32-block map cell carries them through far LODs. On steppe and tundra they are laid after `valleys::flatten`, or exempt from it, with the edge checked for a seam.
*   They run before the last hydrology pass, so a river meeting a new cliff reacts by itself. Whether that makes a fall, a notch or a gorge is left to the water rules. Massive waterfalls are not a goal.
*   Lake thresholds are altered first, where a landmark bowl needs a basin the 300-cell drain would remove. Every world regenerates.

### 4. Mountains (slice 3)

*   **Valleys:** `valleys::run` and `run_alpine` (about 70% duplicate) become one function over a profile and a sited layer. The prototype's plumbing stays, and its siting moves from noise to drainage and range direction, judged in the game on three seeds first. The layer is renamed so it does not collide with the alpine tree rule and climate belt.
*   **Steep walls:** raise the risers on hard rock (`riser()` and `KEEPS` in `geology.rs`), widen per-rock talus, and make soft-bed share the "tricky crossing" dial. The aim is faces over 66 degrees with no ledge a player can stand on, so the player slides off. This is decided by the spike below, because the mesher's chisel snaps normals to 45-degree steps and the ground contact decides footing.
*   **Rock matters:** rock type sets faceting in the surface-net mesh, how steeply a face holds, and its erosion and water behaviour, with variation inside a range if the check finds one rock per range.
*   **Order:** any steepener sits before `carve_passes`, or the passes are checked again, so a range is not sealed unnoticed.

### 5. The spike (before slice 3)

Render synthetic walls (60, 70 and 80 degrees, 20, 60 and 150 blocks) and an existing steep valley wall with `worldprobe render` at LOD 3, 4 and 5. Measure the walkable fraction by landform and rock on 10 or more seeds, and define an unclimbable face height. Test where ground contact lets a player stand on a near-vertical face. Check whether a granite core ringed by marble already occurs.

### Slices

1.  **Scorer, baseline, spike.** No terrain change and no version bump.
2.  **Steppe and tundra landmarks** from the table, if the baseline shows natural emergence too weak.
3.  **Mountains.**
4.  **The rest**, only if slices 2 and 3 pass, with new rows and predicates.

### Compatibility and risk

*   Every generation change bumps `GENERATOR_VERSION`, re-pins `MAP_HASH` and the two real-map chunk hashes, and breaks coordinate-pinned tests. Tests assert behaviour (monotone, bounded, zero when an input is zero, statistical bounds over many seeds), not coordinates.
*   Branches that generate differently get their own `OCHRE_WORLD_DIR`, since the cache key is the version number, and a version is never reused.
*   Each stage is timed with `lap()` against the 0.3 second budget, which may rise modestly.
*   All maths uses `libm`.

## Alternatives Considered

### Alternative A: A catalogue of authored set pieces

*   **Pros**: Each place is designed, so it can be made to look right.
*   **Cons**: The owner wants places to emerge from the systems working together. A catalogue repeats the same few forms, which is the samey risk, and it does not use the rock, water and climate systems at all.
*   **Reason for Rejection**: The aim is emergence. The examples (a marble bowl with a lake, a waterfall cliff, a lush valley through a mountain) are illustrations, not a list to build.

### Alternative B: A separate amplifier from the start

*   **Pros**: Guarantees strong, rare places whatever the systems give.
*   **Cons**: It looks artificial and adds something to tune. The owner prefers nonlinear couplings: marble holds sheer faces, a basin that gathers water is a lake, a sheltered hollow grows more.
*   **Reason for Rejection**: Dropped as a commitment. It comes back only as a measured gate. If the scorer shows natural places are too weak or rare, an amplifier is added against a named smoothing stage, such as `valleys::flatten`, the valley soft edges or the map interpolation.

### Alternative C: Measure natural emergence on steppe and tundra

*   **Pros**: The same method everywhere.
*   **Cons**: Steppe is flattened on purpose and tundra has nothing taller than 18 blocks except a stamped cold upland, so measuring natural emergence there is likely to find little.
*   **Reason for Rejection**: Landmarks on steppe and tundra are sited after the flattening and measured as sited frequency and spacing. Natural emergence is measured mainly for mountains and wet biomes.

### Alternative D: The scorer as a CLI example, or both a CLI and a viewer layer

*   **Pros**: A CLI is scriptable and repeatable across a seed set. Both together cover exploring and baselines.
*   **Cons**: A CLI is a poor way to explore why a place scored as it did. Both together mean two front ends that can drift, and more code in slice 1.
*   **Reason for Rejection**: The owner chose the viewer layer, to explore by eye. The cost is that the viewer holds one seed at a time, so a cross-seed baseline needs a small script that requests each seed in turn.

### Alternative E: Landmarks as stamped shapes in `shapes.rs`

*   **Pros**: Matches the existing pattern for fjords, cones, inselbergs, uplands, karst and cirques, and each shape is easy to test.
*   **Cons**: Every kind is bespoke code in a file already about 950 lines, and the results repeat a handful of forms.
*   **Reason for Rejection**: The owner chose the rule table. Rows are cheaper to extend and tune in one place.

### Alternative F: The scorer as the site selector

*   **Pros**: Landmarks go where the systems already nearly line up, so they look earned.
*   **Cons**: The scorer's weights become the design, scoring is expensive to run in generation, and any change to the score moves every world.
*   **Reason for Rejection**: The scorer stays a tool for judging and tuning. It does not drive placement.

### Alternative G: Many archetypes, each with its own knobs

*   **Pros**: Each place type is exactly what it needs to be.
*   **Cons**: Every archetype is code with its own tests and LOD check, and every one changes generation, so it re-pins `MAP_HASH`, the real-map chunk hashes and the version. A bowl at three sizes is still a bowl.
*   **Reason for Rejection**: Archetypes are built from a few primitives (dip, raised mass, ridge, terrace, valley) that combine. A primitive is added only when the scorer shows a recognisable gap.

### Alternative H: Reusing Hotspots or `scatter()` as landmark anchors

*   **Pros**: Existing sites and spacing.
*   **Cons**: They are encounter sites with no conjunction logic, and their spacing is wrong for landmarks (about 6,000 blocks).
*   **Reason for Rejection**: Landmark sites come from the rule table.

### Alternative I: A new shelter term instead of reusing exposure

*   **Pros**: A clean new quantity, free of what exposure is used for today.
*   **Cons**: Exposure already is shelter turned upside down and already drives steppe wind, trees and bare ground. A second measure would duplicate it.
*   **Reason for Rejection**: Exposure is reused at wider windows. The saved 8-cell layer stays unchanged.

### Alternative J: Raising the world height, or a taller absolute altitude

*   **Pros**: "Taller than anything now" read literally.
*   **Cons**: Ground is clamped to 1,984 and ranges already reach it on some seeds. Raising it is a file-format and bench change.
*   **Reason for Rejection**: "Taller" means relief over the local ground, which works on lowland and in dips.

### Alternative K: Massive waterfalls and a third valley profile in the first slice

*   **Pros**: Closer to the owner's examples.
*   **Cons**: Waterfalls need water-rule changes and a river designed with its cliff. A third valley profile would add to the roughly 70% duplication between `run` and `run_alpine`.
*   **Reason for Rejection**: Deferred. Waterfalls may emerge where a new cliff meets a river. `run` and `run_alpine` are generalised before any new profile.

## Detailed Implementation Plan

Slices are in order and each ends in a commit on its own local branch, cut from the one before, with a `backup/` branch made first. Nothing is pushed. Slice 1 changes nothing in the map, so no version bump and no re-pin. Every later generation change is its own commit with a `GENERATOR_VERSION` bump, a re-pin, a `docs/terrain-perf.md` entry and a before and after score. The rule table is a Rust `const` table, like the valley profiles and every other generation constant, so it is type-checked, needs no parsing and is covered by the platform-maths scan.

### Slice 1: scorer, baseline, spike

#### 1. `crates/ochre-world/src/worldmap/meso/mod.rs` (Create)

*   **Type / Scope**: Rust module, public API of the scorer.
*   **Proposed Changes**: `pub struct Place { x, z, biome, score, terms, rock, weakest }` and `pub fn score(terrain: &WorldTerrain, map: &WorldMap, grid: Grid) -> Vec<Place>`, where `Grid` is the sample step (default every 4 cells, or 8 to 16 blocks). It reads `WorldTerrain` heights, never `map.height`, and calls nothing that writes the map. A header string records the config used (the game's character config), the git SHA and `GENERATOR_VERSION`.

#### 2. `crates/ochre-world/src/worldmap/meso/terms.rs` (Create)

*   **Type / Scope**: Rust module, the terms.
*   **Proposed Changes**: signed relief (height over, or under, the local land average) at three windows of about 8, 32 and 96 cells, using `blur_box`; steepness from block-level gradient; rock at the cell; water (falls and lakes from `RiverProfiles` and `map.water_kind`); exposure at the three windows; an angular salience term from a ring of vantage points at 3 to 5 km. Pseudocode: `term = rank_within_biome(value)`, so no single field dominates.

#### 3. `crates/ochre-world/src/worldmap/meso/rank.rs` (Create)

*   **Type / Scope**: Rust module, ranking.
*   **Proposed Changes**: a place needs two or more independent terms in the top tail and co-located (overlap of tail masks, not a sum); a category's contribution is capped; non-maximum suppression within 2 to 3 km; a type-diverse top-K; a null variant that shifts one layer 50 km to measure chance. Sorting uses `total_cmp` with ties broken by (z, x). Per-row reductions are merged in row order.

#### 4. `crates/ochre-world/src/worldmap/mod.rs` (Modify)

*   **Type / Scope**: Rust module list.
*   **Proposed Changes**: `pub mod meso;` only. `generate` is untouched, so `MAP_HASH` does not move.

#### 5. `examples/common/meso_view.rs` (Create) and `examples/worldview.rs` (Modify)

*   **Type / Scope**: viewer layer, endpoint and the missing input layers.
*   **Proposed Changes**: a `meso` layer drawing the top places per biome, and a `/places?seed=S` endpoint returning them with each term, the rock and the weakest link. The logic lives in `meso_view.rs`, included by `#[path]`, so `worldview.rs` (about 960 lines) grows by wiring only. The viewer also gains the `valley`, `alpine`, `exposure`, `water_closeness` and geology layers, three lines each in `layer_pixel`.

#### 6. `scripts/meso_baseline.sh` (Create) and `docs/meso-baseline.csv` (Create)

*   **Type / Scope**: baseline script and its output.
*   **Proposed Changes**: for each seed, request `/places` from the viewer and write one CSV (seed, rank, x, z, biome, score, each term, rock, weakest link). The header gives the git SHA, `GENERATOR_VERSION`, the character config and the grid step. Seeds: 1, 3, 4, 20, 42 and 1337, plus about four tundra-rich seeds from `docs/census-pr6.csv`, with at least three seeds per biome that have enough land.

#### 7. The mesher and ground-contact spike

*   **`crates/ochre-terrain/src/generate/generator/tests.rs` (Modify)**: build synthetic walls (60, 70, 80 degrees; 20, 60, 150 blocks) and report which the mesher holds as a wall and which it softens, and whether a standing player finds footing on any tiny ledge.
*   **`examples/worldprobe.rs` (Modify)**: `render` the same walls and an existing steep valley wall (seed 20 near 1450, -10900) at `OCHRE_RENDER_LOD` 3, 4 and 5, and print a walkable-fraction histogram by landform and rock over 10 or more seeds. Keep the addition small, since the file is 798 lines.
*   **`docs/meso-spike.md` (Create)**: the results, an "unclimbable face height" metric, whether a granite core ringed by marble already occurs, and the conclusion for slice 3.

#### 8. Docs for slice 1

*   **`docs/landforms.md` (Modify)**: a section on the scorer, and the "landforms ignore climate" rule restated as the two phases.
*   **`docs/world-plan.md` and `docs/terrain-glossary.md` (Modify)**: the chosen term ("place") in the glossary, and the fall threshold corrected (the stale "2.5" in the glossary).
*   **`crates/ochre-world/src/worldmap/README.md` (Modify)**: the `meso` module and its invariants.
*   **`AGENTS.md` (Modify)**: a pointer line under "Where things are".
*   **`docs/issues.md` (Modify)**: remove steppe "to check" entries that slice 1 resolves, and add any defects the scorer finds, with known and guess kept apart.

#### 9. Tests for slice 1 (inside the new files)

*   Monotone and bounded in 0 to 1, zero when a required input is zero, on synthetic `WorldMap::empty` grids.
*   Statistical bounds over many seeds, not coordinates.
*   The null variant scores lower than the real one.
*   Byte-identical CSV for 1 and 7 threads.
*   `plugin_subsets` and every existing pin stay green.

### Slice 2: steppe and tundra landmarks and shelter

#### 10. `crates/ochre-world/src/worldmap/meso/primitives.rs` and `table.rs` (Create)

*   **Type / Scope**: height primitives and the rule table.
*   **Proposed Changes**: primitives (dip, raised mass, ridge, terrace, valley), each a pure profile in blocks with seeded ranges on size and depth, using `libm` only. `table.rs` is a `const` array of rows: biome, primitives with order and weights, the map layers read, thresholds and a smoothstep window per input, spacing, and seeded ranges on every knob. A new primitive is added only when the scorer shows a gap.

#### 11. `crates/ochre-world/src/worldmap/mod.rs` (Modify)

*   **Type / Scope**: the pipeline.
*   **Proposed Changes**: call the archetype stage after `valleys::run` and `run_alpine` and before `hydrology::scatter`, with a `lap()`, so water is routed over the new ground. `GENERATOR_VERSION` bumps, and `generate_with` gets a flag to skip the stage for an A/B.

#### 12. `crates/ochre-world/src/worldmap/valleys.rs` (Modify)

*   **Type / Scope**: flatten.
*   **Proposed Changes**: an exemption hook so landmark sites on steppe are laid after `flatten` or excluded from it, and a check of the 40-cell softened edge for a visible seam.

#### 13. `crates/ochre-world/src/worldmap/hydrology.rs` (Modify)

*   **Type / Scope**: lake thresholds.
*   **Proposed Changes**: relax `MIN_LAKE_CELLS` (300) and `MIN_LAKE_DEPTH` for basins a landmark archetype makes, as the first step before any keep-mask. Every lake on every seed may change, and every world regenerates.

#### 14. `crates/ochre-world/src/worldmap/exposure.rs` and `climate.rs` (Modify)

*   **Type / Scope**: shelter.
*   **Proposed Changes**: `exposure::run` becomes a function of window size; the saved 8-cell layer is produced as before. Just before the climate pass, compute the 32 and 96-cell reads in memory and add shelter terms to temperature and humidity, with the `closeness`-style reach. Pockets under about 0.4 km² are still tidied. No new stored layer.

#### 15. `crates/ochre-world/src/worldmap/character.rs` (Modify)

*   **Type / Scope**: density knobs.
*   **Proposed Changes**: a landmark density band in `LandformKnobs`, not stored in the map. A uniqueness test for the salts.

#### 16. Pins, docs and perf

*   **`crates/ochre-world/src/worldmap/portable_maths.rs`, `crates/ochre-terrain/src/generate/generator/determinism.rs`, `crates/ochre-world/src/worldmap/source.rs` tests (Modify)**: re-pin `MAP_HASH` and the real-map chunk hash and move any coordinate-pinned test, including the one ignored on `exp/meso-geography`.
*   **`docs/terrain-perf.md`, `docs/climate.md`, `docs/landforms.md`, `docs/designs/geography-colour-and-atmosphere.md` (Modify)**: stage cost, the shelter terms, the table, and the cross-reference.

### Slice 3: mountains

#### 17. `crates/ochre-world/src/worldmap/valleys.rs`, `mod.rs`, `io.rs`, `source.rs` (Modify)

*   **Type / Scope**: one valley function.
*   **Proposed Changes**: merge `run` and `run_alpine` into one function over a profile and a sited layer. Rename `map.alpine` (it collides with the alpine tree rule and climate belt). Site troughs along high-flow paths that cross a range, depth by flow and rock, gated by uplift, and raise `upper` (1400) so the tallest massifs can be carved. Bake wall steepness into a map layer read by one lookup, so `Valley::at` runs less often in the block path.

#### 18. `crates/ochre-world/src/worldmap/geology.rs`, `erosion.rs` (Modify)

*   **Type / Scope**: rock drives shape.
*   **Proposed Changes**: raise `riser()` and `KEEPS` on hard rock; make talus depend on rock so marble holds steeper than it does now and granite domes; make soft-bed share the "tricky crossing" dial. Add rock variation inside a range if the spike finds one rock per range.

#### 19. `crates/ochre-terrain/src/generate/generator/nets.rs` and `crates/ochre-gameplay/src/contact.rs` (Modify)

*   **Type / Scope**: facets and footing.
*   **Proposed Changes**: rock type sets the chisel's facet strength. Near-vertical faces get no foothold on small ledges: smooth ledges below a size on faces over 66 degrees, and have ground contact let the player slide off. The exact change is chosen from the spike.

#### 20. `crates/ochre-world/src/worldmap/gameplay.rs` (Modify)

*   **Type / Scope**: passes.
*   **Proposed Changes**: any steepener runs before `carve_passes`, or the passes are checked again afterwards, and the test `river_bed_depth_is_not_a_mountain_barrier` is checked against the new ground.

### Slice 4: the rest

New rows and predicates in `table.rs` for the other biomes, only if slices 2 and 3 pass, with a time-box and a done criterion per biome. Swamp and coast come later. Volcanic ground uses the same mechanism.

### Verification Plan

*   **Compilation/Build**: `cargo build --release --workspace --all-targets`.
*   **Unit Tests**: `cargo test --release --workspace`, plus the new tests above, with pinned hashes re-pinned only in a generation commit.
*   **Static Analysis & Linting**: `cargo clippy --workspace --all-targets`.
*   **Test Coverage Requirement**: behaviours covered, not coordinates. No percentage target.
*   **E2E/Integration**: `bench` vertex counts within 1.10 times and printed; `water_audit` with no uncontained cells and no lake or coast walls; census with no biome over 70% of land and a spawn on every seed; each new stage within the 0.3 second budget, which may rise modestly; scripted ground-level viewpoints on a fixed seed set against the success-test numbers once they are set; contact sheets and flythrough frames from the game for the owner to judge.
*   **Kill criteria**: fewer than 7 of 10 ridge viewpoints per seed showing an unmistakable landmark at 2 to 4 km means height is not the lever; natural co-occurrence under about 1 per 100 km² on steppe and tundra means placed sites with predicates, not emergence; a stage over budget, vertices over 1.10 times, or a biome over 70% of land stops the slice. These numbers are to be confirmed with the owner.

--------------------------------------------------------------------------------

*Drafted with EGM*
