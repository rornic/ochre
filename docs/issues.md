# Known issues

Problems seen in the game or the tools that are not fixed yet. When one is fixed, delete its entry in the commit that fixes it, and say in the commit what the cause was. Add one when you find a problem you are not going to fix now.

An entry says where it was seen, what is wrong, and what is known and what is only a guess, so nobody mistakes a guess for the cause. The tectonic world's own defect table, with the water and LOD limits designed in, is in `docs/world-plan.md` (Open defects); this log is for everything else.

## Steppe on hills wider than a few kilometres stays hilly

- **Seen:** 2026-11-16, seed 3. The flattening of steppe ground (`valleys::flatten`) removes hills smaller than about 4 km. Seed 3's steppe stands on foothills, and a larger smoothing radius makes steep edges where the flattened zone ends.
- **Known:** median slope fell by about two thirds on the seed 3 windows with no valley cut, relief over 1.5 km by about a fifth (`docs/terrain-perf.md`).
- **Not known:** whether steppe should be kept off foothills in the first place, which is a change to `classify` and the census shares.

## Hummocks make chunk meshing a quarter dearer

- **Seen:** 2026-11-23, `bench` on seed 42 at (-8000, -10000), dry swamp with hummocks on 11% of samples (`docs/terrain-perf.md`, The steppe change measured).
- **Known:** with hummocks, chunk meshing takes 610 to 614 ms against 486 to 489 ms without, in three alternating pairs, while chunk vertices fall from 847,179 to 841,481. Generation and LOD are level.
- **Not known:** which part of the sculpted ground's meshing grows over tussocks 1 to 3 blocks high, and whether frame times in a swamp show it.

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
- **Since, 2026-11-23:** it does show. `OCHRE_RENDER_LOD=0, 1, 2 worldview 1 render 6052 520 24` (a lake bank at 0.3 slope) draws the waterline smooth from chunks, in steps of 2 blocks from level 1 and 4 from level 2, with a strip of bed up to a cell wide that chunks cover with water. On the flat shelf at 6962,-4064 the node's edge is a straight cell line up to about 3 blocks from the chunks' contour. A node's cells have no water cut because its water is whole cells (`water_tops` per cell) and `push_shore` needs the block-resolution reach and fresh-water levels that only chunks have.
- **Not known:** how visible it is in play. The hand-over lies 192 blocks out, where a 2-block step is about 8 px, and a shot that put a shore there under `full_detail_distance` of 1 and of 12 came out the same, so the game's own hand-over was not isolated.
- **To fix:** give nodes of cells 1 and 2 a water edge at block resolution, from the region's water tops, cut along the node's own ground, not a bigger `SHORE_CUTS`.
- **Left alone:** 2026-11-23, not worth a new meshing path until it shows in play.

## Near-flat banks leave a saw-tooth waterline

- **Seen:** 2026-11-17, in play, lake edges where the sculpted ground lies just under the water. The waterline follows the ground's triangles, so it zigzags instead of running smooth.
- **Known:** it is the ground's shape meeting the water, not the water mesh. The water plane is hidden wherever the ground is over it, so the visible edge is the ground's own contour at the level, however the shore is cut. Raising `SHORE_CUTS` from 4 to 16 (seed 1, `worldview render` at 6052,520, 7028,-3872, 6962,-4064) moves the edge by under a quarter block and changes the picture only where 4 cuts left a 1/4 block kink, for 3.4 times the water triangles (7,210 to 24,648 in the first view) and no change in `bench` vertex counts (the water is not counted there). At those three banks, whose ground lies 0 to 0.6 under the level for 3 to 40 blocks, the game showed only a mild wobble, not teeth. The teeth in the CPU render come from its flat colour per triangle.
- **Not known:** where it shows worst in play. A fix has to change either the ground near the water (its look) or lift the water at its edge to meet the ground, a meniscus a tenth of a block high, which is a look to judge by eye.
- **Left alone:** 2026-11-23, until it shows in play again: both fixes change the shore's look.

## Sea shores are cut along block edges, not along the ground

- **Seen:** 2026-11-23, on seed 1 at 5025,-432 (a flat shelf 0.55 under the sea, rising 2 blocks in one) in the game and in `worldview render`. The sea's edge runs in straight block-aligned steps and square corners, where a lake's follows the ground.
- **Known:** `push_shore` and the reach over the bank apply only to fresh water (`top_block` needs `water_block_level`, which only lakes and rivers have), so the sea's top faces end at the block edge.
- **Not known:** how visible it is from the beach at eye height, or whether the shelf is typical.

## A chunk's edge higher than the LOD node beside it opens a gap from outside

- **Seen:** 2026-11-17, found while building the sculpted LODs, never seen in play, first written as the chunk's edge lying lower or further in. Measured 2026-11-23 with `OCHRE_RENDER_GAPS` (`docs/sculpted-ground.md`, Checking it): it is the chunk's edge standing higher.
- **Known:** on seed 1's mountains (10816, -6784), views from four azimuths, the eye seeing through 0 to 1,532 pixels of 337,000 to 831,000 along the seam between a chunk and a node, nearly all of them on the side of the area the eye is outside of. In the samples the node's edge vertex, pinned on the side's plane, stands 1.2 blocks under the chunk's edge (`LOD_SINK` is 0.3 of it, the rest is its coarse cells), and nothing covers the slit between them: a skirt hangs down from the node's edge, never up. A sight line from outside passes between them into the ground and out through its open underside. From inside, the node's surface lies behind the chunk's edge and nothing shows. The counts are about the same with and without stacked nodes agreeing (`docs/sculpted-ground.md`).
- **Not known:** whether anything looks at the seam from outside in the game: shadow maps do, from the sun, and were not checked. Seed 4's seracs show 239 to 717 as well, among many more that are not seams, see the next entry.
- **To fix:** a choice about how the seam looks, not a measurement. A collar rising from the node's edge, facing outward, would close it from outside and show nothing from inside, but where the chunk's edge stands lower than it, it is a fin above the ground seen from outside, and it casts a shadow. The node's edge raised to the real ground would end the slit, and make a step up of a block or two at the ring seen from inside, which `LOD_SINK` is there to avoid. Rerun `OCHRE_RENDER_GAPS` on the views above to judge either.

## LOD nodes show thin slits against one another and within one node

- **Seen:** 2026-11-23, with `OCHRE_RENDER_GAPS` (`docs/sculpted-ground.md`, Checking it), looking for the entry above.
- **Known:** on seed 1's mountains a node against another leaves 4 to 193 pixels of 337,000 to 831,000 where the all-chunk view shows ground and the nodes show none, and one node against itself up to 120; on seed 4's seracs 15 to 3,049 and 153 to 901. Some are on the far half of the area from the eye, which an eye inside it would see. Not all are seams: ice slabs thinner than a coarse cell are in the chunks and not in the nodes.
- **Not known:** how many are skirts ending at a node's corner and how many are what coarse cells can't hold. No sample was followed to its cause.

## Stacked LOD nodes still z-fight a little on their shared row

- **Seen:** 2026-11-19, with `OCHRE_RENDER_ZFIGHT` in the worldview renderer (`docs/sculpted-ground.md`, Checking it), after the fix for skirts flickering on distant cliffs: skirts lay in a node's side plane with the neighbour's skirts and its walls across the side, and the row past a node's bottom lay over the top row of the node below and hung the same skirts.
- **Known:** 49 to 117 pixels of about a million drawn still z-fight at 0.001 blocks on seed 1 (10816, -6784) and seed 4 (-6076, 5236), levels 1 to 3, against 1,748 to 22,854 before. The samples are the row past the bottom near its top, where it meets the row below, and the flat crossings both nodes draw in that one cell row, which can't lean.
- **Not known:** whether the rest shows in the game. The renderer is orthographic with no depth precision limit, so the counts are geometry, not the GPU's depth test. Level n beside level n + 1, and a chunk beside a node at the hand-over, were not measured.
- **To fix:** make the two nodes' vertices on their shared row agree (`docs/sculpted-ground.md`, Open).
- **Tried, 2026-11-23:** reading 13 rows of real ground past a node's top and bottom makes the shared row agree (332 of 332 vertices) and cuts the counts from 13 to 88 pixels to 1 to 8, but LOD meshing in `bench` went from 175 to 259 ms and the level 1 ring from 1.8 to 2.3 s, measured with other builds running. Held: too dear for a fight not noticed in play. A cheaper way would share only the vertices of the row itself.
- **Measured quiet, 2026-11-23:** `bench` on that commit (5005be3) against its parent (abcdadd), four alternating pairs on 16 cores with nothing else running: LOD meshing 170.6 to 172.0 ms against 242.4 to 243.3 ms (1.42 times, each level 29 to 52% dearer), the level 1 ring 1.7 s against 2.1 s (one run 3.2 s). Chunk meshing and vertices are unchanged, LOD vertices 119,595 to 119,558. The loaded figures above hold.

## Rock's facets turn at every vertex in 8-block cells

- **Seen:** 2026-11-17, by design. The chisel turns its directions slowly over about 9 blocks, so a cell 8 blocks wide changes direction at each vertex and far rock reads as noise.
- **Not known:** whether anyone sees it at that distance. Not looked at.

## Inselbergs do not read as bare rock domes

- **Seen:** 2026-11-23, from the ground at eye height (`OCHRE_CLEAR_SKY`, noon). Seed 6 at -6096,4368 (savanna): ground 65 to 73 over a stretch 650 blocks wide against 45 to 50 round it, flat on top and cut by valley cliff bands, so it shows as a low plateau with a rock slope and no dome. Seed 3 at 3600,21808 (forest): a round dome 70 blocks over 47, but green with trees on it and standing in a lake.
- **Known:** by `info` along a line through each site; the docs say a bare dome 20 to 45 blocks high on dry, warm sedimentary lowland (`landforms.md`). Both sites are forest or savanna, and the biome is set after the dome is laid.
- **Not known:** whether the valleys (seed 6) and the later hydrology (seed 3) reshape the dome, or whether the dome's rock is simply not picked out by the block rules. Neither was tested.
- **To check:** generating with the valley stages left out, and a block rule that shows bare rock on a dome.

## Tower karst in a desert reads as sand-covered humps

- **Seen:** 2026-11-23, seed 6 at 7500,14300 (desert), from 700 blocks out and from above. The field shows as rounded rock heaps under sand about 20 blocks high, not steep towers 30 to 70 high; hoodoo-like mushroom rocks stand beside it at 6400,14300.
- **Known:** the site list puts the field's towers 30 to 69 high (r 1.5 to 4 cells). Climate then made the place desert, which is not the wet lowland the docs name.
- **Not known:** whether the dunes and yardangs, or erosion, bury the towers, or whether wet-country karst reads right. No wet field was visited.

## A cirque's bowl does not read, only banded cliffs

- **Seen:** 2026-11-23, seed 3 at -5360,8304 (cirque 6 cells, 73 high), from 500 blocks out and 120 up. The walls show as tall stepped cliffs of blue ice over rust-coloured rock, with no bowl floor to read between them.
- **Known:** the cliff faces are present and high; seed 3's mountains are snow-covered and glacier biome at that height, so the whole view is white and blue.
- **Not known:** whether the floor is there but hidden by the haze and the angle, or whether the cliff pass makes a headwall of any steep ground here.

## Steep glacier faces are saturated blue with one-block column stripes

- **Seen:** 2026-11-23, seed 3 at -5360,7800 and -4000,9000, facing the walls; also at the foot of the taiga cliff at -6000,9000.
- **Known:** the ice block's colour is a strong blue against the snow, and steep faces show vertical stripes one block wide, stepping in height down the face.
- **Not known:** whether the stripes are the cliff pass's 3D noise or the greedy mesher's face merging, or whether the blue is meant.
