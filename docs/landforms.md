# Landforms

How the ground gets its shapes beyond what tectonics and erosion make. Water is `docs/water.md`; climate and biomes are `docs/climate.md`. The one rule: landforms read the ground, never the live climate or biome output, so the water can be routed over the ground it will really stand on (`landform.rs`, `source.rs::landform_ground`). Valleys read a snapshot of warmth and moisture taken once before they are cut, as cold uplands already read warmth, and keep it in `map.valley`, so the block-level ground reads that stored layer and never the climate itself.

## The pipeline

`worldmap::generate`: tectonics, geology, hydrology, erosion, the floor, then in order fjords, cones, inselbergs, cold uplands, tower karst and cirques (`shapes.rs`), the steppe and tundra landmarks (`meso/landmarks.rs`), the mountain passes, the tableland terraces (`landform::terrace`), the valleys (`valleys.rs`), scatter, the last hydrology run, landform, climate, roughness and labels. Everything a shape or terrace does to `map.height` is therefore in place before the last water is routed.

## Map-scale shapes (`shapes.rs`)

Counts come from `LandformKnobs::from_seed`, not stored in the map, and the config weights them; no shape reads climate output.

- **Fjords:** a share of the mouths a mountainous coast has room for. A trough runs inland 40 to 120 cells, its floor 32 to 144 blocks under the sea, its walls rising 40 blocks a cell, ragged, so the sea fills it.
- **Cones:** a share of the sites volcanic ground has room for, 90 cells apart. Radius 16 to 36 cells, height 150 to 400 blocks, a crater a quarter as wide and a quarter of the height deep under the rim.
- **Inselbergs:** up to 8 rounded domes, 20 to 45 blocks high and 4 to 10 cells in radius, on dry and warm sedimentary lowland.
- **Cold uplands:** occasional flat-topped, steep-sided landmarks on cold lowland, 55 to 95 blocks high and 4 to 7 map cells in radius. Sites use regional warmth derived from config and height, before climate classification; their tops are broad enough for building.
- **Landmarks:** on steppe and tundra ground, flat and between 20 and 700 blocks over the sea, one landmark every 70 to 110 cells (a seed's draw), each a row of a rule table: a butte (140 to 260 blocks high, 2.5 to 4 cells in radius), a massif (220 to 380 high, 6 to 11 cells), a ridge three times as long as wide (150 to 260 high), or a bowl (50 to 120 deep, sited but not counted among the landmarks a ridge shows, whose basin keeps its lake however small, `hydrology::run_keeping`; most hold one, a few dry out). Steppe is told by the same warmth and moisture the flattening reads, tundra by the warmth band cold uplands read. The flattening leaves them alone, and the passes are carved round them. Sized from `docs/meso-slice1-decision.md`. Both keep about 95% or more of the height laid through the valleys, as the flattening and the valley siting leave their sites alone; an alpine valley through a nearby range can still take a little.
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
| Tundra | pingos, patterned ground, occasional broad low hills (10 to 18 high, 44 to 130 across, hundreds of blocks apart, in provinces 1600 blocks across of which two in five hold none) |
| Taiga | drumlins along the config's gradient angle |
| Desert | yardangs along the wind |
| Savanna | kopjes (rounded rock heaps 6 to 14 high, bare stone where the heap stands 4 or more blocks over the plain, with grass round the foot) |
| Steppe | none of its own: its shape is the valleys below (the gullies it had went unseen on its flat ground, and the kind remains for the foothills) |
| Swamp | hummocks |

`worldprobe features <seed> <x> <z> <half>` counts them.

## Valleys

Long troughs with flat floors, broad slopes and cliff bands, cut into inland ground (6 blocks or more over the sea, under about 220) after the tableland terraces and before the last water routing, so rivers find the floors (`valleys.rs`). A valley is a pure function of the seed and a place: a noise field along the world's ridge angle, bent at three scales so it meanders, with distance to its line measured as the field's value over its slope, so floors stay about 200 blocks wide where the field is flat as well as where it is steep. Floors are flat, slopes rise over about 200 blocks (gradient 0.2 to 0.35), and the cut is 40 to 70 blocks, deepening and shallowing along a valley's length (never under 40% of its depth).

Before the valleys are cut, steppe ground is flattened (`valleys::flatten`): pulled 90% of the way toward a copy of itself smoothed over 60 cells each way (about 4 km), averaged over land only, in proportion to how steppe-like the ground is (the same warmth and moisture band as the siting, softened over 40 cells so the zone has no cliff at its edge) and how far over the sea it is (none under 20 blocks, full from 80, so there is no coastal step). Mountains and the ground within 8 cells of one are left alone. It makes steppe broad open plain with long sightlines, removing hills smaller than a few kilometres. A bigger radius makes steep edges, and hills wider than the radius, such as foothills, remain: it flattens the ground and does not change where steppe grows.

About a third of a slope is cliff, in stretches a few hundred blocks long: a face 14 to 64 blocks wide that sits at a height up the slope which varies, with a ragged edge, so a wall is a broad slope in some stretches and a broken face in others.

`valleys::run` writes `map.valley`, the share of the cut each cell takes, 0..1 as a byte, sited from the ground as it stands before the cut: full on warmth 0.26 to 0.58 (after the height lapse) and moisture under 0.3, which is where `classify` puts steppe, with a quarter of it everywhere else on flat ground, scaled by the seed's `valley_share` (0.5 to 1) and by how far the ground is over the sea. Steppe is favoured, not required: a valley can lie in any biome. Two things keep valleys off water. None is cut within about 7 cells of a lake or of a basin hydrology will fill (a priority flood on the ground after the terraces finds them), and none within about 14 cells of a big basin (2,000 cells, 6 blocks deep), since a valley's sump at the flat rim of a big dry basin fills the whole basin into a lake that floods the plain round it. After the cut, the basins the valleys made (cells no basin held before) are drained by a channel cut through their dam, two rounds of the drain `hydrology::run` uses for small basins (`hydrology::drain_where`).

The cut is `depth * soft_mask * share`, so every LOD sees the trough. Block-level ground adds `depth * share * cliff * (soft_mask - sharp_mask)` (`WorldTerrain::valley_sharpening`), so the face steepens into a cliff while floor and plateau stay where the map put them. It is calmed near water like the tableland's beds, and fades out in cells wider than 16 blocks (none from 32). `TerrainSource::valley_wall` is 0..1 where a cliff face is: it extends `cliff_boost` and adds a gradient of 1 at full wall to the one blocks are chosen by (`terrain.rs::block_gradient`), so mid-steep faces show rock. LOD cells take the same bias so colours do not change at a swap.

`worldprobe`'s `valley` and worldview's `hillshade` layer show them. `generate_with(seed, config, false)` makes the same world without valleys, to compare.

## Not built

Gneiss and andesite, objects (erratics, termite mounds), and LOD fidelity for any of this: see `world-plan.md`.

## Placed objects: boulders, giants and termite mounds

`generate/rocks.rs` sets single objects by a hash of position on grids of cells, one at most in each, and builds them block by block with the chunk, after the trees (a trunk keeps its place). A boulder is a half-sunk, squashed ball of the rock the geology has at its foot (`rock_column` at the ground it sits on, so marble on marble country, and not a stone of its own) with a nibbled rim, on ground up to gradient 0.9 and within 3 blocks of height under it, sunk to the low side. The taiga has them 2 to 5 blocks in radius in 4% of its 24-block cells. The tundra has three sizes and nothing in between, each scaled smoothly over fields 96 blocks across by a factor from 0.1 to 2 (`field`), so they come in drifts with open ground between. Small rocks, 0.9 to 2.3 blocks in radius leaning to the smallest, on a 12-block grid (`SMALL_*`, up to 30% of cells). Outcrops, elongated low ledges 5.5 to 11 blocks in radius and a third as tall as wide (`OUTCROP_*`), on the 24-block grid in about a tenth of its cells, on ground that may vary 6 blocks under them. Giants (`GIANT_*`), 5.4% of the cells of a 96-block grid, kept off the cell's edge: four fifths a lone tall boulder 7 to 11 blocks in radius, and one fifth an arch (`arch`), so an arch is a rare find, about one in 80 cells: two pillars 6 to 9 blocks in radius with a gap of 3.5 to 4.5 blocks between, to walk through, and a long thin capstone laid over the gap and resting on both, its underside 4.5 blocks over the ground. A giant's feet must be dry and within 8 blocks of height of each other, which with its pillars is why `ROCK_REACH` is 20 (the ledges alone need 12). A termite mound is a cone of dirt 3 to 7 blocks high and 2.5 to 4 in radius on savanna (7.5% of cells), on level ground (gradient 0.4, within 1 block). None is placed on a mountain or volcanic modifier, within 3 blocks of water or under 3 blocks over the sea. In the cold their blocks are weathered at meshing time, dimmed to a plain darker grey (`docs/climate.md`, `GrassTint::weathered_at`), so marble boulders aren't bright white; placement and shape are untouched. In LOD meshes each is drawn as cells while its cell is small enough to show it: small rocks up to 2 blocks across, the rest on the 24-block grid up to 4, giants up to 8 (`ROCK_MAX_CELL`), each cell a rock if its middle is, past which one is under a pixel.
