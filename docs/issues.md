# Known issues

Problems seen in the game or the tools that are not fixed yet. When one is fixed, delete its entry in the commit that fixes it, and say in the commit what the cause was. Add one when you find a problem you are not going to fix now.

An entry says where it was seen, what is wrong, and what is known and what is only a guess, so nobody mistakes a guess for the cause. The tectonic world's own defect table, with the water and LOD limits designed in, is in `docs/world-plan.md` (Open defects); this log is for everything else.

## A thin strip of grass or dirt inset from a rock's edge

- **Seen:** 2026-11-18, playing the `feat/geography-colour` build. On rock's sculpted ground, a thin strip of quads in grass or dirt shows a little way in from the rock's edge, instead of the rock running to the edge. The seed and place were not recorded, and there is no screenshot.
- **Known:** the owner says it happens sometimes, and that it comes from the dual contouring, the way rock's vertices are placed where its planes meet (`docs/sculpted-ground.md`, The mesh). Not checked on `main`, so not known to be new.
- **Not known:** the cause. A guess only: a vertex on rock's edge takes the material of its cell's highest solid corner with air over it, so where the edge's cells hold a grass or dirt corner, the quads on that strip blend to it along the `other_cell` edge (`world.wesl`) while the rock's vertices sit further out. Not looked into.
- **Lead, 2026-11-19:** `OCHRE_RENDER_FLAT=1 worldview 1 render 10816 -6784 14 out.png` shows thin grass slivers between rock faces on a cliff, not confirmed to be this.
- **To check:** find one and note the seed and place. Render it with `worldview <seed> render <x> <z> <half> out.png` with `OCHRE_RENDER_FLAT=1`, to see whether the strip's triangles carry a second material, and compare the strip's vertices' materials with the rock's beside it.

## Steppe grass bends in a hollow while its wind sound is quiet

- **Seen:** 2026-11-16, by design of the steppe change. The wind sound's strength follows how exposed the camera's ground is (`Surroundings::exposure`), and the foliage shader's sway size is the same everywhere (`foliage.wesl`), because a shader has no exposure per vertex.
- **Known:** the gust's timing is synced between the two (`gust_at` in `ambience/wind.rs` and `foliage.wesl`, which must be kept in step), its size is not.
- **To fix:** pass the exposure, or a wind strength, into the shader, for instance as a per-vertex value or through a buffer the weather already updates each frame.

## Steppe's streams and the climate shift are not checked as widely as the rest

- **Seen:** 2026-11-16. The dryness curve and the bank cap change rivers, lakes and the grassland and forest shares on every seed (`docs/terrain-perf.md`, Streams in dry country), and were measured on five seeds only.
- **Since:** `water_audit --fine` on seeds 1, 42 and 1337 (2026-11-19) finds no lake or coast walls or steps and no uncontained map cell; river walls are in the seed 42 entry below. The census and frame times were not rerun.
- **To check:** the census (`scripts/census.sh`) for a biome over 70% of land.
## Steppe on hills wider than a few kilometres stays hilly

- **Seen:** 2026-11-16, seed 3. The flattening of steppe ground (`valleys::flatten`) removes hills smaller than about 4 km. Seed 3's steppe stands on foothills, and a larger smoothing radius makes steep edges where the flattened zone ends.
- **Known:** median slope fell by about two thirds on the seed 3 windows with no valley cut, relief over 1.5 km by about a fifth (`docs/terrain-perf.md`).
- **Not known:** whether steppe should be kept off foothills in the first place, which is a change to `classify` and the census shares.

## River walls rise near steppe valleys on seed 42

- **Seen:** 2026-11-16, `water_audit --fine`. Tall river walls went from 31 to 58 on seed 42 against `main` (seeds 1 and 1337 fell or were level).
- **Known:** valleys now cross rivers, and the audit counts the waterfalls the water rule allows as walls. The audit looks at every 100th edge cell (`PICK_EVERY`), so its counts move with the river layout: on 2026-11-19 the same seeds read tall walls 26, 52 and 46 (seeds 1, 42, 1337), and with every 20th cell 329 on seed 42 and 263 on 1337. After small basins were filled or breached shallow (`docs/water.md`, step 3) they read 24, 52 and 46, and 376 and 263 with every 20th; raised banks on 1337 went from 105 to 210 at every 20th.
- **Not known:** whether the walls are waterfalls over valley walls or leaks. Not checked. The audit prints only its first few examples, all mountain waterfalls on seed 1337.
## The steppe change is not measured for speed

- **Seen:** 2026-11-16. Frame times, `bench` vertex counts and the cost of the foliage shader's second gust term were not measured for the valleys, flattening, exposure, flora and wind. The valley stage in map generation takes 0.30 to 0.51 s against the 0.3 s a new stage may add (`docs/world-plan.md`).
- **To check:** `profile` skill on a steppe seed against `main`; `OCHRE_STAGE_TIMES` for the stage.

## Only forests have birdsong

- **Seen:** 2026-11-16. The ambient birds are one recorded loop (`Loop::Forest`, `forest.ogg`), played in proportion to the trees round the camera, so steppe, tundra, desert and the other treeless biomes have none. Their starling flocks and goose skeins are sized by biome (`docs/climate.md`, Birds by biome), but they make no sound.
- **To fix:** a loop per biome, for example a lark or pipit over the steppe: a new `Loop` variant for each and a recording, CC0 if the licence rule stays (`assets/sounds/CREDITS.md`).

## Steppe's own ground texture is a derived first try, with a hard edge

- **Seen:** 2026-11-16. The grass layer's leaves read as too grassy on the steppe, so grass tops there draw `steppe_top` (`docs/climate.md`, Steppe).
- **Known:** the texture is made by `steppe_ground.py` from the dirt and grass layers, not painted: it is looked at only as a preview, tinted and tiled, and not yet judged in the game. The mesher switches texture at the steppe's edge, along one block's edge, under a tint that blends smoothly; the pebbles' colour is worked out for the steppe's golden tint, so a greener tint (steppe by a forest) shows them olive and dull.
- **Not known:** how the edge and the pebbles look, whether the roughness suits the painterly lighting, and the cost: the steppe's edge splits quads that used to merge, and `bench` vertex counts were not rerun.
- **To check:** look at a steppe edge in the game, run `bench` for vertex counts, and consider painting it with SDXL (`lora_paint.py`) as a candidate if the derived one is not liked.

## The steppe flower and its butterflies are a first try

- **Seen:** 2026-11-16. Poppies and dandelions were taken off the steppe, and a dusty-lilac `SteppeFlower` put in a few drifts (`docs/climate.md`, Steppe), with butterflies taking to it one time in five.
- **Known:** the sprite is bold strokes with a painting's tone over them, looked at only over flat colours; the drifts are about a seventh of grassland's by count (`flora.rs`, `STEPPE_FLOWERS`); the butterfly share is a guess.
- **Not known:** how it reads in the game against the steppe ground, or whether one species of flower is too few.
- **To check:** walk a steppe hollow, and count the butterflies there against grassland's.

## LOD nodes draw water with the old block edges, not the shore cut

- **Seen:** 2026-11-17, reading the sculpted-ground change. Full-detail chunks cut the water surface along the sculpted ground (`push_shore`, `mesh.rs`); the water of a node uses the old block edges.
- **Known:** by reading the code. Nodes of cells 1 and 2 now lay a bank's sand as chunks do (`columns::shore_top`), and trees no longer root on it; coarse cells (4 and up) are wider than the 2-block beach and keep the column's own water.
- **Not known:** how visible the water's edge is at the hand-over from chunk to node.

## Near-flat banks leave a saw-tooth waterline

- **Seen:** 2026-11-17, in play, lake edges where the sculpted ground lies just under the water. The waterline follows the ground's triangles, so it zigzags instead of running smooth.
- **Known:** it is the ground's shape meeting the shore cut, not the water mesh alone.
- **Not known:** the fix. Raising `SHORE_CUTS` (`mesh.rs`) gives a finer cut and was not tried against it, and smoothing the ground there would change the look.

## A chunk's edge lower than the LOD node beside it opens a gap from outside

- **Seen:** 2026-11-17, found while building the sculpted LODs, never seen in play. A full-detail chunk whose edge lies lower or further in than the node next to it leaves a gap, visible only from outside the full-detail area, where the eye is not.
- **Known:** skirts hang from the node's open edges to close the usual cases (`hang_skirts`), and the test allows open edges where a skirt ends.
- **Not known:** how often it shows from a mountain or the air.

## Rock's facets turn at every vertex in 8-block cells

- **Seen:** 2026-11-17, by design. The chisel turns its directions slowly over about 9 blocks, so a cell 8 blocks wide changes direction at each vertex and far rock reads as noise.
- **Not known:** whether anyone sees it at that distance. Not looked at.

## Grass tint pops where a chunk hands over to a node near a river

- **Seen:** 2026-11-19, `test_grass_tint_barely_changes_where_a_chunk_hands_over_to_a_node`, when a change to the rivers moved one onto the test's seed 5 spot (512, -3072): its gap went from 0.012 to 0.065. Over twelve level 3 nodes of seed 5 the gap runs from 0 to 0.106 (mean 0.033) before the change and to 0.100 (mean 0.030) after, so the test's spots were ones that happened to be away from water. The spot is now (-2048, -3072).
- **Known:** a coarse node samples tint every 32 blocks (`TINT_SPACING`, `buffers.rs`) and a chunk only at its corners, while a river's bank wetting (`BANK_WETTING`, `vigour` in `source.rs`) lifts humidity from 0.12 to 0.35 over about 50 blocks, so the two interpolate it differently.
- **Not known:** whether 0.1 shows in the game, a tenth of a channel at a node's edge.
- **To fix:** sample tint closer near water, or blend the node's tint toward the chunk's over the handover.

