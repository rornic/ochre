# Landforms

How the ground gets its shapes beyond what tectonics and erosion make. Water is `docs/water.md`; climate and biomes are `docs/climate.md`. The one rule: landforms read the ground, never the climate or biome output, so the water can be routed over the ground it will really stand on (`landform.rs`, `source.rs::landform_ground`).

## The pipeline

`worldmap::generate`: tectonics, geology, hydrology, erosion, the floor, then in order fjords, cones, inselbergs, tower karst and cirques (`shapes.rs`), the mountain passes, the tableland terraces (`landform::terrace`), scatter, the last hydrology run, landform, climate, roughness and labels. Everything a shape or terrace does to `map.height` is therefore in place before the last water is routed.

## Map-scale shapes (`shapes.rs`)

Counts come from `LandformKnobs::from_seed`, not stored in the map, and the config weights them; no shape reads climate output.

- **Fjords:** a share of the mouths a mountainous coast has room for. A trough runs inland 40 to 120 cells, its floor 32 to 144 blocks under the sea, its walls rising 40 blocks a cell, ragged, so the sea fills it.
- **Cones:** a share of the sites volcanic ground has room for, 90 cells apart. Radius 16 to 36 cells, height 150 to 400 blocks, a crater a quarter as wide and a quarter of the height deep under the rim.
- **Inselbergs:** up to 8 rounded domes, 20 to 45 blocks high and 4 to 10 cells in radius, on dry and warm sedimentary lowland.
- **Tower karst:** up to 4 fields of 10 to 40 steep towers, 30 to 70 blocks high, on wet sedimentary lowland.
- **Cirques:** up to 6 bowls 40 to 100 blocks deep with a headwall, cut into cold mountains on the cold half of the config's temperature gradient.

## Tableland and terraces

Sedimentary ground 40 to 360 blocks over the sea that is not mountain is tableland in patches covering about the seed's `tableland_share` (`landform::tableland`). Its height is stepped into the rock beds (`WorldMap::terraced`: hard beds as benches with faces, clay as slopes) at the map level before water is routed, and again at block level, which sharpens the bicubic ramp across a face into a cliff. A river over tableland runs in a gorge: a wide flat floor and a steep wall.

## Cliffs, undercuts and arches

Ground steeper than gradient 1 is cut and built up into cliffs by 3D noise (changing 0.7 as fast with height round the ground as along it, since at full speed it turns the surface over several times in a column and leaves stray blocks), up to 9 blocks, up to 18 where the seed's `cliff_boost` reaches (mountain and tableland). Near a face on that ground, undercuts cut the mass under the cap (`generate/undercut.rs`). The ground is smoothed over each chunk column and 27 blocks round it, and a block goes when open air in that smooth ground lies within a depth of it along any of eight directions. The depth is full up the lower face, falls to nothing under a cap of at least 6 blocks (a tenth of the face if that is more), and comes from slowly changing noise and the ground's steepness, so only steep ground is cut and what is above the cut overhangs. Smooth ground and a depth that changes slowly leave no stray blocks, and a rib thinner than twice the depth is cut from both sides, which leaves an arch. Where a face is cut the cliff pass's roughness gives way to the cut, which is what leaves a smooth face and top, and ground within 2 blocks of the smooth ground is not counted as open air. Cuts never touch ground near water or within 10 blocks of the sea, and only chunk generation sees them.

## Relief features (`ochre-base/src/features.rs`)

Each is a pure function of the seed and position returning blocks to add to the height, at most 18, zero once a sample cell is 16 blocks wide, and laid by `WorldTerrain` after containment only where no water is near:

| Biome | Features |
| --- | --- |
| Glacier | crevasses (2 to 5 wide, 8 to 18 deep, with bridges, in the same fixed frame), seracs |
| Tundra | pingos, patterned ground |
| Taiga | drumlins along the config's gradient angle |
| Desert | yardangs along the wind |
| Savanna | kopjes (rounded rock heaps 6 to 14 high, bare stone where the heap stands 4 or more blocks over the plain, with grass round the foot) |
| Steppe | gullies (swales 2 to 6 deep, about 10 across) in a frame fixed by the world's ridge angle |
| Swamp | hummocks |

`worldprobe features <seed> <x> <z> <half>` counts them.

## Not built

Gneiss and andesite, objects (erratics, termite mounds), and LOD fidelity for any of this: see `world-plan.md`.

## Placed objects: boulders and termite mounds

`generate/rocks.rs` sets single objects by a hash of position on a grid of 48-block cells, one at most in each, and builds them block by block with the chunk, after the trees (a trunk keeps its place). A glacial erratic is a half-sunk, squashed ball of stone 2 to 5 blocks in radius with a nibbled rim, on tundra (a fifth of cells) and taiga (a sixth), on ground up to gradient 0.9 and within 3 blocks of height under it, sunk to the low side. A termite mound is a cone of dirt 3 to 7 blocks high and 2.5 to 4 in radius on savanna (3 cells in 10), on level ground (gradient 0.4, within 1 block). Neither is placed on a mountain or volcanic modifier, within 3 blocks of water or under 3 blocks over the sea. In LOD meshes they are drawn as cells wherever a cell is 4 blocks across or less (`ROCK_MAX_CELL`), each cell a rock if its middle is, past which one is under a pixel.
