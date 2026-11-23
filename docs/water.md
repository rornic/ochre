# Water

How the sea, lakes and rivers are made, and the rules that keep them from leaking. Read this before
touching `ochre-world/src/worldmap/{hydrology,water,source}.rs`.

## The one rule

Water is held in by ground at least as high as its surface. A wall of water is a wet column beside a
dry one whose ground is under the water's surface. Everything below exists to make walls impossible
by construction, not to patch them where they show up.

Two exceptions are meant: a river's waterfall stands over the ground at its foot (it is the lip of a tread, see below), and the terrain
crate draws a curtain there; and a river can run on a levee across a slope, held in by its own bank.

## Map level: `hydrology::run`

`worldmap::generate` runs hydrology twice: once on the raw terrain, so small basins are drained
before erosion wears the ground, and once on the final ground. Nothing that comes after the last run
may lower ground beside still water under its level. `gameplay::crossings` (bridge sites flatten the
banks of wide rivers) is the one later edit, and it keeps every cell a block over any lake or sea
beside it (`held_over`).

The last run, in order:

0. Before the run, `landform::terrace` steps tableland and mountain ground into its beds (see `landform::tableland`), so basins, lakes and rivers form on the treads they will stand on and not on the smooth ground over them. Mountains were stepped at block level only, so a mountain river was routed over smooth ground 30 to 80 blocks over the real treads, followed them down and was cut off short of its lake (below).
1. `scatter` adds up to a block of noise to the ground, for good, so rivers on gentle ground wind
   instead of following the flood's straight lines. It used to be added for the flood and taken out
   again, which left lakes worked out on ground a block off the ground they lay in.
2. A priority flood (Barnes et al.) from the open sea and the map's edge gives every cell the height
   it fills to and the cell it drains into. The cells it raised, joined eight ways as the flood moves,
   are basins, each with one spill height (its brim).
3. Basins smaller than `MIN_LAKE_CELLS` or shallower than `MIN_LAKE_DEPTH` are drained by `drain`:
   a channel from the floor out along the flood's path, each cell under the last, until it reaches
   lower ground (complete breaching, after Lindsay 2016). It stops at water downstream rather than
   drain that too, and never cuts under the sea. A small hollow already under the sea is filled to just
   over it instead, so the sea can't come in up a channel. Up to `DRAIN_ROUNDS`, as draining one basin
   can open another. A basin under `FILL_DEPTH` (2 blocks) is filled to its brim instead: on level
   ground its channel ran for kilometres and left a dry pit with a ditch out of it. One up to
   `BREACH_MAX_BASIN` (12 blocks) deep is filled part way first, so its channel is cut no more than
   `BREACH_DEPTH` (2 blocks) into the ground past its rim (hybrid breaching and filling, Lindsay 2016):
   cut from the floor, the channel was a trench one cell wide and as deep as the basin, which the
   smooth ground turns into a chain of round dips, and where it steps diagonally a cell beside it is
   cut too, so its cells meet along an edge. A deeper basin is a crater or a cirque (37 blocks and
   more) and keeps its full notch.
4. `balance` sets each basin's lake level from its water budget, in the spirit of Fill-Spill-Merge
   (Barnes, Callaghan and Wickert 2021) but for a steady state rather than one storm. Flow runs
   downhill with rain from `rain` (runoff by dryness and rock, the same model rivers use). All that
   drains into a basin is gathered; the lake spreads cell by cell from the floor up, each lake cell
   losing `LAKE_EVAPORATION` by how dry it is, until it loses all that comes in, or reaches the brim
   and spills the rest on. A lake is either full, spilling from its surface into the river that leaves
   it, or held under its brim by what it loses, with no outlet. There's no depth or size cap: capping a
   wet basin's lake left water with nowhere to go but a river starting from nothing at the rim. Then:
   - a basin whose floor is under the sea holds water at least `INLAND_SEA_ABOVE_SEA` over it, or it
     would stand dry under the sea's level, which the terrain crate would fill with sea;
   - fewer than `MIN_POND_CELLS` under the level and the basin is a dry flat.
   Pits inside one basin share its level: two puddles in a dry basin stand at the same height. That
   is a simplification of FSM's hierarchy, and the only place two separate waters are forced level.
5. Basins whose lake doesn't fill them (closed, or held) are rerouted by `reroute`: a flood from the
   lake outward over the basin, so the rivers in it run down to the lake, not out over the brim the
   first flood found. Flow is gathered again over the new routing, with lakes as sinks.
6. Sea cells are the open sea; lake cells are each basin's cells under its level, their beds dug at least
   `SHALLOWEST` under the whole block their water is drawn at, so every cell the map calls lake is lake
   in blocks too (otherwise a lake whose cells only just hold water comes apart into pools); rivers start where
   enough water drains and the ground is steep enough, and are carved into the ground as before, but
   never under the level of a lake or the sea beside the cell (`still_beside`): a carved cell under a
   lake's level would let the lake's water into its whole square.

`water_kind`, `water_surface`, `river_width`, `flow` and `down` are what it leaves on the map. Lakes
and the sea are no longer decided separately: a walled-in basin under the sea is just a lake.

What it means for the world: wet country's lakes fill to their brims as before; dry country's stand
lower in closed basins, or are dry. `LAKE_EVAPORATION` (wet, driest) is the knob: raise the second
number for smaller desert lakes.

Fjords (`shapes::fjords`) are cut after erosion and before the last hydrology run, their floors under the sea, so the sea fills them by height and no lake or river sees an unfilled trough. Volcanic cones are raised the same way with a crater a quarter of their height under the rim, at most 36 cells across so a crater stays under the smallest lake hydrology keeps (300 cells) and is drained. Inselbergs, cold uplands, tower karst and cirques (`shapes.rs`) follow the same order: a cirque's bowl is under 300 cells, so hydrology drains it by a channel instead of making a lake. Valleys (`valleys.rs`) are cut into the map's height after the terraces and before the last run, so rivers find their floors. They are kept off lakes and big basins, and a shallow basin a valley makes is filled up to its dam (`hydrology::fill_where`), since a valley that fills a dry basin turns the plain round it into a lake. Their block-level wall steepening (`WorldTerrain::valley_sharpening`) lies inside `landform_ground`, so it is held to still water and calmed near it like the beds. Tableland's beds step the map's height before the last run (`landform::terrace`), so basins and rivers form on the treads and terrace faces bound lakes. Relief features (`ochre-base/src/features.rs`) and undercuts (`generate/undercut.rs`) are laid only where no river, spring, lake or sea is near, so the one rule above is untouched. Waterfalls are a river-profile step: see Rivers below and `docs/terrain-perf.md`.

## Block level: `WorldTerrain::sample_for_cell`

The terrain crate reads one `TerrainSample` per column (`ground`, `water_surface`, `water_kind`,
`spring`) and nothing else, so all of this lives in `ochre-world`.

### Still water

`still_round` finds the sea or the lake whose cells are among the four round a point, at its level
(a lake's floored to a whole block). That is all a point needs, because of a property of the flood:
every cell beside (eight ways) a basin's cell is either in the basin or at least as high as its brim,
or the water would have spilled there. So among the four cells round any point near a lake, every
non-lake corner is at or over the lake's level, and the water's edge always falls inside the four.
Where the smooth ground (`map.height_at`, bicubic) is under the level, the point is in the lake. Where all
four cells round it are the water's, which no rim can be among, it's in the lake up to `DROWN` over the
level too, fading as fewer of the four are, so low strips between a lake's cells go under.
The shore is the smooth ground's contour at the level: no cell's side ever shows, and no water ever
stands past a rim's crest on ground falling away beyond it. The same holds for the sea.

`contain` then holds the ground to the side of the water its smooth ground is on: under the level
where the smooth ground is, by as much as it is up to `HOLD`, and over it by as much where it's over.
The detail laid on the ground can't poke an island through the water or dig a dry pit under its
level beside it. The hold on the dry side is looked up beyond the water (`still_by`) and fades by
`near`, so it does not stop at a cell edge. A lake's hold is measured in blocks,
not cells. `lake_field` gives each map cell its distance to the nearest lake cell's centre and that
lake's level. `near` is 1 out to 32 blocks and eases to 0 over the next 96, so deep gullies return
gradually instead of at a cell edge. Where two lakes are within reach only the nearer holds the
ground. The sea keeps its one-cell reach. Detail is
also calmed to nothing at the water's edge over `CALM` blocks, for smooth shores.

Inland, out of any lake, the ground is never under the sea (`LAND_ABOVE_SEA`).

### Chunks lay no water the map does not hold

A chunk's blocks hold water only in columns where the map has water over the ground (`ChunkColumns::water_top`), and in ground a cliff cut opens beside such a column, up to its level (`flood_top` in `generate_chunk_with`). Two rules put water elsewhere, and both showed as single blocks of water stuck to a fall's cliff, apart from the river (a few to a dozen near each tall fall):

- The bank's material is chosen by the highest water within two columns that stands within a block of the ground (`columns::shore_top`), a beach where a river meets the bank. It was also handed to `terrain_block` as the column's water, so a dry column whose ground lay one block under a river two columns off had a block of water laid on it. Near falls the banks drop away, so that is where they were (162 of 171 in 600 blocks round the tallest fall of seed 6). `shore_top` now picks the ground's material only, in chunks and LODs.
- Ground a cliff cut opens within four columns of fresh water flooded to the highest water of the eight places four columns off, however far under that the cut was and however far from the water it stood: beside a tall fall that is the top of the fall, so cuts in the cliff down its whole height filled with water. A cut now floods only if a column beside it (the four) holds water at its height.

`worldview <seed> strays x z half` counts the water blocks in dry columns round the falls in a box: on seed 6, in 800 blocks round four falls, 69, 253, 57 and 117 on the ground's top and 37, 239, 135 and 72 in cut ground, to 0 and 4, 19, 7 and 8 (the cuts that still flood are beside real water).

### Rivers

Rivers are curves through their cells (`Stretch`), each with a level profile that never rises
downstream, ramps to the lake or sea it runs into, and starts at the lake it leaves
(`build_river_profiles`). `RiverProfiles::index` lists each stretch in every cell its water, banks and
valley reach, plus the meander, so a point looks only at its own cell's list, however wide the river.
Each stretch also knows its river (`stem`, the run of widest feeders it's part of), so the second river a
point sees is another river, never another stretch of a wide one: a river wider than its cells reaches
over several of its own stretches, whose different levels read as humps if taken for a second river.
Consecutive profiles share their endpoint level: a lower downstream start brings the upstream
endpoint down to it, carrying the drop in the last profile interval instead of jumping between
different heights at the same point.

`channel` cuts the bed and eases the bank into the surrounding terrain over its width.
At full block detail, a dry column near a river then checks its four actual neighbours. If a
neighbour contains water, the dry column is raised to the block containing that water's
surface. The same containment check handles bends, springs and adjacent still water without
estimating a neighbouring level from an upstream curve point. A conservative geometric
search bound avoids checking neighbours far from water; LOD cells keep the channel's smooth
geometry rather than this block-level correction.

A steep stretch runs in treads and falls (`stairs`, `docs/terrain-perf.md`): its profile holds level for a few blocks, then drops 5 blocks or more at a lip, over about a block, and `river_hits` reads the level of the tread the nearest point is on. The bed steps down to the lower tread `SHEET_THICKNESS` (4) blocks before the lip, reading the next stretch's levels past a stretch's end (`level_ahead`), but only where a lip lies in that reach: a rapid that runs down a ramp, as a drop just under `FALL_MIN` does, keeps its own bed, or every column of the ramp read as a fall and drew a fin of sheet where its level stepped a block, which on a ramp across the map's diagonal was a white sawtooth and spikes standing out of the surface. A sheet is as thick wherever its lip falls, and a coarse LOD's cells, which sample one column in 2, 4 or 8, still hit it (2 blocks thick, and reading only its own stretch, a wide fall's sheet had gaps in it from 200 blocks off). Each tread is held under the ground the stretch was graded to, so no bank is built up to hold it, and a point either side of a lip is a river's water at its own level: the water mesh draws a curtain between columns that differ by 2 blocks or more. `water_audit` counts a fall as a step between two river columns and, beside a dry column, as a wall.

Where a river meets still water:
- **joined**: its level within `JOIN` of the still water's, as at a mouth or outlet. Inside the
  still water the river is left out; the still water's shore is the edge there. Near it the river
  cuts its bed but builds no banks, and its water takes the still water's level, so the two meet at
  one height instead of being parted by a strip of bank.
- **under** the still water: the still water covers it.
- **over** it, passing by: it is cut and banked as ever, and its banks keep the two apart.

The same barrier holds between two rivers: the second river's bank holds too unless the two are
joined or the point is in the nearest's water (where a stream runs into a river, its banks would
otherwise stand up through the river as strips), and a second river's bed isn't cut where it would sink the first's bank under the first's
water. This is the rule Minecraft's aquifers use: wherever two waters at different levels meet,
ground stands between them.

## Flowing water

Generated water is still and never moves. Water the player lets out flows (`ochre-terrain/src/flow.rs`):
it falls as far as it can, spreads up to 7 blocks sideways from where it rests, a level less each block,
and dries up back from wherever it's cut off. Flowing levels live in `ChunkData` beside the blocks.

The one rule now matters at block level, not only to the eye: still water beside or over air or flora
spills the moment an edit nearby stirs it. `test_lakes_and_the_sea_have_no_open_block_beside_or_under_their_water`
holds lakes and the sea to that. Rivers have open blocks at every step down their course, so flow never
spreads over still water, only joins it. A waterfall's lip still spills when stirred.

Neighbouring surfaces less than `WATER_SLOPE_MAX_STEP` (2 blocks) apart are drawn as one slope, so a
rapid climbing more than a block a column is a ramp, not stairs. Bigger drops are curtains.

## Checking it

- `cargo test --release -p ochre-world`: `test_no_lake_or_sea_cell_stands_over_the_dry_cells_beside_it`
  (map) and `test_no_lake_or_sea_stands_as_a_wall_over_the_ground_beside_it` (blocks round every
  40th shore cell) over three seeds. The hydrology tests cover basins, draining, below-sea basins and
  rivers that never rise.
- The terrain crate's water tests (`generator/tests.rs`) are the block-level regression spec: walls,
  crevices, joins, ramps, LOD water.
- `cargo run --release --example water_audit -- <seed>` counts walls, steps and raised banks round
  rivers, lake shores and coasts, one `AUDIT` line each, for a sweep of seeds before and after a
  change. Each line counts only its own sampled water kind, even when a window also contains
  another kind. `--fine` also checks the four immediate neighbours of sampled river columns,
  reporting height gaps and edges where the rounded dry ground lies below the water block.
  This samples the existing windows rather than every river column. Walls left on rivers are
  mostly waterfalls.
- The `worldview` skill to look, and `worldview <seed> info x z` for one column's ground, dry
  ground, smooth ground, water and map cell.
- `worldview <seed> dryrivers 0 0` counts the map's river cells with no river or lake water in their own 32-block square, and which of them have it on their line a few blocks off: the meander (`source.rs`, up to 43 blocks) draws a river off its map line, so a cell's square can miss its own river. It is 99% of them (seed 1 581 cells, 578 with water on their line; 42 701, 696; 1337 805, 800). The rest are the last cell of a river into the sea, a beach the river stops short of (`ends`, below 0.15). Zero are a stretch with a gap in its water. `worldview <seed> course x z` lists one stretch's reach, levels and the blocks' ground and water along it.

Before this model (seeds 368871838, 5, 2198630850, the default 65 km world): lake walls over 1.5
blocks 0 / 23 / 16 (6+ blocks: 0 / 6 / 10), dry map cells under a lake's level 32 / 42 / 61. After:
lake walls 0 / 0 / 2 (none over 6), dry cells under a lake's level 0 / 0 / 0.
Across 16 seeds: lake walls 61 to 3 (over 6 blocks 17 to 0), dry map cells under a neighbouring
lake's level 543 to 0, river walls 105 to 60 (over 6 blocks 4 to 0).

## Research

- Barnes, Lehman, Mulla 2014, *Priority-flood*: the flood every basin and river comes from.
- Barnes, Callaghan, Wickert 2021, *Fill–Spill–Merge*, Earth Surface Dynamics 9: lakes from a water
  budget over the basin hierarchy.
- Lindsay 2016, *Efficient hybrid breaching-filling sink removal*, Hydrological Processes 30: draining
  a basin by one channel all the way out, or filling it part way to keep the channel shallow.
- Génevaux et al. 2013, *Terrain generation using procedural models based on hydrology*, and Fischer,
  Boeckers, Zachmann 2022: water first, ground shaped to hold it.
- Minecraft's aquifers (1.18): per-region fluid levels, with stone only where neighbouring levels
  disagree.

## Water in dry country

Rain a cell sheds, and the flow a river loses crossing it, scale with how dry its ground is (`rain` in `hydrology.rs`, `climate::dryness`): moisture of 0.3 and over is not dry and 0.1 and under is fully dry, easing between. Steppe sits under 0.3, so it is dry in its drier half and not in its wetter, and desert (under 0.14) stays dry. The curve was 0.2 to 0.45, which made all of the steppe as dry as desert: it had about a twentieth of grassland's river cells (0.05%, against 0.3 to 1.2%), and a stream died within a cell or two (a dry cell takes up to `DRY_EVAPORATION` cells' worth of flow). On the new curve steppe has 0.2 to 0.7% river cells on four of five seeds, as grassland and savanna do, and desert stays under 0.1%. Lakes in dry country stand higher, since their evaporation follows the same dryness.

A bank made wet by a river or lake makes the ground round it wetter (`RIPARIAN_MOISTURE`), which in dry country turned a stream's banks into forest. Ground drier than 0.28 moisture now has banks capped well under that (`DRY_BANK`, `DRY_BANK_MOISTURE`, see the paragraph below for the value), easing back to the old rule by 0.35.

A river through a valley showed as a line of dashes in dry country, and a grassland band the width of the valley round it. The valley's floor was a long closed trough, the lake in it stood just over its floor, and the lake was a string of ponds. `valleys::run` now fills every shallow basin that lies mostly in a valley up to its dam, and `climate::big_lake_cells` leaves a lake under 20 cells out of the riparian bank, so a pond wets no ground. A lake of 20 cells or more still does. What is left of a trough that has no stream of its own is a dry valley.

Banks in dry country stay as dry as their ground, within reason: ground drier than 0.28 moisture gets banks no wetter than 0.29 (`DRY_BANK_MOISTURE`), just under where steppe ends at 0.3, so a lake or big river in the steppe leaves a steppe shore and not a patch of grassland a kilometre across (a lake wetted ground 14 cells, about 450 blocks, round it). It had been 0.5. In hot country the same cap makes a desert river's belt savanna, where it had been forest.

Filling, not draining: a basin cut out through its dam left a stepped ditch from its floor out to lower ground, a cell at a time and each just under the last, so on the flattened steppe it ran a long way, a dry riverbed that held no water, ending in a round pit. A valley's basins 12 blocks deep or less, or 12 cells or fewer, are now filled to their spill level (`hydrology::fill_where`), a flat with no pit and no channel. A deeper one is left as it is: filling it plugs the valley (the basin by the seed 1 spawn is 34 blocks deep and 650 cells, and filling it raised the spawn by 35 blocks), and draining it cut a one-cell slot that deep across the plain, the dry ladder by the spawn. A deep valley basin is a closed pit, its lake, if it has one, a pond limited by evaporation. Outside valleys, the small basins of the last run are filled or breached shallow, see step 3 above.

## Beds

The sea's floor and beaches are sand. A lake's or a river's bed, and its banks within 2 blocks of its water and a block of its level, are what the ground there is (`terrain::fresh_bed`, through the biome's own rules first): mud where grass would grow, gravel where the bed slopes 0.3 or more (most rivers) or where snow would lie, the geology's rock where the ground is rock, and sand in sandy country. On seed 1, beds at lake and river cells went from sand 2,301 and gravel 261 of 2,563 to mud 2,096, gravel 313, sandstone 86, limestone 30, shale 29 and a few others; at river cells alone gravel 61, mud 35, rock 15 and sand 3.

