# Meso spike: what a player can stand on, and what the mesher holds

Part of slice 1 of `docs/designs/meso-scale-geography.md`. It asks two questions before any steep-mountain design: where does the player find footing on a steep face, and what face can the ground mesh draw. Only the first is answered so far.

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

## 2. The mesher: not yet answered

Not started: whether the ground mesh can hold a near-vertical face of 20 to 150 blocks at LOD 3, 4 and 5, or softens it to a slope, and whether it opens holes in it. The holes the owner found on seed 1's steep steppe cliffs near (13408, -4704) (`docs/issues.md`) are the first real case. Planned: render that spot and an ALPINE valley wall with `worldprobe render` at `OCHRE_RENDER_LOD` 3, 4 and 5 with `OCHRE_RENDER_FLAT=1`, and a walkable-fraction histogram by landform over several seeds.

## Decision rule so far

For slice 3: aim risers at 2.2 blocks or more with no ledge of 0.2 or more on faces over 66 degrees, and check by this test. Whether the mesher can hold such faces decides how.
