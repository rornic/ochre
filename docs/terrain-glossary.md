# Terrain glossary

What each landform and ground feature in Ochre is, what it looks like in the real world, and how it shows up in the game. Entries give sizes in blocks (one block is about a metre). The mechanisms are in `landforms.md` and `climate.md`, the numbers in `terrain-perf.md`, and the plan in `world-plan.md`.

## How the ground is made, in three layers

1. **The map.** A 64 km grid of 32-block cells holds height, rock, water and climate. Erosion, rivers and lakes are worked out here, once per seed.
2. **Shapes.** Large forms (fjords, cones, inselbergs, karst, cirques) are pressed into the map's heights before water is routed, so rivers and lakes form round them.
3. **Chunk detail.** When a 16-block chunk is built, small forms (relief features), cliffs and undercuts are added to the ground at block scale. Far chunks are coarser (see LOD) and show only part of this.

A form that is part of the map shapes where water goes. A form added at chunk time never does, which is why chunk-time forms keep away from water.

## Large forms

**Mountain.** The map's one `landform` flag. Mountain ground is steep nearly everywhere, takes the mountain biome modifier, and gets the longest cliffs.

**Tableland.** Flat-topped country cut into steps. It is sedimentary (layered) ground, 40 to 360 blocks over the sea, that is not mountain. Hard rock layers stand as flat benches and soft layers (clay) as slopes.

- **Bench or tread.** The flat top of a hard layer. Lakes and rivers can sit on it.
- **Face or riser.** The steep step between two benches.
- **Apron.** The slope of loose material at the foot of a face.
- **Mesa and butte.** A mesa is a flat-topped hill with cliff sides, a butte a smaller, narrower one. Tableland cut up into many of them is badland.
- **Scarp.** A long cliff line where a hard layer ends.

**Fjord.** A glacier-cut valley that the sea has flooded: a narrow inlet with a flat floor and walls 40 or more blocks high. Cut into mountainous coasts.

**Volcanic cone.** A conical mountain raised on volcanic ground, with a crater at the top. A crater on land stays dry, and one below sea level is sea.

**Inselberg** ("island mountain"). A bare rounded rock dome standing alone on a plain, 20 to 45 blocks high and 8 to 20 map cells across. Desert-weighted. Real examples are Uluru and Sugarloaf Mountain.

**Tower karst.** Steep-sided limestone hills in a field, 30 to 70 blocks high, in groups of 10 to 40. Wet-weighted. Real examples are Guilin and Ha Long Bay.

**Cirque.** An amphitheatre bowl bitten into a cold mountain by a glacier, with a steep back wall (the headwall) 40 to 100 blocks deep. It is not allowed to become a lake.

**Gorge.** A river cut into tableland with a wide flat floor and a steep wall on each side.

## Relief features

Small forms a biome lays over its ground. Each is a pure function of seed and position that adds or removes up to 18 blocks of height. They never touch ground near water or the sea, and they vanish in coarse LODs.

| Feature | Biome | What it is | In the game |
| --- | --- | --- | --- |
| Crevasse | glacier | A crack in glacier ice. | Slots 2 to 5 wide and 8 to 18 deep, in belts across gentle slopes, with a bridge of uncut ice every 15 to 30 blocks. |
| Serac | glacier | A tower of ice left where crevasses cross. | Jagged pillars 6 to 18 high on steep ice. |
| Pingo | tundra | A mound with a core of ice that froze and pushed the ground up. | Round mounds 6 to 14 high, 8 to 16 across. Rare. |
| Patterned ground | tundra | Polygons of frost cracks in frozen soil. | A net of low ridges 12 to 20 across, one block high. |
| Drumlin | taiga | A smooth streamlined hill left by an ice sheet, long axis along the ice flow. | Parallel swells 80 to 200 long, 4 to 8 high, along the world's ridge angle. |
| Yardang | desert | A ridge carved out of soft rock by wind-blown sand. | Parallel ridges up to 5 high and troughs up to 2.5 deep, along the wind. |
| Kopje | savanna | A small rock hill on a plain, from granite that weathered into rounded heaps. Also *koppie*, or *tor* elsewhere. | Heaps 6 to 14 high, 10 to 25 across, in clusters of 2 to 6. |
| Gully | steppe | A small channel cut by runoff down a slope. | Swales 2 to 6 deep and about 10 across, running along a direction fixed for the world. |
| Hummock | swamp | A low tussock of raised ground in wet ground. | Mounds 1 to 3 high, 4 to 8 across. |

## Cliffs and carving at chunk time

**Cliff pass.** On ground steeper than gradient 1 the surface is cut and built up from 3D noise, up to 18 blocks where the seed's `cliff_boost` reaches (mountain and tableland). It makes ledges and rough faces. The top 3 blocks of a column are left alone so the grass is not pitted.

**Overhang.** Rock that sticks out over open air. A heightfield cannot hold one, since it stores one height per column, so overhangs are cut into the blocks.

**Undercut.** Rock cut away at the base of a cliff so the top sticks out. Here it is a recess under a cap, deepest at the foot of the face and tapering to nothing under the cap. It only appears on steep ground, never within 24 blocks of water.

**Arch.** What an undercut leaves where a rib of rock is cut from both sides.

**Cap or roof.** The rock left on top of an undercut.

**Foot and rim.** The bottom and top of a face. The cut's depth falls from the foot to the rim.

## Objects placed by hash

Single things dropped on the ground by a hash of position, not shaped from the heightfield. Trees are the existing example. Each is built block by block when a chunk is built.

**Glacial erratic.** A lone boulder carried by a glacier and left where the ice melted, of a different rock from the ground it sits on. Real ones run from a metre to house size. Planned for tundra, taiga and the cold edge of glaciers, as rounded stone 2 to 6 blocks across.

**Termite mound.** A tall earth tower built by termites on savanna, 2 to 6 m high in Africa and Australia. Planned as slim cones of dirt with a few blocks of height variation.

**Kopje rock top.** The kopje relief feature is a rounded heap, but its top is grass because the block rule only looks at slope. A rock top means the core of a kopje shows bare stone, as a real one does, with a grass skirt round it.

## Level of detail (LOD)

**Chunk.** A 16-block cube of blocks, built at full detail near the player.

**LOD.** Level of detail. Chunks beyond `full_detail_distance` are replaced by coarser meshes that cover 2, 4, 8 or more times the area, so the horizon costs little to draw.

**Heightfield.** A surface stored as one height per column. LOD meshes are heightfields, which is the reason they cannot show an overhang, an arch, a cave or a tree-sized object.

**LOD seam.** The line where full-detail chunks meet a coarser mesh. Where the two disagree on the ground's height there is a visible step. Steep fjord walls and the reach of the cliff pass leave seams of up to about 3 blocks.

**LOD pass.** Work to make far terrain agree better with near terrain: matching the cliff pass's reach, showing the height of terraces and boulders, and where it is cheap showing the cap of an undercut as a cliff. Planned.

## Climate and biomes

**Climate class.** One of the biomes a warmth and moisture pair gives: glacier, tundra, taiga, steppe, savanna, desert, jungle, forest and others (the table is in `climate.md`).

**Modifier.** Mountain or volcanic ground laid over a climate biome. It changes the blocks and trees, not the climate.

**Treeline.** The height above which trees do not grow, set by warmth.

**Transition zone.** Near a class boundary a block takes the biome across it in patches of 10 to 20 blocks, thickest on the boundary and stopping two map cells from it.

**Grove.** A patch of denser trees with clearings between, from slow noise at about 160 blocks.

**Undergrowth.** Ferns, tall grass and other low plants on a forest floor, by biome.

## Words used in the code and the plan

**Spike.** A throwaway experiment with a written pass or fail condition, run before a feature is built. A failed spike leaves the feature unbuilt, with the numbers recorded.

**Gate.** A limit a change is measured against: vertex count at most 1.10 times the commit before, no uncontained water, and so on (`world-plan.md`).

**Census.** A run over many seeds to check no biome fills more than 70% of land and every seed has somewhere to spawn.

**`GENERATOR_VERSION`.** A number in the map's file name. A change that alters the world bumps it, so an old cached map is not loaded for the new rules.

**Province.** The rock a plate or range is made of: sedimentary, volcanic, metamorphic or intrusive.

**Facies.** The kind of sedimentary rock at a place, such as limestone or clay.
