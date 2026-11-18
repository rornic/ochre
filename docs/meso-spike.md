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

## 2. The mesher: answered for full detail, open for LOD hand-over

`worldprobe <seed> render <x> <z> <half> <out> [azimuth]` prints, for the area it draws, the share of triangle area by slope (walkable over 0.4 of up, steep, wall under 0.1) and the open edges in the middle 80% of the area. The count leaves out water (an open sheet along its shore) and edges beside tree blocks (trunks sink up to 2 blocks into the ground and are drawn open by their own mesher), and splits off those lying in a chunk plane. `OCHRE_RENDER_EDGES=1` lists them. Run on seed 1 around the steppe peak, (13408, -4704), half 400, `OCHRE_RENDER_FLAT=1`:

| Detail | Triangles | Walkable / steep / wall by area | Open edges in the ground | Of them in a node's or chunk's plane |
| --- | --- | --- | --- | --- |
| Full chunks | 4.4 M | 57% / 16% / 27% | 0 | 0 |
| LOD 3 (8-block cells) | 74 k | 37% / 8% / 56% | 4,961 | 3,586 |
| LOD 4 (16-block cells) | 22 k | 38% / 7% / 55% | 942 | 642 |
| LOD 5 (32-block cells) | 5 k | 40% / 5% / 56% | 154 | 137 |

- **Full-detail ground is closed on a sheer peak.** Its walls, 50 blocks and more, are drawn as flat faces with no open edge, and the same holds at (11393, -20193), where a hole was seen earlier. This was first misread: 3,724 of 3,796 edges near the peak's lake were the lake's own surface, and 160 more were 20 trunk bases.
- **The surface-net construction explains it.** Each crossing between a solid and an open block makes a quad of the four cells round it, and each of those cells holds a vertex, so the interior is closed whatever the faces do to the vertices after (relax, spike cap, wobble, chisel only move them). The chunk edges are exact by the reach rule in `docs/sculpted-ground.md`. So pulling vertices off their cells, the usual cause of surface-net and dual-contouring cracks, cannot open a hole here, only fold or thin one.
- **LOD nodes are open by design.** They stitch to a neighbour with skirts, not shared vertices, so their sides, tops and bottoms are open, and the counts above are mostly that. What is left (LOD 3: 1,375, LOD 4: 300, LOD 5: 17) is where skirts end and the node's top and bottom, per the docs, which was not separated further.
- **So where are the holes the owner saw?** Not in full-detail ground. The likely place is the hand-over between a chunk and a node on a sheer face, which `docs/sculpted-ground.md` lists as open: a node's coarse surface can stand lower or further in than the chunk's, and a skirt only hangs down. That is a guess, not checked in the game.

Not yet done: the walkable-fraction histogram over several seeds, an ALPINE valley wall, and a look at the hand-over on a sheer face.

## Decision rule so far

For slice 3: aim risers at 2.2 blocks or more with no ledge of 0.2 or more on faces over 66 degrees, and check by this test. The mesher draws sheer faces without holes at full detail, so steeper faces are not blocked by it. The place to check before steepening is the chunk-to-node hand-over on a sheer face.
