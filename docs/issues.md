# Known issues

Problems seen in the game or the tools that are not fixed yet. When one is fixed, delete its entry in the commit that fixes it, and say in the commit what the cause was. Add one when you find a problem you are not going to fix now.

An entry says where it was seen, what is wrong, and what is known and what is only a guess, so nobody mistakes a guess for the cause. The tectonic world's own defect table, with the water and LOD limits designed in, is in `docs/world-plan.md` (Open defects); this log is for everything else.

## Holes in the mesh on a steep steppe mountain's cliffs, seed 1

- **Seen:** 2026-11-18, playing `feat/meso-slice1` (seed 1, `OCHRE_SPAWN=13408,-4704,60`), on the peak of a steppe mountain over a lake and on its steep cliff faces. The owner saw holes in the cliff mesh. The owner did not record the exact spots.
- **Known:** the place is the scorer's best steppe place on seed 1 (`docs/meso-baseline.csv`, relief 110, 235 and 315 blocks at the three windows). The build includes the ranges' valleys from `exp/meso-geography`, so seed 1's mountains differ from `main`'s.
- **Known:** the owner confirmed the holes are in full-detail ground. `worldprobe 1 render 13408 -4704 400` finds no open edge there but 15,347 folded triangles (winding against their own vertices' normals), 99% in rock, scattered through the peak's chunks. The same count at the seed 4 marble arch finds 23 folded triangles, all in the two chunks holding that hole. They exist before the chisel and are not made by the plane solve's overshoot clamp (`docs/meso-spike.md`). An earlier version of this entry said the ground was closed: it counted open edges only, and a folded triangle has neighbours on every edge.
- **Fixed locally, not confirmed in the game:** the mesher now holds every vertex inside its cell across its crossing axes and splits each quad from the final positions, so no triangle folds (`docs/sculpted-ground.md`). Delete this entry once the owner sees the arch and the peak's cliffs without holes.
- **Not known:** whether folded triangles are the whole cause (culling them in a level render shows a sliver at the arch's base but not the pillar's window), which cell patterns fold, and whether the chunk-to-node hand-over on a sheer face adds a second cause. A guess only: the dual-contoured plane solve (`nets.rs`, planes from the blurred gradient with `PULL` 0.05) on cells with few solid corners or seven of eight.
- **To check:** `OCHRE_RENDER_CULL=out OCHRE_RENDER_PITCH=4 OCHRE_RENDER_FLAT=1 worldprobe 4 render 11393 -20180 14 out.png <azimuth>` against the same without the cull, from the screenshot's angle, and a count of folded triangles by cell pattern.

## A thin strip of grass or dirt inset from a rock's edge

- **Seen:** 2026-11-18, playing the `feat/geography-colour` build. On rock's sculpted ground, a thin strip of quads in grass or dirt shows a little way in from the rock's edge, instead of the rock running to the edge. The seed and place were not recorded, and there is no screenshot.
- **Known:** the owner says it happens sometimes, and that it comes from the dual contouring, the way rock's vertices are placed where its planes meet (`docs/sculpted-ground.md`, The mesh). Not checked on `main`, so not known to be new.
- **Not known:** the cause. A guess only: a vertex on rock's edge takes the material of its cell's highest solid corner with air over it, so where the edge's cells hold a grass or dirt corner, the quads on that strip blend to it along the `other_cell` edge (`world.wesl`) while the rock's vertices sit further out. Not looked into.
- **To check:** find one and note the seed and place. Render it with `worldprobe <seed> render <x> <z> <half> out.png` with `OCHRE_RENDER_FLAT=1`, to see whether the strip's triangles carry a second material, and compare the strip's vertices' materials with the rock's beside it.

## A hole in the mesh at a marble arch on seed 4

- **Seen:** 2026-11-18, seed 4, at (11393, 110, -20193), a block outline on the ground beside a giant marble arch in the tundra, on the `feat/geography-colour` build. The screenshot is `docs/issues/mesh-hole-seed4-11393-110--20193.jpg`: sky shows through a gap in the arch's pillar just over the outlined block.
- **Known:** the owner says it is not from the geography colour change. Not checked on `main`.
- **Not known:** the cause, or whether the gap is the arch's designed opening (3.5 to 4.5 blocks between pillars) and not a hole. From one frame it reads as a hole in the pillar's face. Not reproduced. A guess only: it is where the arch's pillar meets its capstone, which `generate/rocks.rs` builds block by block.
- **Since:** `worldprobe 4 render 11393 -20193 40` finds no open edge but 23 folded triangles in the chunks (11376, 112, -20192) and (11392, 112, -20192), the only anomaly in the area (see the seed 1 entry above). **To check:** `test_sculpted_ground_leaves_no_hole`'s checker looks for open edges only, so it cannot see this. See Open in `docs/sculpted-ground.md`, which already lists rare holes seen in play and not found by the tests.

## A river through the steppe is a chain of round dips, not a carved valley

- **Seen:** 2026-11-16, in the steppe, playing the steppe branch. The seed and place were not recorded. A river's bed shows as a dried-up line of stepped, circular dips one after another, where a river should lie in a carved valley.
- **Known:** the owner says it has been there for a while and is not from the steppe change. Nobody has looked into it. Steppe's rivers used to die within a cell or two, since all of it was counted as desert-dry; the dryness curve now lets streams run through it (`docs/water.md`, Water in dry country), which may change this, and was not checked against it.
- **Found, 2026-11-16 (seed 20, near 1450,-10900):** one cause. A valley's floor was a long closed trough, since the valleys drained only the basins they made new, and in two rounds. With the dryness curve letting water in, a dry-country lake stands just over its floor, so the trough held a string of one-to-five-cell ponds with dry ground between, which read as a stream of dashes, and each pond wet the ground 14 cells round it into grassland. Valleys now fill any shallow basin lying mostly in a valley up to its dam (`valleys.rs`, `hydrology::fill_where`), and a lake under 20 cells wets no ground (`climate::RIPARIAN_LAKE_CELLS`). Seed 20's trough is now a dry valley with no water.
- **Not known:** whether every chain of dips is this. Tiny lakes in valleys fell from 18, 17 and 4 to 6, 6 and 0 on seeds 20, 1 and 1337, not to none: the rest are not found. Other dips, in non-valley country, were not looked at.
- **To check:** render the remaining valley ponds (worldview's `/export` with the `hillshade` and water layers at `scale=0.5`), and whether a dry valley should instead carry a stream through (the map has the stream's cells in the trough, which the drain now cuts out of).

## `storm.ogg` and `desert.ogg` are not in the repo

- **Seen:** 2026-11-16, listing `assets/sounds/music/`. `music.rs` (`Mood::Storm`, `Mood::Desert`) names both files, and only the cold, forest, grassland, rainforest, savanna and steppe tracks exist.
- **Known:** the files are missing.
- **Not known:** what a player gets in a storm or the desert, silence or an error in the log. Not checked.

## Steppe grass bends in a hollow while its wind sound is quiet

- **Seen:** 2026-11-16, by design of the steppe change. The wind sound's strength follows how exposed the camera's ground is (`Surroundings::exposure`), and the foliage shader's sway size is the same everywhere (`foliage.wesl`), because a shader has no exposure per vertex.
- **Known:** the gust's timing is synced between the two (`gust_at` in `ambience/wind.rs` and `foliage.wesl`, which must be kept in step), its size is not.
- **To fix:** pass the exposure, or a wind strength, into the shader, for instance as a per-vertex value or through a buffer the weather already updates each frame.

## Steppe's streams and the climate shift are not checked as widely as the rest

- **Seen:** 2026-11-16. The dryness curve and the bank cap change rivers, lakes and the grassland and forest shares on every seed (`docs/terrain-perf.md`, Streams in dry country), and were measured on five seeds only. `water_audit`, the many-seed census and frame times were not rerun.
- **To check:** `water_audit --fine` on seeds 1, 42 and 1337, and the census (`scripts/census.sh`) for a biome over 70% of land.

## Steppe on hills wider than a few kilometres stays hilly

- **Seen:** 2026-11-16, seed 3. The flattening of steppe ground (`valleys::flatten`) removes hills smaller than about 4 km. Seed 3's steppe stands on foothills, and a larger smoothing radius makes steep edges where the flattened zone ends.
- **Known:** median slope fell by about two thirds on the seed 3 windows with no valley cut, relief over 1.5 km by about a fifth (`docs/terrain-perf.md`).
- **Not known:** whether steppe should be kept off foothills in the first place, which is a change to `classify` and the census shares.

## River walls rise near steppe valleys on seed 42

- **Seen:** 2026-11-16, `water_audit --fine`. Tall river walls went from 31 to 58 on seed 42 against `main` (seeds 1 and 1337 fell or were level).
- **Known:** valleys now cross rivers, and the audit counts the waterfalls the water rule allows as walls.
- **Not known:** whether the new walls are waterfalls over valley walls or leaks. Not checked.

## The steppe change is not measured for speed

- **Seen:** 2026-11-16. Frame times, `bench` vertex counts and the cost of the foliage shader's second gust term were not measured for the valleys, flattening, exposure, flora and wind. The valley stage in map generation takes 0.30 to 0.51 s against the 0.3 s a new stage may add (`docs/world-plan.md`).
- **To check:** `profile` skill on a steppe seed against `main`; `OCHRE_STAGE_TIMES` for the stage.

## Only forests have birdsong

- **Seen:** 2026-11-16. The ambient birds are one recorded loop (`Loop::Forest`, `forest.ogg`), played in proportion to the trees round the camera, so steppe, tundra, desert and the other treeless biomes have none. Their starling flocks and goose skeins are sized by biome (`docs/climate.md`, Birds by biome), but they make no sound.
- **To fix:** a loop per biome, for example a lark or pipit over the steppe: a new `Loop` variant for each and a recording, CC0 if the licence rule stays (`assets/sounds/CREDITS.md`).

## Small basins outside valleys still drain by a ditch and leave a pit

- **Seen:** 2026-11-16, seed 1, round pits with a faint stepped line out of them in flat steppe, for example near x 1300, z 10100, and faint ladders with no valleys at all (`generate_with(seed, config, false)`, the same world with no valleys, which the viewer does not draw). The owner's complaint was dry riverbeds made of round hollows, right by the seed 1 spawn.
- **Known:** the ones by the spawn were the valleys' drain channels (a channel cut from each basin's floor out the way its water would leave, `hydrology::drain`, each cell just under the last, so on flat ground it runs far, as deep as the basin). Valleys now fill their shallow basins and leave the deep ones, see `docs/water.md`. The general run of hydrology still drains basins under 300 cells or 4 blocks deep that way.
- **Not known:** what a deep valley basin should be: they are left as closed pits (on seed 1, 14 tiny lakes in valleys remain, from a drained 6), and the seed 1 spawn's own valley ends in one. How common the rest are, and whether filling shallow basins (a block or two) outside valleys is safe for fjords, craters and cirques, whose tests lean on the channel.
- **To check:** count pits by `fill - height` after the last run, then try `fill_where` for basins under two blocks deep.

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

## LOD shores ignore the generated sand and the water cut, and trees root on shore sand

- **Seen:** 2026-11-17, reading the sculpted-ground change. Full-detail chunks give any bank within 2 blocks of water and a block of its level sand (`ColumnGrid::shore_top`, `columns.rs`) and cut the water surface along the sculpted ground (`push_shore`, `mesh.rs`). LOD nodes choose surface blocks through `column_block` with the column's own water top, and `trees.rs` does the same, and the water of a node uses the old block edges.
- **Known:** by reading the code, not by looking. The two paths pass the column's own water top where chunks pass the shore's.
- **Not known:** how visible the change is at the hand-over from chunk to node: a lake's bank may turn from sand to grass, and trees may stand on sand. Not checked.
- **To fix:** pass the shore top into the LOD columns and the tree ground rule, then bump `GENERATOR_VERSION`.

## Near-flat banks leave a saw-tooth waterline

- **Seen:** 2026-11-17, in play, lake edges where the sculpted ground lies just under the water. The waterline follows the ground's triangles, so it zigzags instead of running smooth.
- **Known:** it is the ground's shape meeting the shore cut, not the water mesh alone.
- **Not known:** the fix. Raising `SHORE_CUTS` (`mesh.rs`) gives a finer cut and was not tried against it, and smoothing the ground there would change the look.

## Rare holes in the sculpted ground, found in play

- **Seen:** 2026-11-17, in play, rarely. Where was not recorded. The tests (`test_sculpted_ground_leaves_no_hole`, `test_sculpted_lod_nodes_leave_no_hole`) find no open edge on the spots they mesh.
- **Known:** earlier holes were a winding flip, a chunk-edge float mismatch, a remesh range too short after a dig, and plants cut at a chunk's vertical edge, all fixed.
- **Not known:** the cause of these. Record the seed and the place when one is seen, and mesh it with `worldprobe <seed> render <x> <z> <half> <out.png>` (`docs/sculpted-ground.md`, Checking it).

## A chunk's edge lower than the LOD node beside it opens a gap from outside

- **Seen:** 2026-11-17, found while building the sculpted LODs, never seen in play. A full-detail chunk whose edge lies lower or further in than the node next to it leaves a gap, visible only from outside the full-detail area, where the eye is not.
- **Known:** skirts hang from the node's open edges to close the usual cases (`hang_skirts`), and the test allows open edges where a skirt ends.
- **Not known:** how often it shows from a mountain or the air.

## Rock's facets turn at every vertex in 8-block cells

- **Seen:** 2026-11-17, by design. The chisel turns its directions slowly over about 9 blocks, so a cell 8 blocks wide changes direction at each vertex and far rock reads as noise.
- **Not known:** whether anyone sees it at that distance. Not looked at.
