# Tectonic world MVP

> A dated log of how the tectonic world was built, newest decisions last in each section. What the world does now is described in `docs/landforms.md`, `docs/climate.md` and `docs/water.md`, and the scope and the open defects are in `docs/world-plan.md`; numbers per change are in `docs/terrain-perf.md`.

The first iteration replaces noise-selected mountain belts with a procedural tectonic
snapshot. The art direction is an alpine coastal region: deep offshore water, rugged
coasts, forested slopes, high peaks and a sheltered dry interior. The map remains 64 km
across at 32 m/cell; it represents a compressed region, not an entire planet.

## What changes

- World bounds are signed and configurable, defaulting to -1024..2048 (exclusive top).
  Sea level is zero. Missing water is identified by its kind, never by positive elevation.
- Twelve seeded plates carry motion. Continental crust crosses plate boundaries.
  Relative convergence makes collision belts or asymmetric subduction ranges/trenches.
  Other boundary types are identified; landforms beyond mountain belts (tableland, fjords, cones, inselbergs, karst, cirques) are laid by `docs/landforms.md`.
- Erosion and water routing run over the new ground. Stream erosion varies with rock type.
  Fixed peak rescaling is removed; the vertical budget is not a mountain shape algorithm.
- Flood queues use signed sortable height keys. Raw positive-float bit ordering cannot be
  used for negative ocean elevations. Monotone height interpolation preserves steep lake rims.
- Rivers remain water in coarse samples when their width resolves at that cell size; the
  old unconditional 16 m cutoff hid even broad rivers from overview maps and distant LODs.
- A regional warmth baseline replaces the miniature equator-to-pole gradient. Wind
  carries ocean moisture and loses precipitation on rising ground. Soil moisture reflects
  precipitation versus evaporative demand, without percentile-based biome quotas.
- Curvature retains its previous radius independently of build height. Clouds move above
  the intended summits. Snow and treeline elevations are tuned for the larger relief.
- A generator version bump invalidates older maps (the current version is `GENERATOR_VERSION` in `worldmap/mod.rs`). Cache identity includes bounds, geology
  and climate configuration; no legacy generator is kept for existing worlds.
- Range connectivity uses inland water surface slopes, keeping riverbed depth separate
  from mountain barriers. River crossing availability is still checked independently.

The world configuration lives under `[world]` in `assets/settings.toml`, with nested
`bounds`, `tectonics` and `climate` tables. Wind components point downwind; sea-level
warmth is normalized to 0..1. Runtime height expansion and variable sea level are outside
this iteration.

## Reviewing the maps

Use the repository's `.claude/skills/worldview/SKILL.md` workflow. Build the viewer in
release mode, run it, and export matching views:

```sh
cargo build --release --example worldview
target/release/examples/worldview 1 8777
# In another terminal:
bash scripts/capture-worldview.sh http://127.0.0.1:8777 target/tectonic-mvp/after after
```

The capture script uses seeds 1, 42 and 1337, the same coordinates, 64 m/pixel and
1024-square exports. Before captures use the original generator with only the viewer's
elevation palette expanded to match the after palette. Elevations are relative to each
generator's sea level. Do not compare independently normalized height images.

New viewer layers:

| Layer | Meaning |
| --- | --- |
| plates | Plate membership; distinct categorical colors |
| boundaries | Red collision, purple subduction, cyan extension, yellow shear |
| motion | White arrows showing plate motion direction |
| uplift | Pre-erosion uplift, dark to orange, 0..1400 m |
| precipitation | Background rainfall, tan to blue, 0..1 calibrated relative units |
| surface_temperature | Temperature after altitude cooling, blue cold to red warm |
| rivers | Drainage network, with display widths clamped for legibility; actual widths are in `/summary` |

Click inspection includes plate, boundary, uplift, precipitation, wind and bounds.
Boundary colors show influence bands rather than infinitesimally thin fault lines.
The existing temperature layer remains the sea-level field, distinct from surface temperature.
`/summary?seed=42` reports elevation range, peak position, maximum physical river width,
its location and its effective runoff area. The network layer is diagnostic; the regular
water layer and close-up exports show the sampled water surface.

## Checkpoint review (2026-10-27)

**Draft checkpoint, not ready to merge.** The coastal river that appears to run parallel
with the sea is an open visual correctness issue reported during review. A coastal
barrier could explain such a route, but that has not been established for this map.
Inspect drainage direction, terrain and water-surface profiles, flood-fill spill heights,
and the actual sampled water before accepting it. Visibility and routing are separate:
the coarse-sampling fix makes rivers visible but does not prove their routes are right.

A later desktop launch lost the graphics device after world generation. The rejected-seed
warning preceded a retry; both reported freezes followed this path.
The render failure has not been reproduced without a GPU; its headless comparison is
recorded in [terrain-perf.md](terrain-perf.md). A temporary safe-render preset started,
and a fixed seed then started with the original full render settings. The preset was
removed and the original renderer settings remain.
The random-seed geography rejection and retry have since been removed: each launch
builds one map, including seeds whose geography still has issues. The map diagnostic
validator remains available without affecting startup.

### Matching landscape comparisons

Comparisons used the same seed, coordinates, scale and elevation palette; generator
geometry intentionally changes.

Seed 42 rises from a previous maximum of 542 m above sea level to 1451 m. The new
maximums for seeds 1 and 1337 are 1828 m and 1984 m. These are map heights before
block-scale detail. The warped plate boundaries improve the straight initial belts;
geological shape and erosion still need further visual work.

### Review overlays (seed 42)

The region is 64 km across. Seed 42's widest physical river is 84.3 m at
`x=-14384, z=848`, with effective runoff area 35.16 km². Seeds 1 and 1337 have maximum
widths of 46.0 m and 71.7 m. A continent-scale river is not an expected consequence of
more vertical relief: catchment area, precipitation and routing determine discharge.
At 64 m/pixel, even the widest river is only about one pixel wide. The cyan network
is therefore a diagnostic overlay, not a literal rendering of those widths.

An earlier prototype's approximately 575 m river was invalid: raw float-bit ordering
broke flooding below zero. It is superseded by these exports and must not be used as
evidence of a successful large river.

### Validation and remaining work

- 364 release workspace library tests passed (base 24, engine 5, gameplay 18,
  terrain 237, world 80).
- Release Clippy across the workspace and all targets passed; formatting and shell
  syntax checks passed. Release viewer, flight, benchmark, water audit and seam tools built.
- Signed flood ordering, water at zero, coarse river sampling, cache configuration,
  deterministic terrain, variable vertical bands and range connectivity have regression coverage.
- Headless streaming timings and follow-up profiling are recorded in
  [terrain-perf.md](terrain-perf.md). GPU timings remain unmeasured.
- Water audits on seeds 1, 42 and 1337 report zero map-level uncontained cells.
  River-edge wall flags are 26/21/75, including 1/1/2 over 6 m. Seed 42's tall
  flag is near `x=-21752, z=15520`. These are unresolved diagnostic findings.
- The seed-42 seam diagnostic completed. Completion is not evidence that every seam is
  artifact-free.
- In-game mountain silhouettes and cloud/shadow intersections have not been systematically
  reviewed on a graphics device. The coast-parallel river remains unresolved.

### Draft follow-up from checkpoint 4883cfa

The seed-42 river visible near the southern coast was traced with `worldprobe route` from
`(3216,1392)` to the sea at `(6352,3632)`. Its map water surface falls from 59.9 m at
`(3472,1488)` to sea level. At the `x=4000` transect, sampled dry ground north and south
of the channel reaches 70.8 m at `z=1400` and 75.7 m at `z=2100`. This supports a local
valley barrier, but the checkpoint did not identify the exact reported segment; a longer
route or a better outlet elsewhere has not been excluded. Keep the visual issue open.
The detail view covered `x=0..8192, z=0..8192` at 8 m/pixel.

The audit previously counted every water kind inside each 192 m window. Its three seed-1337
"coast walls" were river water near the sampled coast cells, including a flagged river at
`(-10212,26188)`. Counting only the sampled kind in each category gives zero lake and coast
walls or surface steps for all three seeds. River walls remain 26/21/75 and steps remain
529/981/2183 for seeds 1/42/1337. Seed 42's 6.8 m river wall lies on a steep reach whose
water falls roughly 12 m over 24 blocks, consistent with the waterfall exception in
[water.md](water.md). The other river flags still need block-level review. The seed-42 wall
view covered `x=-21952..-21552, z=15320..15720` at 1 m/pixel.

### Draft steep-bend correction

At `(-21752,15523)` the seed-42 river surface was 775.51 m, while the dry column one block
south stood at 774.12 m. The closest point on the steep curve moves downstream as the sample
moves across its edge, so the dry bank used a lower river level. The version-40 fix read the
surface about one block upstream from nearby stretches of the same river. That dry column
stands at 776.42 m; the water's downhill profile and audit wet-column counts are unchanged.
The later block-neighbour containment check replaces that upstream estimate.
Generator version 40 invalidates older saved terrain because the banks change.
The matching viewer comparison changed 102 of 160,000 pixels, concentrated along banks;
the river footprint remained in place.

| Seed | River walls before / after | Over 6 m before / after | Lake and coast walls after |
| --- | ---: | ---: | ---: |
| 1 | 26 / 16 | 1 / 0 | 0 |
| 42 | 21 / 16 | 1 / 0 | 0 |
| 1337 | 75 / 48 | 2 / 0 | 0 |

The bend audits use the same 4 m sampling windows as the review reports. Seed 2790361079,
a known steep gully, has 11 river wall flags and none over 6 m.
Remaining river flags, waterfall curtains, and GPU appearance still need visual
review; a lower audit count alone does not establish that every steep bend looks right.

## Where the iterations went

Landform variety, climate and ecology and the first visual composition are done, in `docs/landforms.md` and `docs/climate.md`; the remaining ideas and defects are in `docs/world-plan.md`. Every iteration still needs matching viewer exports, deterministic fixture tests, water audits, seam/loading checks and timing comparisons. Geological plausibility supports the scenery; it does not substitute for looking at it from the ground.

### Later rock pass

After the initial landform, climate and visual stages, expand the rock palette and give
each addition a role in terrain and appearance:

- **Granite:** intrusive mountain cores, domes and fractured cliffs. Done as metamorphic
  cores, see "Granite and shale".
- **Shale:** layered sedimentary rock alongside limestone and sandstone; retain clay as
  a softer sediment or deposit. Done, see "Granite and shale".
- **Gneiss:** banded metamorphic rock in mountain belts, alongside marble.
- **Andesite:** volcanic rock for island arcs and volcanoes, alongside basalt.

Granite and shale are in. Gneiss and andesite were dropped from the plan, see `docs/world-plan.md`.
Tune erosion and make the strata visible with the block texture pass; names and textures
alone will not create distinctive landforms.

## Rock data: first landform-variety step

The world map now records broad volcanic, metamorphic and sedimentary provinces, based
on the seeded tectonic fields. Volcanic provinces sample as basalt, metamorphic ones as
marble. Sedimentary provinces alternate limestone, clay and sandstone beds every 160 m,
with a smoothly varying vertical offset. `WorldMap::rock_at` samples those beds at an
arbitrary elevation; the viewer's `geology` layer colors the rock at the surface. This is
a simplified starting model, not a geological history or a final visual treatment.
At this stage provinces are sampled at 32 m map-cell resolution, and their bedding is
smoothly offset rather than folded or faulted.

The geology layer uses dark for basalt, pale for marble, tan for limestone, red for clay
and orange for sandstone.

Stream erosion now reads the rock at the current surface on each pass. Basalt and marble
have lower stream erodibility than limestone; clay and sandstone have higher values.
Hydraulic droplets and thermal slumping still use the same rules for all rocks. Terrain
blocks and textures use the rock data now, see "Rock blocks".

The first erosion trial exposed a river profile discontinuity on seed 1: adjacent samples
at `(11508,-2708)` and `(11508,-2707)` differed by nearly 8 m because two consecutive
stretches disagreed at their shared endpoint. Profiles now meet there; the difference is
about 1.7 m. Tests retain this join, a block-level bank edge, a spring rim and the
earlier steep-bend fixture.

In the four seed audits, lake and coast walls were
zero. The fine check found no open block edges among the sampled river columns; the
four-block river check still reported 2, 0, 5 and 1 walls over 6 m for seeds 1, 42,
1337 and 2790361079. Some are steep reaches with intervening banks. These are sampled
checks, and the remaining waterfall and bend flags still need visual review.

The new erosion changes terrain, so generator version 41 uses new map caches. Version-40
terrain and its saved edits do not match the new world. Different block materials and
textures are deferred to the visual composition pass. Rifts, island arcs, geological age
and additional landform work remain in this iteration.

Matching views of seed 1 at 64 m/pixel and a seed-42 mountain basin at 4 m/pixel compare
the geology-data-only build with rock-dependent erosion. The basin view shows how a terrain
change can also change a lake and its drainage.

### Mountain river flats

On seed 3942325500, a river's bank hold raised the surrounding terrain to a constant
height far beyond its channel. At `(20749,-1920)` it raised ground by about 16 m. The
channel now eases into the terrain over its bank width; at full block detail, dry ground
beside actual water is held to the water's block height. This replaces the separate
upstream bank-floor estimate, so bends and spring edges use the same check. The new
terrain uses generator version 42. See [water.md](water.md) for the containment rule and
the sampling limits of the water audit.

Both views cover `x=20600..21624, z=-2320..-1296` at one block per pixel. In the
five seed audits, no sampled river column had an open neighbouring block. The
four-block scan still flags steep banks and falls; it is
not a direct test of whether the intervening blocks contain the water.

## Landform variety draft

Plate divergence now makes continental rift valleys with raised shoulders and submarine
spreading ridges. Lateral motion makes narrower transform valleys. Converging oceanic
plates produce a trench and a broken volcanic island arc. These shapes enter the map
before erosion and water routing. Stream erosion still uses the exposed rock type;
hydraulic and thermal erosion do not yet vary by rock.

The map also stores a relative crust-age indicator: seeded plate ages blend at plate
edges, with younger values at active spreading zones and volcanic arcs. This is a
procedural snapshot, not dated geology or a history of plate movement. The viewer's
`geological_age` layer shows young crust in blue and old crust in gold. Age does not
yet affect erosion or sedimentary bedding.

Generator version 43 invalidates earlier map caches because the terrain changes.
Matching overview captures for seeds 1, 42 and 1337 show new island chains and rift
lakes; these still need ground-level review. Fine water audits on five seeds found no
sampled open river block edges and no lake or coast wall flags. The coarser river scan
found 31 tall-wall flags on seed 1337, up from 20 before this draft; steep falls and
their appearance remain open for review. A seed-42 seam scan reported 28 candidate
steps across 374 sampled windows; the scan alone does not establish which are artifacts.
A short seed-1 headless loading simulation reported no holes in its sampled window.
The headless timing and vertex-count comparison is recorded in
[terrain-perf.md](terrain-perf.md).

## Rock-dependent slumping draft

Thermal erosion now varies its angle of rest with the exposed rock in each map cell.
The threshold uses the inverse square root of the existing stream-erodibility tuning:
harder basalt and marble retain steeper slopes, while softer sedimentary beds slump
further. The donor's threshold is used for both removal and distribution. These are
artistic values, and deposited material is not tracked as a separate sediment layer.
At this point hydraulic droplets still used a uniform erosion rate; geological age
remains diagnostic.
Generator version 44 invalidates earlier terrain caches and saved edits.

For seed 42, matching 64 m/pixel overview exports changed 656 of 1,048,576 pixels,
mostly in steep terrain. Drainage changed near the old seed-42 river-edge fixture,
which was moved to a new river reach. Fine water audits for seeds 1, 42 and 1337 found
no sampled open river block edges and no lake or coast wall flags. The coarse river
scan reported 5, 0 and 17 tall-wall flags respectively. Those flags, drainage changes,
and ground-level appearance need visual review; the sampled audits do not prove every
bank is contained.

## Rock-dependent hydraulic erosion draft

Droplets now scale their erosion demand at each brush cell by the exposed rock, sampled
again as its height changes. Basalt resists cutting more than clay. The coefficients are
artistic values at most one, so the brush cannot take more than the original downhill
limit. Sediment carried by a droplet still equals the material removed; deposition and
the existing handling of droplets leaving the grid are unchanged. Version 44 includes
both rock-dependent passes.

On seed 42, matching 64 m/pixel overview exports before and after this hydraulic pass
changed 4,231 of 1,048,576 pixels, including changes to lakes and drainage. Fine water
audits for seeds 1, 42 and 1337 found no sampled open river block edges and no lake or
coast wall flags. The coarse river scan reported 0, 1 and 20 tall-wall flags respectively.
These sampled checks do not establish containment everywhere. Ground-level scenery,
waterfalls, seam candidates and changed drainage still need visual review.

## Foothill relief adjustment

Foothill scarps, gullies and the range-driven roughness floor now use 60% of their
previous strength. Mountain relief keeps its own full-strength treatment. A matching
block-scale view around seed 300059728 at `x=-12594, z=-340` changed 22,837 of
1,048,576 pixels; the foothills still need ground-level review. A fine water audit on
that seed found no sampled open river edges or lake and coast wall flags. Generator
version 45 prevents terrain from version 44 being reused with the adjusted foothills.

The lake near `x=-8240, z=3068` has a conspicuous shore band. At `x=-8304, z=3108`,
the surrounding terrain detail would place ground about 20 blocks below the lake, so the
shore hold raises it to contain the water. Previously the hold and calming faded within
one map cell, leaving a steep bank. A wider lake-only falloff spreads the transition.
Along the sampled transect its largest four-block rise near the old transition falls
from about 9 blocks to under 2. A ground-level appearance review remains needed.
Fill-spill routes water on the map before block-scale detail is applied; it cannot see
these later gullies. The wider fade reduces the abrupt correction but does not make
lake routing account for detailed terrain. Generator version 46 invalidates version-45
terrain and saved edits.

## Landform layer

Block-scale detail used to read the mountain biome, and the biome is made from climate, which lakes and rivers wet. Lakes were therefore fixed before the detail that shapes their shores could be known, and fill–spill could not see the gullies that later cut under the water.

`landform::run` now classes each cell from the ground alone (height, nearby peaks, slope) and stores it in `WorldMap::landform`, after the final hydrology and before climate. The mountain biome is made from it, except where the climate says glacier. `roughness_layer`, `range`, mountain relief and terracing read the landform, never the biome. Dunes are the one climate-driven shape left, added after the landform ground in `ground_dry`. The desert (0.6) and glacier (0.5) roughness multipliers are gone, so steep desert and glacier ground is now as rough as other ground, and glacier peaks are no longer exempt from mountain relief. These changes have not been reviewed visually.

`landform_ground_ignores_climate_and_biome` scrambles biome, moisture, temperature and trees on a copy of a map and requires identical landform ground at 400 points. Pointing the mountain cover back at the biome makes it fail. On seeds 1, 42 and 300059728 the landform and the old mountain biome differ on 0.6 to 1.3% of mountain cells. Generator version 47 invalidates earlier maps.

Fine water audits on seeds 1, 42, 1337 and 300059728 found no lake or coast walls or steps, no open river block edges and no uncontained map cells. Tall river walls were 0, 1, 20 and 0, as before. Map-level hydrology is unchanged by this step, since it still reads the 32-block height alone.

Lake shores now use a distance field instead of a count of cells (see [water.md](water.md)), and generator version 48 invalidates version-47 maps. The 36-cell scan and the outer-band weighting are gone.

Two measurements decided what this does and does not fix. First, taking the lowest block-scale ground in each map cell as the rim, as a way to let fill–spill see detail, would collapse lakes. On the cells ringing each lake, that lowest ground is more than 2 blocks under the lake level for 13,359 of 32,011 cells on seed 300059728 (8,503 of 29,356 on seed 1, 4,206 of 18,519 on seed 42), more than 10 blocks for 6,415, 3,273 and 1,615, and more than 25 blocks for 1,938, 1,084 and 729. A per-cell minimum cannot tell a closed pit from a channel that drains the lake, so that route needs block-scale connectivity near each lake. Second, the shore hold still builds ground up. Walking outward from every fifth lake edge in 4-block steps, 691 of 2,822 walks on seed 300059728 (336 of 2,691 on seed 1, 157 of 1,592 on seed 42) have the hold raise ground more than 3 blocks above the unheld detail ground, the 99th percentile raise is 32 to 35 blocks and the largest 64 (seed 1). The distance field changes the abrupt-step count by 7 or fewer per seed against the merged wide fade (573 against 569 on seed 300059728), so it is a simplification, not a fix for the banks.

Two more levers were tried on the same walks and reverted, since neither moved them. A freeboard, cutting each lake's pass down so the lake stands under its old brim by `30 * roughness^1.5` blocks (capped at half its depth), left lake area and the raised-walk share within about 10% for four strength settings. Most lakes are held under their brim by evaporation, so the brim is not what limits them. Raising the minimum lake depth from 4 to 8 and 12 removed lakes (seed 1 fell from 78,794 to 74,880 and 62,727 lake cells) without lowering the share of raised walks (12.5%, 14.8%, 17.2% on seed 1). The build-up is not a shallow-lake problem. Rim ground sits under the lake level by a median 0.5 to 7.8 blocks and a 90th percentile of 2 to 32, rising with the map's roughness (0 to 0.2: 2.1, 0.2 to 0.4: 5.6, 0.4 to 0.6: 10.1, above 0.6: about 31).

The remaining candidate is a graded floor instead of a flat hold: ground near a lake may not fall under a surface rising from the lake at a graded stream slope, so gullies bottom out on a ramp. It needs a range limit with its own transition, so it has not been built. In-game review of the lake near `x=-8240, z=3068` found it acceptable, but the numbers above say the hold is still doing a lot of work. Raising `map_cell` to 16 was tried on seeds 1, 42 and 300059728 and does not remove the mismatch. It took 26 to 51 s to generate against 4 to 11 s, used about 1.7 GB in the viewer, and the share of raised lake banks rose.

## Bedded terraces

Mountain terraces were a staircase of the height alone: step sizes drawn per 2400-block square, patches of them by noise, tied to no rock. They are gone. A sedimentary province now holds beds (`geology::bed`): about 160 blocks thick but each boundary wanders by up to a quarter bed (80-240 thick), and each bed is clay (20%) or hard rock by a hash of its number, so the stack never repeats on a fixed cycle. The hard beds are limestone where the `facies` layer (noise, 9000-block wavelength) is high and sandstone where it is low, so about 60% of sedimentary ground is dominated by one or the other, with clay between. `rock_at_cell` reads the same beds, so erosion and the terraces agree on where each rock is.

`WorldMap::terraced` steps the mountain's ground through the beds at its height: hard beds as benches ending in a face (the face climbs over 70% of the bed), clay as a slope (95%). It is a function of height and the slow drift of `strata_offset`, so rising ground climbs through the steps and never falls back (`bedded_ground_climbs_through_benches_and_faces_and_never_falls_back`). The `bedded` layer, a box blur of the sedimentary province over 4 cells either side, fades the terraces out over about 290 blocks at a province's edge, so no cliff stands on a border. Volcanic and metamorphic provinces were massive and unterraced at first (50-75% of mountain ground on the four audit seeds); see "Lava flows and no more scarps".

Generator version 49. The new layers change the erosion, so every world changes, and the seed 300059728 lake bank pin (`lake_bank_eases_into_deep_adjacent_gully`, a coordinate) is gone since that spot is no longer a lake. Hard-rock faces climbing over 40% of a bed (cliffs, as the old terraces' 35%) leaked at LOD seams in `test_nothing_is_seen_through_a_seam_between_lod_levels` (2 of 5152 rays at levels 2 and 3; 50% leaked 1; 60% leaked 6 of 12096 at levels 3 and 4; with no terraces none leak), since a skirt is only three cells deep. 70% passes, and is the steepest tried that does. Cliffier faces needed the seams to cope first, see "Steeper cliffs and deeper LOD skirts". Fine audits on the four seeds found no lake or coast walls or steps and no uncontained cells. These changes have not been reviewed visually at ground level.

## Rock blocks

Rock reaches the blocks now. `TerrainSource::rock_column(x, z)` returns the rocks down a column as beds (`RockColumn::bed`: the run of blocks that are one rock, and the block). The chunk and LOD generators draw every stone as that rock, so limestone, clay, basalt, marble and (existing) sandstone blocks show in cliff faces, benches and the stone under soil. Province edges use the same wandering offset as biome edges (`WorldTerrain::wandered`), so they don't follow the map's cell squares. Generic ground, and any source with no geology, stays `Stone`.

Compacted stone follows the rock: each rock has a compacted block (`BlockType::compacted`), unbreakable and a darker tile derived from the rock's tile, so beds show in cliff faces, which are almost all compacted. The depth rule is unchanged and independent of the rock (6 blocks, 16 under snow); only the look follows the geology. The world floor stays plain compacted stone.

New tiles: Limestone, Clay, Basalt, Marble, and five compacted ones, inserted before the flora (so the flora stay the atlas's last tiles). The four rocks were generated with the block-textures skill (limestone candidate 1, clay 2, basalt 1, marble 2) and the compacted ones derived by `shade`. `out/` is scratch, so `pack.py` only writes the tiles in it.

A cross-section of seed 300059728's mountains at x=-18000..-17400, z=-8080 shows compacted clay, limestone and sandstone as separate bands. Generator version 50.

## Mountain cover and coherent provinces

A mountain's ground is steep nearly everywhere (median block slope about 2.4 on seed 1692045686), so choosing blocks by the real gradient made about 85% of mountain tops bare. Mountain blocks are now chosen by half the gradient (`MOUNTAIN_EASE`), so grass and snow are the cover and bare rock is the steepest faces plus outcrops (`relief::outcrop_patch`: noise patches about 30 blocks across, a third of the ground, each adding 1.0 to the eased gradient). The slope that turns to rock falls with height (1.6 at 150 blocks over the sea to 1.0 at 600, `biome/mountain.rs`) and the old flat 260-block rock line is gone, so peaks are rockier but never bare for height alone. About 55% of the mountain at (1000, 11600) on that seed is rock from above, with exposed dirt where the cliff pass cuts through grass. Chunk generation also chose its surface blocks by the raw gradient, so meadow patches only reached trees and plants. It now uses the same eased gradient.

A range used to show marble, basalt and limestone together, since the province follows the tectonic boundary type cell by cell. `geology::smooth_provinces` gives each cell the commonest province within 40 cells (about 2.6 km), so provinces are blobs several km across. Facies only chooses between limestone, sandstone and clay inside a sedimentary province, so it was not the cause. The change moves erosion, so generator version 51. Audits on five seeds found no uncontained cells and no lake or coast walls or steps. River walls and steps were in the baseline too (seed 1337: 125 walls and 1747 steps before, 90 and 791 after; seed 300059728: 75 and 1864 before, 88 and 2804 after). `test_a_lone_tree_is_still_there_in_every_cell_under_sixteen_wide` found no interior lone tree on test seed 5 after this, so it uses seed 8.

## Lava flows and no more scarps

The foothill scarps (a height-only staircase of 12-34 block steps in noise patches, tied to no rock) are gone. Bedding now reaches the foothills instead: terraces fade in from 25 blocks over the sea (was 40) to full at 110 (was 130). Volcanic provinces get the same treatment with lava flows, which stack thin: layers about 50 blocks thick (`FLOW`, boundaries wandering as the beds' do), all basalt, faces climbing over 70% of the layer. A `flowed` layer, a box blur of the volcanic province like `bedded`, fades them out at its edge, and `terraced` applies beds and flows one after the other, each weighted by its own layer, so a border between the two has no jump. Both steps are functions of height and the slow drift, so the ground still never falls back (`lava_flows_step_the_ground_thinner_than_beds_and_never_fall_back`). Metamorphic and generic ground stays unterraced, with the foothill gullies and ridges still cutting it.

Profiles across the basalt range of seed 1692045686 show benches (runs of 250 to 300 over several samples) between faces. Generator version 52 (the new layer is saved). Audits on five seeds changed nothing at coasts, and lake raised banks moved by a few (seed 1: 87 to 78, seed 300059728: 202 to 204); river counts were identical, so the new steps stay out of the valleys. The seam test passes.

## Granite and shale

Shale is a soft bed: of the 20% of beds that are soft, half are shale and half clay (a second hash of the bed number), so the stacks have a dark, flaky rock between the hard benches. Its face climbs over 85% of a bed (clay 95%, hard rock 70%), erodes and cuts a little less than clay (0.95 and 1.3 against 1.0 and 1.5).

Granite is the core of a metamorphic belt: a metamorphic cell uplifted over 450 blocks becomes a fifth province (`INTRUSIVE`, saved as geology byte 4) after the provinces are smoothed, leaving marble on the belt's margins. It is the hardest rock (hydraulic 0.45, stream 0.55, basalt 0.55 and 0.65) and massive, so no terraces. Both draw as their own blocks with compacted forms and tiles (granite candidate 1, shale 2; the compacted ones derived by `shade`), the atlas now 35 tiles with the flora still last. The soft share and the harder cores move erosion, so generator version 53. Audits on five seeds: no uncontained cells and no lake or coast walls or steps, river counts moved with the new worlds (seed 300059728 tall walls 1 to 40, seed 1337 20 to 7). The one 14 block river wall of seed 300059728 probed at (-18144, -15460) is a bank, a levee built up 10 blocks beside the river, which the audit reads as a wall on its far side.

## Steeper cliffs and deeper LOD skirts

A skirt (the wall a LOD node hangs from its edge so a coarser neighbour's lower ground leaves no crack) was three cells deep, which capped hard-rock faces at a face over 70% of a bed. Its depth now follows the ground: `LodBlocks::skirt_cells` gives 3 cells plus twice the steepest rise to a neighbouring column in cells, up to `MAX_SKIRT_CELLS` (40), so slopes of 10 (faces of 80 degrees, seen at a face over 25% of a bed) are walled. `known_empty` and the skirt's light search use the maximum. With the old fixed three, a face over 40% of a bed leaked 4 of 5152 rays at levels 2 and 3.

The seam test needed a fix: it loaded only the fine and coarse nodes, so a ray grazing down a steep face, below the quantized surface and parallel to it, left the loaded area before meeting ground and counted as a leak (34 of 12096 at a face over 40%, none of them near the seam). It now loads the nodes either side along the seam too, and takes the node's whole z extent. It passes at hard rock over 15% of a bed. Hard rock and lava flows are now 40% (faces about 2.5 times as steep as before), shale 60%, clay 95%.

`bench` LOD total 240.7 ms against 235.0 before (2%), 460305 vertices against 456984 (0.7%), chunk generation and meshing unchanged. Audits on five seeds: no uncontained cells, no lake or coast walls or steps; no change at coasts, lake raised banks moved by 10 to 60 as the cliffs reshaped lake edges (seed 300059728: 204 to 148). The faces change the ground the water is worked out over, so generator version 54. These cliffs have not been reviewed visually at ground level.

## Sheer walls, aprons and thin soil

A bed climbed its whole thickness over 40% of itself, which on gentle ground stretched a 160 block face over 200 blocks of ground, so cliffs read as many small steps. Hard rock and lava flows now climb over 10% of a bed (shale 30%, clay still 95%): at (3200, 12000) on seed 1692045686 a wall of about 90 blocks rises in 14. Each face is footed by a scree apron (`TALUS`): below the riser the ledge climbs 30% of the non-riser share, squared, so it steepens into the cliff and the ground never falls back. Soft beds are 35% of a stack (were 20%) so walls alternate with slopes, and fade to none between 150 and 450 blocks above the sea (`SOFT_FADE`, read at a bed's bottom so a bed keeps one rock), as peaks are capped by hard rock and not clay. Beds therefore depend on height, so `bed` takes the stack's phase and the shift invariance of a stack holds only below 150.

Soil follows the slope (`soil_depth`): on ground under gradient 0.3 one block of dirt lies under the grass before the rock, thinning to none at gradient 1.0, so a mountain's grass sits on bare rock. Sand and gravel keep 3 blocks. The compacted rock tiles darken their rock by 15% with no tint (a cool tint had turned compacted limestone grey), `CompactedStone` keeping its cool one.

Audits on four seeds: no uncontained cells and no lake or coast walls or steps; river tall walls moved with the worlds (seed 300059728 40 to 5, seed 1 0 to 6). The lake-bank probe of `mountain_ground_far_from_a_river_keeps_its_own_height` moved 50 blocks from a river levee the terraces had put it beside. Generator version 55. `bench` was not run for this change.

## Desktop freeze during first launch

Launches that generated the map inside the running game froze the desktop (pointer moves, no window responds, gnome-shell has to be killed). A launch that loaded a cached map did not. On 2026-11-01 two generating launches (11.6 and 11.7 s) froze and one cached launch (245 ms) did not. The journal shows gnome-shell, not the GPU, failing: at the start of each freeze it logged thousands of "Attempting to run a JS callback during garbage collection" errors (offending signal `notify` on `MetaStage`), and none during the launch that worked. The game's `DeviceLost` and panics followed when gnome-shell was killed. This is a correlation from three runs, not a proven mechanism, and no kernel GPU errors were logged.

The sandbox read the map in a `Startup` system, so its window existed while the main thread was blocked for the whole generation. It now calls `load_world` in `main` before the app and window are built, as the headless tools already did. Capping generation at two threads fewer than the logical CPUs did not stop the freeze. To test the fix, force a fresh generation with `OCHRE_WORLD_DIR=$(mktemp -d)`. One fresh-generation launch after the change (11 s, seed 300059728) did not freeze and logged no shell errors, and the lake near `x=-8240, z=3068` was judged acceptable in a ground-level look. One launch is not proof that it always holds.
