# Known issues

Problems seen in the game or the tools that are not fixed yet. When one is fixed, delete its entry in the commit that fixes it, and say in the commit what the cause was. Add one when you find a problem you are not going to fix now.

An entry says where it was seen, what is wrong, and what is known and what is only a guess, so nobody mistakes a guess for the cause. The tectonic world's own defect table, with the water and LOD limits designed in, is in `docs/world-plan.md` (Open defects); this log is for everything else.

## Steppe grass bends in a hollow while its wind sound is quiet

- **Seen:** 2026-11-16, by design of the steppe change. The wind sound's strength follows how exposed the camera's ground is (`Surroundings::exposure`), and the foliage shader's sway size is the same everywhere (`foliage.wesl`), because a shader has no exposure per vertex.
- **Known:** the gust's timing is synced between the two (`gust_at` in `ambience/wind.rs` and `foliage.wesl`, which must be kept in step), its size is not.
- **To fix:** pass the exposure, or a wind strength, into the shader, for instance as a per-vertex value or through a buffer the weather already updates each frame.

## Steppe's streams and the climate shift are not measured for frame time

- **Seen:** 2026-11-16. The dryness curve and the bank cap change rivers, lakes and the grassland and forest shares on every seed (`docs/terrain-perf.md`, Streams in dry country).
- **Since, 2026-11-19:** `water_audit --fine` on seeds 1, 42 and 1337 finds no lake or coast walls or steps and no uncontained map cell; its river "walls" are steps in a river's level, not leaks (they compare samples 4 blocks apart, and its one-block check finds no water beside lower dry ground and no open block edge). The census over seeds 1 to 64 finds no biome over 70% of land: the most is forest at 56% on one seed, steppe averages 19% and reaches 47%.
- **To check:** frame times, with the `profile` skill.
## Steppe on hills wider than a few kilometres stays hilly

- **Seen:** 2026-11-16, seed 3. The flattening of steppe ground (`valleys::flatten`) removes hills smaller than about 4 km. Seed 3's steppe stands on foothills, and a larger smoothing radius makes steep edges where the flattened zone ends.
- **Known:** median slope fell by about two thirds on the seed 3 windows with no valley cut, relief over 1.5 km by about a fifth (`docs/terrain-perf.md`).
- **Not known:** whether steppe should be kept off foothills in the first place, which is a change to `classify` and the census shares.

## The steppe change is not measured for speed

- **Seen:** 2026-11-16. Frame times, `bench` vertex counts and the cost of the foliage shader's second gust term were not measured for the valleys, flattening, exposure, flora and wind. The valley stage in map generation takes 0.30 to 0.51 s against the 0.3 s a new stage may add (`docs/world-plan.md`).
- **Since, 2026-11-19:** `OCHRE_STAGE_TIMES` with no map cached reads the valley stage at 0.93, 0.89 and 1.22 s on seeds 1, 3 and 20, and alpine valleys at 0.43 to 0.60 s, of about 14 s for the map. These ran on a 4-core container, where map generation uses 2 worker threads (`worker_threads`, every logical CPU but two), so they are not comparable with the 0.30 to 0.51 s above. Timers inside `valleys::run` on seed 1 put about 0.22 s in the steppe-likeness layer (`favoured_layer`, which works out the climate fields again), 0.19 s in `flatten`'s blurs, 0.26 s in `siting` (0.18 s of it a priority flood) and 0.14 s in `fill_where`'s flood. Caching `flood_ties`, which five stages each work out, saved nothing measurable over three runs.
- **To check:** `profile` skill with perf on the stage; the climate fields and the siting flood are the likeliest to share with the stages before and after.

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
- **Known:** it is the ground's shape meeting the water, not the water mesh. The water plane is hidden wherever the ground is over it, so the visible edge is the ground's own contour at the level, however the shore is cut. Raising `SHORE_CUTS` from 4 to 16 (seed 1, `worldview render` at 6052,520, 7028,-3872, 6962,-4064) moves the edge by under a quarter block and changes the picture only where 4 cuts left a 1/4 block kink, for 3.4 times the water triangles (7,210 to 24,648 in the first view) and no change in `bench` vertex counts (the water is not counted there). At those three banks, whose ground lies 0 to 0.6 under the level for 3 to 40 blocks, the game showed only a mild wobble, not teeth. The teeth in the CPU render come from its flat colour per triangle.
- **Not known:** where it shows worst in play. A fix has to change either the ground near the water (its look) or lift the water at its edge to meet the ground, a meniscus a tenth of a block high, which is a look to judge by eye.

## Sea shores are cut along block edges, not along the ground

- **Seen:** 2026-11-23, on seed 1 at 5025,-432 (a flat shelf 0.55 under the sea, rising 2 blocks in one) in the game and in `worldview render`. The sea's edge runs in straight block-aligned steps and square corners, where a lake's follows the ground.
- **Known:** `push_shore` and the reach over the bank apply only to fresh water (`top_block` needs `water_block_level`, which only lakes and rivers have), so the sea's top faces end at the block edge.
- **Not known:** how visible it is from the beach at eye height, or whether the shelf is typical.

## A chunk's edge lower than the LOD node beside it opens a gap from outside

- **Seen:** 2026-11-17, found while building the sculpted LODs, never seen in play. A full-detail chunk whose edge lies lower or further in than the node next to it leaves a gap, visible only from outside the full-detail area, where the eye is not.
- **Known:** skirts hang from the node's open edges to close the usual cases (`hang_skirts`), and the test allows open edges where a skirt ends.
- **Not known:** how often it shows from a mountain or the air.

## Stacked LOD nodes still z-fight a little on their shared row

- **Seen:** 2026-11-19, with `OCHRE_RENDER_ZFIGHT` in the worldview renderer (`docs/sculpted-ground.md`, Checking it), after the fix for skirts flickering on distant cliffs: skirts lay in a node's side plane with the neighbour's skirts and its walls across the side, and the row past a node's bottom lay over the top row of the node below and hung the same skirts.
- **Known:** 49 to 117 pixels of about a million drawn still z-fight at 0.001 blocks on seed 1 (10816, -6784) and seed 4 (-6076, 5236), levels 1 to 3, against 1,748 to 22,854 before. The samples are the row past the bottom near its top, where it meets the row below, and the flat crossings both nodes draw in that one cell row, which can't lean.
- **Not known:** whether the rest shows in the game. The renderer is orthographic with no depth precision limit, so the counts are geometry, not the GPU's depth test. Level n beside level n + 1, and a chunk beside a node at the hand-over, were not measured.
- **To fix:** make the two nodes' vertices on their shared row agree (`docs/sculpted-ground.md`, Open).

## Rock's facets turn at every vertex in 8-block cells

- **Seen:** 2026-11-17, by design. The chisel turns its directions slowly over about 9 blocks, so a cell 8 blocks wide changes direction at each vertex and far rock reads as noise.
- **Not known:** whether anyone sees it at that distance. Not looked at.

## Some river cells still have no water at block scale

- **Seen:** 2026-11-19, `worldview <seed> dryrivers 0 0`: 528 of 9,853 river cells on seed 1, 693 of 13,825 on seed 42 and 693 of 12,507 on seed 1337 have no river or lake water anywhere in them (sampled every 4 blocks), after mountain rivers were fixed (they were 1,233, 1,040 and 1,260; see `docs/terrain-perf.md`, Mountain rivers reach their lakes).
- **Known:** the worst left on seed 1 is at -2704,14928, a map surface of 574 over ground 113 blocks under it. Not looked into.
- **Not known:** how many are small streams ending in hollows, rivers that stop at a fall onto ground under a lake they don't drain into, or something else.
