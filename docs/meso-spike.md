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

## 2. The mesher: partly answered

`worldprobe <seed> render <x> <z> <half> <out> [azimuth]` now prints, for the area it draws, the share of triangle area by slope (walkable over 0.4 of up, steep, wall under 0.1) and the open edges in the middle 80% of the area, with where they cluster (32-block bins). Run on seed 1 around the steppe peak the owner found, (13408, -4704), half 400, `OCHRE_RENDER_FLAT=1`:

| Detail | Triangles | Walkable / steep / wall by area | Open edges |
| --- | --- | --- | --- |
| Full chunks | 4,404,006 | 58.0% / 15.7% / 26.3% | 5,677 |
| LOD 3 (8-block cells) | 84,216 | 37.9% / 6.9% / 55.3% | 5,007 |
| LOD 4 (16-block cells) | 23,074 | 40.0% / 6.5% / 53.4% | 956 |
| LOD 5 (32-block cells) | 5,190 | 41.2% / 4.4% / 54.4% | 163 |

- **The mesher holds tall steep faces.** In the renders the peak's large near-vertical faces appear as big flat walls at full detail and at LOD 4, and coarser levels simplify them into planes. The larger wall share at the LODs includes the skirts they hang, and the LOD open edges include where skirts end (see `docs/sculpted-ground.md`), so those counts are not holes.
- **The generator makes genuinely sheer faces.** A cross-section of the blocks at z -4930 (`worldprobe 1 slice 13120 -4930 x 120 380 520`) shows ground rising almost vertically for more than 50 blocks (y 471 to 520).
- **The full-detail open edges are not scattered, they sit along such a wall.** About 3,600 of the 5,677 are in one area: x 13088 to 13152, y 448 to 480, z -4992 to -4864, a patch about 128 blocks across, roughly 300 blocks from where the owner started. That is a hole in the mesh on a sheer face, found by coordinates for the first time. It is the same defect the owner reported on seed 1's cliffs (`docs/issues.md`), now located.
- **So the answer so far is: it draws sheer faces, but it does not reliably seal them.** Steeper terraces and deeper valley walls would multiply faces like this one, so the holes need explaining before slice 3 builds on them.

Not yet done: the walkable-fraction histogram over several seeds, an ALPINE valley wall, and what in the sculpted mesher (the rule that no vertex stands far over its neighbours' mean, the chisel, or the chunk-edge handling) opens the wall.

## Decision rule so far

For slice 3: aim risers at 2.2 blocks or more with no ledge of 0.2 or more on faces over 66 degrees, and check by this test. The mesher draws sheer faces but leaves open edges along some, so the cause of the seed 1 hole at (13088 to 13152, 448 to 480, -4992 to -4864) has to be found before steeper faces are made.
