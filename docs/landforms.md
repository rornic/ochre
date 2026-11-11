# Landforms

How the ground gets its shapes beyond what tectonics and erosion make. Water is `docs/water.md`; climate and biomes are `docs/climate.md`. The one rule: landforms read the ground, never the climate or biome output, so the water can be routed over the ground it will really stand on (`landform.rs`, `source.rs::landform_ground`).

## The pipeline

`worldmap::generate`: tectonics, geology, hydrology, erosion, the floor, then in order fjords, cones, inselbergs, cold uplands, tower karst and cirques (`shapes.rs`), the mountain passes, the tableland terraces (`landform::terrace`), scatter, the last hydrology run, landform, climate, roughness and labels. Everything a shape or terrace does to `map.height` is therefore in place before the last water is routed.

## Map-scale shapes (`shapes.rs`)

Counts come from `LandformKnobs::from_seed`, not stored in the map, and the config weights them; no shape reads climate output.

- **Fjords:** a share of the mouths a mountainous coast has room for. A trough runs inland 40 to 120 cells, its floor 32 to 144 blocks under the sea, its walls rising 40 blocks a cell, ragged, so the sea fills it.
- **Cones:** a share of the sites volcanic ground has room for, 90 cells apart. Radius 16 to 36 cells, height 150 to 400 blocks, a crater a quarter as wide and a quarter of the height deep under the rim.
- **Inselbergs:** up to 8 rounded domes, 20 to 45 blocks high and 4 to 10 cells in radius, on dry and warm sedimentary lowland.
- **Cold uplands:** occasional flat-topped, steep-sided landmarks on cold lowland, 55 to 95 blocks high and 4 to 7 map cells in radius. Sites use regional warmth derived from config and height, before climate classification; their tops are broad enough for building.
- **Tower karst:** up to 4 fields of 10 to 40 steep towers, 30 to 70 blocks high, on wet sedimentary lowland.
- **Cirques:** up to 6 bowls 40 to 100 blocks deep with a headwall, cut into cold mountains on the cold half of the config's temperature gradient.

## Tableland and terraces

Sedimentary ground 40 to 360 blocks over the sea that is not mountain is tableland in patches covering about the seed's `tableland_share` (`landform::tableland`). Its height is stepped into the rock beds (`WorldMap::terraced`: hard beds as benches with faces, clay as slopes) at the map level before water is routed, and again at block level, which sharpens the bicubic ramp across a face into a cliff. A river over tableland runs in a gorge: a wide flat floor and a steep wall.

## Cliffs

Ground steeper than gradient 1 is cut and built up into cliffs by 3D noise (changing 0.7 as fast with height round the ground as along it, since at full speed it turns the surface over several times in a column and leaves stray blocks), up to 9 blocks, up to 18 where the seed's `cliff_boost` reaches (mountain and tableland). There are no undercuts or overhangs cut into the mass beyond what that noise leaves (see `terrain-perf.md`).

## Relief features (`ochre-base/src/features.rs`)

Each is a pure function of the seed and position returning blocks to add to the height, at most 18, zero once a sample cell is 16 blocks wide, and laid by `WorldTerrain` after containment only where no water is near:

| Biome | Features |
| --- | --- |
| Glacier | crevasses (2 to 5 wide, 8 to 18 deep, with bridges, in the same fixed frame), seracs |
| Tundra | pingos, patterned ground, occasional broad low hills (10 to 18 high, 44 to 130 across) |
| Taiga | drumlins along the config's gradient angle |
| Desert | yardangs along the wind |
| Savanna | kopjes (rounded rock heaps 6 to 14 high, bare stone where the heap stands 4 or more blocks over the plain, with grass round the foot) |
| Steppe | gullies (swales 2 to 6 deep, about 10 across) in a frame fixed by the world's ridge angle |
| Swamp | hummocks |

`worldprobe features <seed> <x> <z> <half>` counts them.

## Not built

Gneiss and andesite, objects (erratics, termite mounds), and LOD fidelity for any of this: see `world-plan.md`.

## Placed objects: boulders and termite mounds

`generate/rocks.rs` sets single objects by a hash of position on a grid of 24-block cells, one at most in each, and builds them block by block with the chunk, after the trees (a trunk keeps its place). A glacial erratic is a half-sunk, squashed ball of the rock the geology has at its foot (`rock_column` at the ground it sits on, so marble on marble country, and not a stone of its own) 1.5 to 5.5 blocks in radius with a nibbled rim, on tundra (up to 90% of cells) and taiga (4%), the share scaled smoothly over fields 96 blocks across by a factor from 0.1 to 2 (`field` in `rocks.rs`), so they come in fields, nearly every cell holding one, with open ground between. A quarter of the tundra's are outcrops instead: elongated low ledges 5.5 to 11 blocks in radius and a third as tall as wide (`OUTCROP_*`), on ground that may vary 6 blocks under them, which is why `ROCK_REACH` is 12, on ground up to gradient 0.9 and within 3 blocks of height under it, sunk to the low side. A termite mound is a cone of dirt 3 to 7 blocks high and 2.5 to 4 in radius on savanna (7.5% of cells), on level ground (gradient 0.4, within 1 block). Neither is placed on a mountain or volcanic modifier, within 3 blocks of water or under 3 blocks over the sea. In LOD meshes they are drawn as cells wherever a cell is 4 blocks across or less (`ROCK_MAX_CELL`), each cell a rock if its middle is, past which one is under a pixel.
