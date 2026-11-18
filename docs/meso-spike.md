# Meso spike: what a player can stand on, and what the mesher holds

Part of slice 1 of `docs/designs/meso-scale-geography.md`. It asks two questions before any steep-mountain design: where does the player find footing on a steep face, and what face can the ground mesh draw. The first is answered, the second partly.

## 1. Footing: answered

`contact::tests::spike_which_ledge_heights_a_jumping_player_climbs` (in `ochre-gameplay`) builds a staircase face (vertical risers, flat treads) and simulates a player walking at it at 4 blocks a second and jumping at every chance, with the game's `JUMP_SPEED` 9, `GRAVITY` 32 and `STEP_HEIGHT` 1, through the real `Walker`. Run it with `cargo test --release -p ochre-gameplay -- spike --nocapture`.

| Measure | Result |
| --- | --- |
| Tallest ledge a player climbs | 2.15 blocks. A ledge of 2.2 or more stops them at the foot (a jump rises about 1.27 and a step is 1, so about 2.27 in theory) |
| Climbing a staircase of such ledges | no limit: six ledges of 2.0 are climbed to the top, whatever the tread |
| Shallowest tread that gives footing | 0.2 blocks deep. At 0.1 or 0.05 the player cannot stand on it and cannot climb |

So the **unclimbable face height is 2.2 blocks between footholds**, and a "tiny ledge" is anything 0.2 blocks or more deep. A face that is sheer for 2.2 blocks or more with no ledge of at least 0.2 blocks anywhere on it cannot be climbed by walking and jumping. The 0.2 comes from the body's footprint (five ground samples at 0.7 of the 0.3 radius), so it is a property of `Walker`, not of the terrain.

What this means for the mountain design:

- **Riser height is the lever, not tread.** To be jump-proof, every riser on a steep terraced face needs to be 2.2 blocks or more, and a series of risers under 2.15 is a staircase however narrow the treads.
- **The existing bed risers are probably already tall enough** (a hard bed's face is of order 16 blocks or more by `geology.rs`), so the climbable cliffs the owner has met are more likely mesh softening and cliff noise producing sub-2-block ledges than the beds themselves. That is a guess, to check in the mesher half.
- **No contact change is needed to "slide off".** A face over 66 degrees that is also over 2.2 blocks tall already walls the player out. What lets them climb is ledges, so the fix is in the ground shape, as the design review said.

## 2. The mesher: what the holes were

**Found 2026-11-18: the holes the owner saw were faces no chunk drew.** A sculpted face belonged to the chunk on its low side, and the chunk loader never meshed an all-air chunk, so a face toward -x, -y or -z lying on a chunk boundary, with a wholly empty chunk on its air side, was never drawn. Big sheer cliffs have exactly that: 16 blocks of open air against a west- or north-facing wall. The hole's edges were chunk planes, a dig filled it (a remesh meshes every chunk), and offline renders never showed it, as they mesh every chunk. Reproduced in the game from the owner's F1 position, (13288, 849.9, -4526) facing east, `OCHRE_FLY=1 OCHRE_LOOK=1,0,0 OCHRE_SHOT=35,out.png`: a hole before the fix, a whole wall after. The fix and its tests are in `docs/sculpted-ground.md` (Chunk edges).

Wrong turns, kept as a record of what was ruled out: the earlier sections below blamed folded triangles, the LOD hand-over and the air layer behind the sandstone skin. None was the cause. Folded triangles were real and are now held at zero, but fixing them did not remove a hole. Mesh packing, group merging, the loader's visibility and the camera's near plane were all checked and cleared.

`worldprobe <seed> render <x> <z> <half> <out> [azimuth]` prints slope bands and checks the ground mesh. It leaves out water and edges beside tree blocks (trunks sink up to 2 blocks and are drawn open). `OCHRE_RENDER_EDGES=1` lists open edges, `OCHRE_RENDER_CULL=out` draws only triangles whose winding faces the camera as a backface cull would, and `OCHRE_RENDER_PITCH=<degrees>` sets the camera pitch (35 by default), so a near-level view can be diffed against the uncullled one.

**What the owner saw is not an open edge.** The owner confirmed the holes are in full-detail ground. A first reading of this section said the mesh was closed, because it counted open edges only, and a folded or inverted triangle has a neighbour on every edge. The checks that find it:

| Check | Seed 1 peak, (13408, -4704), half 400 | Seed 4 marble arch, (11393, -20193), half 40 |
| --- | --- | --- |
| Open edges in the ground | 0 | 0 |
| Edges wound the same way by exactly two triangles | 14, all beside trees | 0 |
| Edges with three or more triangles | 3,948 | 0 |
| Triangles wound against their own vertices' normals | 15,347 of 4.3 M (0.1% of area), 99% in rock tiles | 23 of 26,586, all in the two chunks that hold the reported hole, (11376, 112, -20192) and (11392, 112, -20192) |

- **At the arch the only anomaly in an 80-block area is 23 folded triangles, in the chunks holding the hole.** Culling them in a near-level render (pitch 4, half 14) removes a sliver of rock at the arch's base. It does not reproduce the sky window in the pillar from the owner's screenshot, so the arch hole is consistent with this cause and not proven to be it.
- **The folds are rock's.** Measured with a temporary count inside `nets.rs` (since removed) against the outward direction the placement itself reads (the gradient of the blurred blocks): 41,378 of 1.57 M triangles over the peak at half 250 (2.6%) wind more than about 100 degrees against it, 41,306 in rock and 72 in soft ground. That test also flags thin rock features where the direction is ambiguous, so 41,000 is an upper bound.
- **They exist before the chisel.** 15,312 before it and 17,348 after, so the chisel adds about 13% and is not the origin.
- **Rock has no relax and no wobble, so placement and the spike cap are what move its vertices before the chisel.** Clamping the plane solve inside its cell (`PAST` 0.0, 0.3 and 1.0) changes nothing. Removing the spike cap doubles the folds (0.43% to 0.81% of triangles), so the cap helps. That points at the plane solve itself (planes from the blurred gradient, pulled by `PULL` 0.05 toward the crossings' mean) on cells with few solid corners or seven of eight, which are most of the bad triangles, mostly on near-flat tops and not sheer walls.
- **A render box shows false holes.** The renderer cuts the world at the area's sides, so the inside of the mountain's far wall shows through the open cut and looks like missing or inverted slabs when culled. Judge only near the middle.

**Fix applied: vertices held inside their cells across each crossing axis, quads re-split from final positions** (`docs/sculpted-ground.md`). By the exact definition (a triangle winds against the solid-to-air direction of its quad) folds went, measured over the peak at half 250 and the arch at half 40:

| Stage | Peak | Arch |
| --- | --- | --- |
| Before the chisel | 3,458 (3,178 rock) | 34 |
| After the chisel | 10,478 | 39 |
| Final | 0 | 0 |

`test_sculpted_ground_never_folds_a_triangle` holds it at zero. Costs: chunk meshing 480.8 to 490.0 ms (+2%) and LOD meshing 133 to 136 ms in `bench`, with the vertex count 1 higher for chunks and 8 for nodes, as the diagonal decides which triangles share a vertex.

**Not settled by the fix.** `OCHRE_RENDER_INVERTED=1` steps 0.75 blocks out along each triangle's normal and in against it, and counts triangles with solid outside and air inside. Over the peak it went from 6,966 (0.61%) to 4,433 (0.39%) and at the arch from 15 to 9. The remainder is 0.4 to 0.6% of rock tiles and 0.01% of steppe grass, so it is rock-specific and not the corner-cutting any smoothed surface shows. These triangles do face along their crossing, so they are not folded by the exact definition. A guess only: chiselled 45 degree facets tilt a triangle so far that the step lands in the neighbouring step. A culled render diff was tried and dropped, as the render box's open cut shows back faces for other reasons.

**Seed 1 peak, the hole at (13374, 915, -4672), found by the owner in play after the fix.** The owner reads this as the block under the crosshair (F1's `looking at`), standing on the face's top. The game's real loading shows the surface is there:

- `OCHRE_PROBE=13373.5,915,-4672 lod_sim -- 0 1` (with `OCHRE_SEED=1 OCHRE_SPAWN=13374,-4672,6`) decodes the packed group meshes the game builds. The group holding the block has 4,200 triangles, 42 within 3 blocks of it and the nearest 0.2 away, and all 12 groups round it are visible. `lod_sim` reports no surface chunk without something drawing it.
- The ignored test `diag_group_pipeline_keeps_every_triangle` runs the game's merge, cutout split and 16-bit pack over the groups round the spot and finds every triangle kept, to rounding.
- `worldprobe 1 render 13374 -4672 32` finds no open edge and no exact fold. The blocks there are a thin sandstone skin with a 1 to 2 block air layer behind it before the compacted core.

So it is not a missing patch of mesh. It is either a face drawn but culled or discarded in the render, or terrain, and a game frame with the F1 readout (position and looked-at block) is needed to say which. A flythrough of the spot from a fixed camera shows the face solid, with small dark dimples.

Not yet done: a close render of the arch pillar from the screenshot's angle, which cell patterns fold and why (the plane solve on thin or 7-corner cells is the suspect), the LOD hand-over on a sheer face (still a possible second cause), the walkable-fraction histogram over several seeds, and an ALPINE valley wall.

## Decision rule so far

For slice 3: aim risers at 2.2 blocks or more with no ledge of 0.2 or more on faces over 66 degrees, and check by this test. Folds are now held at zero by construction, so steeper rock cannot fold triangles, only change how the chisel's facets look.
