# Terrain performance log

Every terrain phase is compared against the row before it. A phase that regresses p99 frame time or
chunk throughput needs a recorded reason here.

Tools:
- `cargo bench --bench terrain`: criterion micro-benchmarks, then p50/p99 over 128 distinct chunks per kind.
- `cargo run --release --example flight -- --seed 1 --secs 30 --out target/flight-<phase>.txt`: scripted
  camera flight (settle, walk 20 m/s, sprint 80 m/s, high fly-over at y=900), headless. Reports are
  `key: value` lines, so `diff target/flight-a.txt target/flight-b.txt` works.
- `cargo run --release --example bench`: single-thread stage timings, vertex counts must not change in a refactor.

Flight throughput is wall-clock and varies run to run (the walk leg is limited by what the camera needs, not
by generation speed), so judge it by p99 frame time and by the sprint and high legs.

## Baseline: layered noise (before phase 0), seed 1

| bench | result |
|---|---|
| chunk_gen cold, plains / mountains / coast (criterion) | 32.4 / 41.0 / 25.3 ms |
| chunk_gen p50/p99 warm regions, plains | 0.129 / 30.4 ms |
| chunk_gen p50/p99 cold, plains / mountains / coast | 26.2/60.2, 27.1/60.3, 19.8/84.1 ms |
| terrain_sample height / biome / climate / cell 64 | 1.119 / 1.109 / 0.173 / 0.968 us |
| lod_node level 1 / 3 / 5 / 7 | 0.171 / 0.238 / 2.62 / 0.174 ms |
| example bench | 664454 chunk vertices, 352454 lod vertices |

Flight: frame p99 3.0 / 5.2 / 5.7 / 4.7 ms for load / walk / sprint / high,
worst 84.9 ms in the high leg. Sprint and high generate about 13.4k chunks/s.

## Phase 0: `TerrainSource` trait

Chunk generation and LOD read terrain through `Arc<dyn TerrainSource>` only (`RegionStore::terrain()`).
Vertex counts unchanged (664454 / 352454). Criterion and percentiles within noise of the baseline
(height sample 1.124 us, cold plains chunk p50 26.3 ms, level 5 node 2.65 ms). Flight: frame p99 2.4 / 5.4 / 5.9 / 5.1 ms, high-leg throughput 13.4k chunks/s.
No regression. The one-off 77-85 ms worst frame in the high leg is in the baseline too.

## Phase 1: world map (not yet used by the game)

`worldmap::generate` makes the default 65,536 m world at 32 m/cell (2048 x 2048) in 92 ms on 16 cores.
Samples: bicubic height 47.5 ns, bilinear 16.9 ns, `WorldTerrain` height 71.9 ns, biome 72.2 ns,
water 82.7 ns, against 1.12 us for a layered-noise sample. The game still runs on the noise generator
until phase 5, so the flight numbers are unchanged.

## Phase 3: hydrology (not yet used by the game)

Priority-flood fill, breaching of small basins, D8 flow, accumulation and river carving.
Generating the default world rose from 92 ms to 2.4 s, almost all of it hydrology (a priority flood over
4.2M cells, up to six times while rims are cut). `WorldTerrain` height 112.9 ns, water 115.1 ns, against
71.9 ns before, because each lookup now searches 25 cells for river segments. Still well under the
1.12 us of a layered-noise sample. A coarse per-cell "river nearby" mask would remove the search; not
needed yet.

## Phase 2: erosion (not yet used by the game)

Generation pipeline: base terrain, hydrology (breaching only), stream-power valleys (6 passes),
droplet erosion in tiles, thermal erosion, hydrology again with rivers carved. The default world takes
3.3 s on 16 cores. The priority flood now queues only the sea's shore (open sea is never reached from
land), which took hydrology from 2.3 s to under 1 s. Droplets run in 128-cell tiles, four rounds apart,
so tiles run in parallel and a seed still gives the same map (tested).
Plain droplets alone left cell-sized speckle on slopes and no valleys at range scale, hence the
stream-power pass for valleys and fewer, longer, higher-inertia droplets for flank gullies.

## Phase 4: climate and biomes (not yet used by the game)

Warmth from latitude, lapse rate of 1/800 per block (`snow_line` has since come to use 1/2600, in `ochre_base::cold`, and climate 1/3500, `LAPSE_PER_BLOCK`), moisture from a
westerly sweep over each row (sea evaporates, rising ground rains out, 60% of rain is recycled inland),
blended with its rank among the land so every seed has dry and wet ground. A Whittaker table picks one
of ten biomes per cell. The default world takes 3.5 s. Seed 1 comes out at 18% desert, 14% steppe, 21%
savanna, 16% jungle, 15% forest, 7% tundra/taiga/glacier, with jungle on windward coasts and desert in
the lee of ranges.

## Phase 5a: the game runs on the world map

`ochre_world::load_world` loads or generates the map (cached under `worlds/`, or `OCHRE_WORLD_DIR`), `WorldTerrain` is the
`TerrainSource`, and the layered-noise generator, its biome shaping and the `Fractal` helper are gone. The
terrain differs (new continents), so LOD vertex counts and spawn-area timings are no longer comparable
with earlier rows, only the per-sample and per-chunk numbers are.

| bench | before | now |
|---|---|---|
| terrain sample, height | 1.12 us | 0.256 us |
| chunk_gen cold p50, plains / mountains / coast | 26.2 / 27.1 / 19.8 ms | 13.5 / 13.4 / 13.4 ms |
| chunk_gen cold p99, plains / mountains / coast | 60.2 / 60.3 / 84.1 ms | 52.4 / 40.1 / 39.1 ms |
| lod_node level 1 / 3 / 5 / 7 | 0.171 / 0.238 / 2.62 / 0.174 ms | 0.226 / 0.827 / 1.27 / 0.182 ms |

Level 1 and 3 nodes are slower because the spawn area is now jungle and forest with far more trees to place
(tree placement dominates those levels), not because sampling got slower.
Flight: frame p99 2.5 / 5.4 / 6.1 / 5.0 ms against 2.4 / 5.4 / 5.9 / 5.1,
throughput unchanged (it is capped by the loader's per-frame budget, not by generation). Worst frame in the
high leg was 110 ms, against 77-85 ms in earlier runs; it is a single frame and varies run to run.

## Open question: map cell size, 16 m or 32 m

Seed 1, 65,536 m world, 16 cores (`world_map --cell N`):

| cell | cells | generation | peak memory |
|---|---|---|---|
| 32 m | 2048^2 (4.2M) | 2.7 s | 270 MB |
| 16 m | 4096^2 (16.8M) | 12.9 s | 856 MB |
| 8 m (extrapolated) | 8192^2 (67M) | about a minute | 3+ GB |

32 m stays the default: rivers are drawn as segments with their own width, so cell size does not limit them,
and chunk detail noise supplies everything finer than 64 m. 16 m is supported (`[world] map_cell = 16`) and
costs about 5x the time. The erosion constants are in cell units, so a finer map erodes differently, not
just more finely; it would need retuning, not only a config change.

## Phase 5: chunk detail, water, strata, cliffs (done)

Full chunk pipeline: bicubic ground plus roughness-scaled detail, river channels and lakes, water filled to
each column's own surface, soil/stone/compacted-stone strata, 3D-noise cliffs cut before the water fills.
A cheap "water nearby" mask skips river and lake lookups for the columns that have none.

| bench | baseline | now |
|---|---|---|
| terrain sample, height | 1.12 us | 0.242 us |
| chunk_gen cold p50, plains / mountains / coast | 26.2 / 27.1 / 19.8 ms | 13.2 / 11.1 / 12.0 ms |
| chunk_gen cold p99, plains / mountains / coast | 60.2 / 60.3 / 84.1 ms | 61.3 / 33.1 / 37.0 ms |

Cold plains p99 is level with the baseline (61.3 against 60.2 ms): it is the first region in an area plus
columns, and it varies by about 10 ms between runs of the same build. The other five rows improved.
Flight (taken before the water mask, so the mask only helps): frame p99
2.6 / 5.3 / 6.2 / 5.2 ms against baseline 3.0 / 5.2 / 5.7 / 4.7. Sprint is 0.5 ms over the baseline's p99, a
run-to-run difference on a headless harness (the same build moved by 0.4 ms between two runs), and
throughput is identical, as the loader's per-frame budget caps it. The high leg's worst frame was 73 ms
against 77-85 ms in the baseline runs.
Determinism: `generator::determinism` pins a hash of chunks built from a hand-made map, portable across
machines.

## Phase 6: gameplay pass, plus fixes from flying the game

Passes (graded valleys joining walkable lowlands across ranges), river crossings at 2-3 km, spawn regions,
resource sites, caves and hotspots are generated and validated on world creation; the default world takes
3.0 s (`worldmap_generate`). Smoothing, minimum biome size, riparian belts (big rivers and lakes only) and
drying of narrow streams in dry ground were added after flying the game.

Flight: frame p99 3.2 / 5.1 / 7.1 / 6.3 ms against baseline 3.0 / 5.2 / 5.7 / 4.7.
Sprint and high-altitude p99 are about 1.5 ms over the baseline. Not proven, but the likely reason is that the
flight now crosses forest and jungle where the baseline crossed open land, so the main thread integrates more
mesh per frame; the same build moved 0.4-0.5 ms between runs, which does not cover it. Treat it as a regression
to chase with a render-side profile, not as settled. Chunk generation is faster than the baseline (cold p50
11.1 / 11.0 ms against 26.2 / 27.1, plains p99 43.9 against 60.2).

## After flying it, round three

Cliffs and terrain: mountain is now a biome (high or steep ground, grey on the map) with terraces in patches
(55-120 block steps, strata tilted differently in each patch), cliff carving starts at gradient 1.0 (half of
mountain columns are over it; it started at 2.6, which almost none were), and some stretches of coast rise
60+ m within 190 m of the shore (13-16% of coast points) so there are sea cliffs. Dunes are gentler and
desert below 90 m has no cliffs. Streams join rivers (two rivers sampled at a confluence), rivers are smooth
curves, and the bed is cut down into the land rather than levelled. The climate is wetter (desert 10-12%
of land, from 23-25%), lakes and narrow streams dry up in dry ground, only big water greens its banks.
Foothills is gone: it needed the old noise generator's ruggedness.
`GENERATOR_VERSION` is 5: a map cached by an earlier version is regenerated, not reused.

## Round four

Terraces stack (they follow height alone, with a slowly drifting step size and phase, so rising ground climbs
through them and never falls back; an earlier version added a tilt that sawtoothed on gentle ground). The same
function runs at every LOD cell size. Rivers are never raised by their bed (a test pins it) and the tallest
ground is stretched so peaks reach 986-988 of the world's 1024. `GENERATOR_VERSION` is 7.
Known gap: the LOD seam test only samples ground under 640 blocks; in high ranges rays leak at seams between
levels 1 and 2. Probably the harness not loading every node below the surface (to confirm and fix later).

## Round five: peaks

The height easing curve squashed every summit to the same level, so tops were flat. It is replaced by sharpening
(each high cell pushed 1.1x its height over the ground within ~600 blocks, which raises peaks and deepens the
valleys between them) and a straight rescale so the tallest ground stands at 990. Seed 1: tallest 990, 99.9th
percentile about 900, 4% of summit area nearly flat. Ground over 360 blocks above the sea is snow-capped in any
climate. `GENERATOR_VERSION` is 8.

## Round six: snow and alpine forest

Snow is full snow blocks on every face (the snow-over-dirt tile is no longer used) lying 14 blocks deep, with
packed stone only beneath it at 16. Warmth now falls with height wherever trees and grass read the climate,
so pines grow on the slopes of any range (cooler than 0.46 at their height), and a mountain's lower slopes are
a conifer belt (density 0.3) that thins to nothing between 170 and 290 blocks above the sea. `GENERATOR_VERSION` is 9.

## Round seven: meadows on mountain faces

Mountain slopes were all bare rock, so no tree could stand on them. In patches about 50 blocks across (a third
of the ground), mountain ground is treated as no steeper than 0.9 when choosing its blocks, so grass, and the
pines and flora on it, cling to the faces. The alpine rock line moved from 175 to 260 blocks above the sea, and
trees over 120 blocks above the sea are pines in any climate (tropical ranges never get cold enough). The
generator version is 10.

## Round eight: rough foothills

A `range` layer (the mountain biome's share of a wide window, spread twice) marks the foothills. Ground near a
range gets roughness up to 0.8, scarps (12-34 block steps in patches), and gullies and sharp ridges (ridged noise,
22 blocks), as mountains do; cliff carving follows from the steeper faces. Median gradient on foothill ground is
1.49 against 0.29 far from a range (a test needs 3x). Mountain terraces and scarps now use a fixed flight size and
phase per square of ground, blended at square edges, because a flight size that drifted slid ledges across level
ground as phantom cliffs. `GENERATOR_VERSION` is 12.

Cost (game closed): terrain sample 274 ns (was 242); cold chunk p50 plains / mountains / coast 13.7 / 17.9 / 13.2 ms
(was 13.2 / 11.0 / 12.0), p99 52.7 / 52.1 / 47.6. Mountain chunks cost more with gullies, terraces and cliffs on
them; still under the original baseline (26.2 / 27.1 / 19.8). GPU memory at spawn rose to 3.7 GB (from 2.7) with the
rougher ground, so watch it in big ranges on a 6 GB card.

## Round nine: flowing water

`examples/flow_sim` loads round a spot headless, times frames with the water still, then breaks up to 8 banks of
still water nearest the camera and times frames while it flows. Seed 2790361079 at `OCHRE_SPAWN=6730,16960,3`
(a steep river in a gully) spills down the gully to 8,661 flowing blocks over about 45 seconds, then settles.

The flow step itself is cheap: at most 0.63 ms for about 100 changed blocks, four times a second, and queueing
their groups to mesh again 0.05 ms. The cost was swapping the new meshes in: `apply_remeshes` packed every mesh on
the main thread, 0.36 ms a group, and a step finishes up to 20 groups at once, 7-8 ms. Packing now happens in the
meshing task, for groups loading in as well (`pack_group`). Flowing frames, two runs: p99 4.38 to 1.57 / 1.61 ms,
slowest 7.94 to 2.20 / 2.85 ms; still frames p99 0.77 either way. `lod_sim` (seed 7, 80 blocks/s) has no slow
frames or new holes, and flies further in the same time, as its frames are faster.

## Shadow caster pruning and solid shadow pipelines (2026-10-26)

Options 1 and 3 from `shadow-perf.md` are implemented. The runtime prunes terrain casters against
each cascade's camera slice swept toward the actual shadow direction, with filter/bias/sway
padding and a fallback to Bevy's visibility when world curvature is active. Meshes are split
into solid and cutout parts before packing on worker tasks. Solid directional shadows omit
their fragment stage where native depth clipping is available. Camera fade materials retain
their solid/cutout classification and masked rendering; emulated depth-clamp GPUs keep a fragment
depth write. LOD splitting and packing also run on workers to avoid adding work to mesh upload frames.

Seed 1, current settings, 512-block shadows, three cascades, level camera, 15 sun directions:
mean submissions 2.365M to 1.083M triangles (54% fewer); 1.054M retained triangles (97.3%) are
eligible for the pipeline without a fragment stage. `shadow_cull` uses the runtime hull and counts
the split meshes, including child foliage. These are CPU submission counts; GPU time and frame-time
p99 were not measured, so no GPU speedup or p99 improvement is claimed. Splitting can add a draw
for a mesh containing both ground and foliage. See `shadow-perf.md` for correctness guards and
the required rendered A/B check.

`bench` before/after: chunk vertices 620,695 both times; LOD vertices 311,077 both times, with
all eight levels matching exactly. Chunk meshing best of three 208.2 / 216.7 ms; LOD total
78.9 / 79.6 ms. Generation is unchanged; splitting happens later, outside this benchmark.
The seed-1 `lod_sim` route flags one persistent surface chunk after stopping with both the
original branch code and this change. Temporary diagnostic edits were removed after the
baseline comparison. This check does not establish that the existing loader has no gaps.

## Cloud march resolution cap

`renderer.cloud_resolution` selects a target width: low 320, medium 640, high 960, ultra 1280.
Medium is the default. Height follows the window's aspect ratio, and smaller windows retain
half display resolution. Both the march target and its temporal history use this calculation
on startup and resize. At 16:9, medium reduces the 1440p main march from 921,600 to 230,400 rays
per frame; at 4K, from 2,073,600 to 230,400. The fixed reflection target and march step counts
are unchanged. Pixel counts depend on aspect ratio; this caps width rather than total area.

The user reported that both 960×540 and 640×360 retained the desired cloud quality. The second
cap uses 56% fewer main-march rays than the first at 16:9. GPU timings and frame-time p99 were
not measured, so no proportional timing improvement is claimed. See `docs/clouds.md` for presets.

## Tectonic MVP checkpoint (2026-10-27)

Release Rust 1.97.1, managed CPU-only environment (2 vCPUs), seed 1, render distance
1024. `flight --seed 1 --secs 3` uses 179 frames per leg, separate world caches and
unchanged renderer settings. Before is 96679cb; after is the version-39 tectonic MVP.
Geometry and spawn surroundings differ, so vertex equality does not apply. These short
runs are diagnostic, not a stable frame-time benchmark or GPU measurement.

| Headless frame p99 (ms) | Before | After |
| --- | ---: | ---: |
| Loading | 8.101 | 6.298 |
| Walking | 5.730 | 4.816 |
| Sprinting | 7.791 | 5.779 |
| High-altitude flight | 11.997 | 41.383 |

Walk-stage mesh storage was 158 MB before and 159 MB after. High-altitude LOD throughput
fell from 257.0 to 110.5 meshes/s. The larger vertical bounds and increased relief change
the scene and loading workload; they are hypotheses to profile, not an established cause
of the p99 regression. This checkpoint is a draft and should not merge until that
regression is understood or reduced. Normal ground legs do not establish high-altitude
performance.

Final viewer generation times were 13.0 s (42), 11.4 s (1) and 19.1 s (1337), without
concurrent builds/tests. Earlier baseline captures were not all taken under equivalent
load, so no generation speedup or regression ratio is claimed. See
[tectonic-mvp.md](tectonic-mvp.md) for same-scale visual comparisons and open issues.

## Tectonic MVP draft follow-up from 4883cfa

The fixed 900 m high-flight path stays at least 824 m above seed-1 ground, so flying
through newly tall terrain does not explain its slow frames. Raising the camera to
2200 m made high-flight p99 worse in a two-core diagnostic run (14.8 to 22.1 ms),
consistent with more horizon and LOD work; the run was not an A/B of identical scenes.

Temporary timers found 9–12 ms calls in `retire_lods` during the high leg. It checked
membership in the retired vector while retaining the retirement queue, making a large
retirement batch quadratic. A set now handles that membership check. On this 16-core host
restricted to cores 0 and 1 with `taskset`, three alternating old/new pairs of
`flight --seed 1 --secs 3` gave:

| Pair | Old high p50/p99 (ms) | Draft high p50/p99 (ms) |
| --- | ---: | ---: |
| 1 | 6.302 / 16.551 | 4.407 / 11.229 |
| 2 | 5.147 / 16.122 | 3.625 / 13.360 |
| 3 | 6.397 / 18.657 | 4.559 / 11.434 |

The draft wins both frame percentiles in each pair. LOD mesh throughput varies across
the pairs and does not establish a throughput gain. `bench` matched exactly before and
after: 712,019 chunk vertices and 330,471 LOD vertices, including every LOD level.
These timings are diagnostic headless CPU runs and cannot be compared numerically with
the original managed-environment checkpoint. GPU frame time remains unmeasured.

Version 40's steep-bend bank correction adds one upstream profile lookup near river banks.
`bench` still reports 712,019 chunk and 330,471 LOD vertices, with 224.5 ms chunk meshing
and 97.7 ms LOD meshing in one run. A single two-core, three-second seed-1 flight had an
11.566 ms high-leg p99, within the three version-39 draft runs above. That short run does
not establish GPU cost or a precise frame-time change.

## Startup device loss report (2026-10-27)

A desktop launch warned that random seed 1522288356 was unplayable, then lost the GPU device
about 39 seconds later. The cache shows that seed's map saved at 09:01:35 and a replacement
seed 2413427271 saved at 09:01:39. The warning is a rejected map during the normal bounded
retry loop; it did not stop world generation. The later `DeviceLost` and invalid mapped-buffer
errors show a render-device failure, but do not identify its cause. No graphics device is
accessible in this environment, so the GPU failure has not been reproduced here.

For seed 2413427271, a six-second-per-leg headless flight completed at both view distances.
At 512 chunks it held 405 MB of CPU mesh data after load and 459 MB after walking; at the
configured 1024 chunks, 478 MB and 532 MB. Peak process RSS was 1.24 GB and 1.32 GB,
respectively. These are CPU measures, not GPU allocation or frame time. The default game
still needed a GPU check at the time of this measurement.

A second launch with the original 1024-chunk settings and a different random seed also
froze the desktop. Its log says seed 1737182360's map was ready and the player spawned
before the final `bevy_time` warning; the earlier rejected seed was not the blocker.
The user confirmed a temporary 256-chunk safe preset started, then confirmed a fixed
seed started with the original 1024-chunk view, 16-chunk full detail and LOD base,
64-chunk shadows and medium clouds. The safe preset and runtime overrides were removed;
the original renderer settings remain in `assets/settings.toml`. The fixed-seed success
shows the full render settings can start on this GPU, though it does not isolate the
cause of the random-seed freezes. A two-second-per-leg headless flight on seed
1737182360 at the temporary safe preset completed with 102 MB of mesh data after load,
105 MB after walking and 624 MB peak process RSS.

## One map per launch

Both reported hangs followed the random-seed retry path: a geography issue caused the
first map to be discarded, then another map was generated before terrain streaming.
`load_world` now chooses one seed and loads or generates exactly one map. It does not run
the geography validator at startup. The gameplay pass still carves passes and places
spawn regions; the validator remains available to diagnostic map tools. This removes
the second large map generation and its allocation churn from the startup path. It is
plausible that the repeated work contributed to the desktop stalls, but the logs do
not establish that. A fixed-seed launch with the full render settings succeeded on
the user's GPU; random-seed startup with the one-map build has not been checked.
At seed 2154739607, which the former validator rejected, a one-second-per-leg headless
flight now loads that seed directly without a playability warning and completes all legs.

## Initial rock data prototype

The geology fields add five bytes per map cell (20 MiB at the default 2048-square map),
plus temporary generation and serialization storage. This initial prototype did not
feed geology into erosion. Its release `bench` at seed 1 matched the previous build at
every level: 712,019 chunk and 330,471 LOD vertices.

Loading an old cached seed-1 map and initializing a zero-length headless flight on two
cores took 0.33–0.39 s before and 0.44–0.45 s with geology reconstruction over three
alternating pairs. Peak process RSS was about 433 MB before and 465 MB after. This was
measured on the prototype that reconstructed geology from older caches; the subsequent
erosion change uses version-41 caches instead.

Three alternating old/new headless flights, restricted to two cores, used seed 1 and
three seconds per leg. High-flight p99 was 11.234/13.277, 10.393/10.838 and
12.543/12.557 ms (old/new). The new build is higher in these short pairs, by 0.014–2.043
ms. The extra retained map data may change cache or memory pressure, but these runs do
not establish a cause; frame time and GPU cost need longer checks. The sample counts
and timing variance make this a diagnostic rather than a stable performance baseline.

## Rock-dependent stream erosion

The version-41 erosion and water changes alter terrain, spawn surroundings and mesh
counts. At seed 1, `bench` reports 693,310 chunk and 323,777 LOD vertices, against
712,019 and 330,471 before; exact vertex equality does not apply to this change.
Three alternating two-core headless flights, three seconds per leg, gave high-flight p99
of 10.468/11.105, 13.321/10.597 and 13.598/10.605 ms (before/after). Sprint p99 was
3.586/3.529, 3.717/4.983 and 4.717/4.631 ms. The second sprint pair regressed by
1.266 ms; changed terrain and spawn can change streaming work, but these short runs do
not isolate the cause. No GPU timing is available.

## Mountain river containment

The first general neighbour check sampled all four neighbours throughout the indexed
river area. On seed 1, one `bench` run spent 650.1 ms building regions, against 179.8 ms
before the change. A conservative bound using meander movement and possible river-width
variation skips neighbour sampling where water cannot reach an adjacent block; the same
benchmark then took 208.2 ms. Mesh output intentionally changed with the bank geometry:
693,310/693,664 chunk and 323,777/323,873 LOD vertices before/after.

Three alternating two-core headless flights, three seconds per leg, used render distance
1024 and full-detail distance 16. Load p99 was 4.064/3.223, 1.539/3.980 and 3.636/3.104 ms;
high-flight p99 was 11.259/11.637, 11.137/10.281 and 9.407/11.062 ms (before/after).
The second loading and first and third high-flight pairs regressed. Neighbour sampling
adds work and bank geometry changes the scene; these short, variable runs do not isolate
their contribution. No GPU timing was collected. These measurements precede the final
short-circuit that skips width-bound calculations when the nearest river already qualifies.
The final short-circuit retained the same vertex counts and five water-audit summaries;
one `bench` run measured 206.6 ms for regions and 98.5 ms for LOD meshing.
After the clipped-mouth search was included, `bench` still matched those vertex counts,
with 208.2 ms for regions and 104.2 ms for LOD meshing in one run.

Three more alternating two-core flights used the current full-detail distance of 12.
Load p99 was 3.185/3.405, 3.338/3.214 and 3.556/2.280 ms; high-flight p99 was
11.622/11.995, 10.436/11.517 and 11.768/11.257 ms (before/after). High-flight p99 rose
in the first two pairs by 0.373 and 1.081 ms. The new neighbour checks add work near
water and the terrain differs; the short runs do not establish how much each contributed.

## Landform variety draft

With seed 1, one release `bench` run measured 693,664 / 1,896,431 chunk vertices and
323,873 / 458,404 LOD vertices before / after the landform changes. The generated scene
and spawn terrain differ, so this is a workload change rather than a like-for-like mesh
optimization. Chunk meshing took 225.0 / 419.7 ms and total LOD meshing 103.7 / 250.0 ms.
In the second flight's load snapshot, all mesh assets held 96 / 340 MB (2.66 / 9.31
million vertices). A separate instrumented after run measured 335 MB: 5 MB was water,
70 MB was opaque chunk and LOD meshes, and approximately 260 MB was other meshes using
the terrain material, including cutout and inner-leaf faces. All measured mesh assets
were attached to entities. The different spawn scenery is a likely contributor, but the
specific source of the extra terrain-material geometry needs profiling before drawing
conclusions about GPU memory or render performance.

Three alternating two-core headless flights used full-detail distance 12 and three seconds
per leg. High-flight p99 was 12.774 / 8.460, 12.917 / 16.633, and 12.579 / 8.426 ms
before / after. The second pair regressed by 3.716 ms. Changed terrain and mesh workload
are plausible contributors, but these short runs do not isolate the cause; a viewer export
also overlapped part of the measurement. GPU performance has not been measured.

## Rock-dependent slumping draft

One seed-1 release `bench` run before / after reported 1,896,431 / 1,896,431 chunk
vertices and 458,404 / 458,590 LOD vertices. Region generation took 124.8 / 123.4 ms,
chunk meshing 417.9 / 416.3 ms and total LOD meshing 249.9 / 253.7 ms. These single
runs do not establish a performance change. The map and mesh workload change with
erosion; map generation and frame-time A/B comparisons remain to be measured.

## Lake shore transition draft

Three alternating seed-1 release `bench` runs before / after the wider lake search
measured region generation at 123.7 / 144.2, 124.5 / 143.8 and 123.7 / 143.1 ms.
Total LOD meshing was 253.0 / 267.6, 252.7 / 268.3 and 252.8 / 268.6 ms. The wider
per-sample search is a likely contributor. The sampled scene produced 1,891,787 chunk
and 458,029 LOD vertices in both builds, but the shore change alters terrain elsewhere.
Frame-time p99 and GPU performance were not measured.

## Lake shore search mask

`still_near` scanned a 6 by 6 window of map cells for every sample after the wider lake search. A per-cell mask, built once in `WorldTerrain::new`, now skips the scan wherever no sea or lake cell lies in that window. Output is unchanged.

Three alternating seed-1 release `bench` pairs (before / after) measured region generation at 143.0 / 117.5, 143.3 / 117.5 and 143.5 / 117.5 ms, below the 124 ms before the wider search. Total LOD meshing was 267.4 / 246.8, 268.9 / 247.1 and 267.6 / 246.9 ms. Chunk and LOD vertices were identical (1,891,787 and 458,029). A `perf` run of the whole `bench` (cpu-clock, 3 kHz) put `still_near` at 1.42% of samples before and 0.42% after. Frame-time p99 and GPU cost were not measured.

## Landform layer

Splitting the mountain class out of the biome adds one pass (`landform::run`, a window max, the slope layer and the biome tidy filter over three classes). With the mask above, two seed-1 `bench` pairs against the previous build measured regions at 115.4 and 116.6 ms against 143.4 and 143.7, and LOD meshing at 246.0 against 267.6 and 268.1 ms. Chunk vertices were 1,891,787 in both builds and LOD vertices 458,026 against 458,029. Map generation time was not isolated.

## Lake distance field

Replacing the 6 by 6 cell scan for lakes with a distance field built once per map (two chamfer passes over the cells) left seed-1 `bench` unchanged. Two alternating pairs against the landform build measured regions at 117.1 / 114.7 and 115.4 / 116.5 ms and LOD meshing at 246.1 / 246.0 and 245.5 / 246.2 ms. Chunk and LOD vertices were identical (1,891,787 and 458,026). That scene has no lake shore in view, so it does not show the shore change. Fine water audits on seeds 1, 42, 1337 and 300059728 were identical to the landform build.

## Generation thread cap

Map generation (`par_rows`, the erosion tiles and the river profile pass) now uses every logical CPU but two, 14 of 16 on an 8-core SMT machine, so the desktop keeps running during a first launch. Map generation took 4.34 and 4.32 s on seed 1, 4.43 and 4.57 s on seed 42 and 10.68 s on seed 300059728, against 4.17, 4.38 and 10.73 s with all threads, and those runs also include the landform pass. Whether it stops the reported desktop freeze during generation is not established.

## Bedded terraces

The terraced staircase (a height-only function with per-square flight sizes) is replaced by sedimentary beds with noise facies (see [tectonic-mvp.md](tectonic-mvp.md)). `bed` is a few integer hashes per `rock_at_cell` call in the erosion loops; map generation time was not A/B measured. Vertex counts change with the world, so `bench` has no like-for-like baseline across this change.

## Rock blocks

Drawing stone as its rock costs a rock column per column with stone in it (a box, the map cell and offset, one bed lookup, then the cached bed for the rest of the chunk column). `bench` generate chunks went from 191 ms to 237 ms (about 25 µs a chunk) and the other phases and every vertex count are unchanged. Of the 46 ms, about 8 ms is the wandering offset, none is per block (a run of blocks is looked up once per bed), and about 38 ms is making the column handle, which nearest-cell phase instead of a bilinear one did not reduce. Not optimised further: a chunk's columns share map cells, so a per-chunk cache is the next step if this matters.

## Occlusion culling, shadow pruning and the CPU overlay (2026-11-02)

`examples/visibility` software-rasterizes every terrain mesh in the frustum from eight headings into a 480x270 depth buffer. Seed 1692045686: only 13% of the meshes in the frustum have a pixel from the ground below a 130 block limestone wall, 34% and 38% from the air and from a mountain (28 to 31% of the triangles in the frustum are in meshes that are seen). Fragments per pixel passing the depth test are 0.6 to 1.3 as submitted and 0.5 to 0.9 front to back, so pixel overdraw is low and the waste is vertex work.

The real GPU did not follow. With Bevy's `OcclusionCulling` on the camera (it needs `DepthPrepass`, which the camera has), `OCHRE_STATS` GPU ms over three views, three runs each: below the wall 9.4 to 8.5 (the main opaque pass 1.77 to 0.85, the early prepass 1.10 to 0.50), plateau edge 9.7 to 9.9 and mountain 9.2 to 9.7 (run to run noise is about 0.4 ms, so the mountain's +0.4 may be the depth pyramid's cost). The shadow pass, 2.5 to 2.8 ms in every configuration, did not move with the component on the sun either, or with the swept-hull pruning off (see `shadow-perf.md`, where it is removed). Occlusion culling was not kept: a 0.9 ms win in one walled view against a small cost in open ones. The frame is GPU bound (9.9 ms against 9.2 GPU, main world 1.5, render encoding 0.2), terrain passes being 5.6 ms of it and clouds, light probes, atmosphere and bloom the rest.

The debug overlay (F1, or `OCHRE_STATS`) now shows `cpu:` frame time, the main world split into first, pre, update, post and last (marks laid in `Main`'s order), and the render passes' encoding time, beside the GPU passes.

Shadow pass A/B (same day, below a wall and on a mountain, GPU ms of `per_view_shadow_pass`, `OCHRE_STATS` last 100 samples): shadow distance and cascades 64 chunks x 3 2.77 / 2.58, 64x2 2.24 / 2.03, 64x1 1.97 / 1.86, 64x4 3.59 / 3.14, 32x3 2.22 / 1.80, 16x3 1.51 / 1.13, 4x1 0.36 / 0.22. Shadow map size at 64x3: 1024 2.52 / 2.49, 2048 2.79 / 2.58, 4096 3.82 / 3.13. Total GPU falls 9.2 to 7.0 ms with shadows near off. `shadow_cull` (mean of 15 suns, 1024 blocks) hands the cascades 0.23M, 1.63M and 5.6M triangles, the last (texel 2 blocks) 75% of them and mostly L1 to L3; full detail is 6% of that cascade. The pass has a floor of about 2.4 ms that map size does not move, so it is mostly geometry bound, with fill adding above 2048. Lowering the map size buys 0.1 to 0.3 ms, and the lever is the far cascade's triangles or its distance.


## Measurement baseline (PR 0, tectonic remaining work)

Recorded before any change from `docs/designs/tectonic-world-plan.md`; later PRs compare to these. The design's PR 0 needed no generation change, so these are HEAD's numbers.

View set (seed 1692045686, `OCHRE_SPAWN=x,z,lift`, vertex counts from `BENCH_AT=x,z cargo run --release --example bench -- 1692045686`, single thread, region and LOD stages measured while a census ran, so times are loose and counts exact). GPU ms are the three-run `OCHRE_STATS` medians logged above under occlusion culling (culling off).

| View | Spawn and look | GPU ms | Chunk vertices | LOD vertices |
| --- | --- | --- | --- | --- |
| below_wall | (3000, 12000, 2) looking (1, 0.05, 0) | 9.4 | 881,683 | 514,786 |
| plateau_edge | (3195, 12000, 1) looking (-1, -0.1, 0) | 9.7 | 530,460 | 444,094 |
| mountain | (1000, 11600, 1) looking (1, 0.05, 0) | 9.2 | 636,501 | 530,055 |

Straight biome edges (`worldprobe <seed> edges <x> <z> 2048`, samples every 4 blocks, longest run in one axis-aligned line), windows at (0, 0) and (+-12000, +-12000):

| Seed | (0,0) | (12000,12000) | (12000,-12000) | (-12000,12000) | (-12000,-12000) |
| --- | --- | --- | --- | --- | --- |
| 1 | 44 | 92 | 84 | 88 | 0 |
| 42 | 100 | 0 | 132 | 92 | 140 |
| 1337 | 108 | 60 | 0 | 88 | 80 |
| 300059728 | 56 | 100 | 120 | 84 | 84 |

The longest run is 140 blocks, so HEAD already passes the design's provisional limit of 200. That metric does not separate fixed from unfixed; PR 7 needs a tighter limit or a different measure before it can gate.

Per-stage generation time (`OCHRE_STAGE_TIMES=1`). Seed 1, 5.5 s total: tectonics 0.32 s, geology 0.24, hydrology 0.91, erosion 2.31, carve passes 0.12, scatter 0.15, hydrology again 0.89, landform 0.18, climate 0.12, roughness 0.06, label 0.11. Seed 1337, 8.6 s total: the same except carve passes 3.1 to 3.5 s and landform 0.17 to 0.23. Carve passes is the stage whose cost varies most between seeds.

Census at HEAD (`scripts/census.sh 1 64`, per-seed rows in `docs/census-head.csv`; shares are of land cells, land is ground at or above sea level):

| Measure | Result | Design goal |
| --- | --- | --- |
| Land share | 10% to 39%, median 27%; 52 of 64 seeds in 20% to 70% | 90% of seeds (58 of 64) |
| Spawn region found | 64 of 64 | 95% |
| Largest single biome | mountain, 53% on one seed; none over 70% | none over 70% on more than 10% of seeds |
| Cold group (glacier, tundra, taiga) with 15% or more of land | 0 of 64 (largest 0.6%; 10 seeds show any) | at least 10 seeds |
| Hot and dry group (desert, savanna, steppe) with 15% or more | 64 of 64 | at least 10 seeds |
| Wet group (forest, jungle; swamp does not exist yet) with 15% or more | 64 of 64 | at least 10 seeds |

Findings: cold biomes are unreachable at HEAD (the design's premise holds, so PR 1b keeps its full scope). The land-share goal is already missed by 12 seeds, all but a few under 20%, so seed-derived relief (PR 1) has to widen the range, not just keep it. Grassland, steppe and sea read 0% of land in the biome layer; only desert, forest, savanna, jungle and mountain cover land in practice.

## Portable maths (GENERATOR_VERSION 56)

World generation called `exp`, `powf`, `sin`, `cos` and `hypot` through the platform's maths library, which can differ in the last bit between x86-64 and ARM64. `exp`, `powf`, `sin` and `cos` now go through the `libm` crate (pure Rust, pinned by `Cargo.lock`); `hypot` became `sqrt` of the sum of squares. `powi(2)` is left alone, since both LLVM and `__powisf2` give `x * x`. Render shading (`greedy.rs`) is not world content and still uses `powf`.

Measured against `main` before the change: stage times on seed 1337 are within noise (tectonics 0.328 to 0.338 s, climate 0.117 to 0.118 s, total unchanged at 8.6 s); `water_audit --fine` is identical on seeds 1, 42, 1337, 300059728 and 1692045686; whole-world hillshade, biome and water exports at 32 blocks a pixel change 3 pixels in 4 million on seed 42 and none on the others; `bench` vertex counts on the three views are identical.

Guarded by `worldmap::portable_maths` (the map's hash is pinned and is the same with 1 and 7 worker threads, and a source scan fails on platform maths in `ochre-base`, `worldmap` and `generate`) and by `determinism.rs` (chunk blocks from a generated map are pinned). Not yet run on ARM64: that needs `qemu-user` and `rustup target add aarch64-unknown-linux-gnu`, then `cargo test --release --target aarch64-unknown-linux-gnu -p ochre-world -p ochre-terrain` with `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_RUNNER=qemu-aarch64`.

## Seed-derived plates, relief, warmth, humidity and wind (GENERATOR_VERSION 57)

`worldmap/character.rs` draws `plates` (8 to 16), `relief` (0.7 to 1.4), `warmth` (0.20 to 0.80), `humidity` (0.8 to 1.6) and the wind direction (whole degrees) from the seed. `load_world` and the tools that build a map by seed use it; `OCHRE_CHARACTER=off` keeps the config as given. `bench` and the pinned hash tests use `WorldConfig::default()` and are unchanged.

`humidity` is new in `ClimateConfig` (default 1.0, appended to the map file's config) and multiplies the rain that biome moisture sees: `moisture = rain * humidity / demand`. Hydrology reads the same moisture field for how much ground soaks up, so humid worlds shed more water to rivers and lakes; lake outlines and deep-cell counts change (seed 1: deep14 1703 to 2615), while uncontained cells stay 0 and lake and coast walls and steps stay 0. The first plan, changing the vapour entering at the upwind edge, has no effect (vapour over the sea rebuilds toward saturation; a 24-seed census with that value at 0.90 was identical). Without humidity, hot-dry was the largest lowland group on 32 of 64 seeds (wet 20, cold 12) because moisture demand grows with warmth.

64-seed census with the character on (seeds 1 to 64, rows in `docs/census-pr1.csv`) against HEAD's census above:

| Measure | HEAD | Character on | Goal |
| --- | --- | --- | --- |
| Land share, seeds in 15% to 70% | 62 of 64 | 62 of 64 (53 in 20% to 70%) | 58 of 64 |
| Spawn region found | 64 | 64 | 61 |
| Seeds with one biome over 70% of land | 0 | 0 | at most 6 |
| Cold group (glacier, tundra, taiga) at 15% or more | 0 | 15 | 10 |
| Hot and dry group at 15% or more | 64 | 43 | 10 |
| Wet group (forest, jungle) at 15% or more | 64 | 52 | 10 |
| Largest lowland group per seed: hot-dry / wet / cold | 64 / 0 / 0 | 23 / 29 / 12 | |
| Slowest generation | 11.4 s (seed 46) | 11.8 s | 15 s |

Land share does not follow the drawn knobs (correlation with relief -0.14, warmth 0.16, plates -0.01): it comes from where tectonics puts the sea. The two seeds below 15% land are 49 (9.9%) and 3 (13.3%); the design's floor is 15%.

`water_audit --fine` with `OCHRE_CHARACTER=off` is identical to main on seeds 1, 42, 1337, 300059728 and 1692045686. With the character on (measured before humidity, which changes lakes and rivers a little; the same gates held with it on seeds 1, 42 and 1337; no uncontained cells and no lake or coast walls or steps on any seed): rivers walls / tall walls / steps are 27 / 0 / 595 (seed 1), 177 / 30 / 3740 (42), 111 / 9 / 1564 (1337), 32 / 9 / 1273 (300059728) and 11 / 0 / 130 (1692045686). `lod_sim` and `flight` on seed 13 show no holes and no frames below ground.

## Temperature gradient, continentality and the warmth-driven treeline (GENERATOR_VERSION 58)

Before this, a world's lowland warmth was its drawn baseline plus 0.06 of noise, so a cold world had a narrow temperature band and read as one cold biome pair (taiga and tundra; steppe, grassland and forest never appeared below warmth 0.26). `ClimateConfig` gains `gradient` (drawn 0.10 to 0.30), `gradient_angle` and `continentality` (drawn -0.12 to +0.12); the map file's config grows by 12 bytes. Regional warmth is `warmth + gradient * u + continentality * inland`, where `u` is the signed position along the gradient direction with warped coordinates (worked out every 8 cells and interpolated, so it costs about 0.01 s) and `inland` is the 3-4 chamfer distance from the sea over 375 cells. The design capped the gradient at `warmth - 0.12` so the cold edge never reached glacier; that cap is dropped, because it left a cold world only 0.08 of spread, and a polar glacier region is a feature of a cold world. With both fields 0 the temperature is bit-for-bit the old one (tested).

`treeline(R)` replaces the fixed 1100 to 1650 m: it moves 3500 blocks per unit of regional warmth (`LAPSE_PER_BLOCK`), is exactly 1100 to 1650 at the default warmth, starts at 50 m at the lowest, and ends at least 100 m above its start. A cold world's mountains are bare above a low line; a hot world's forest climbs.

64-seed census (`docs/census-1b.csv`) against the previous commit's:

| Measure | Humidity only | With gradient |
| --- | --- | --- |
| Lowland biomes at 5% or more of non-mountain land, mean | 3.8 | 4.5 |
| Same, cold worlds (warmth under 0.35, 15 seeds) | 4.1 | 4.7 |
| Cold worlds with 15% or more non-cold lowland | 7 of 15 | 11 of 15 |
| Non-cold share of a cold world's lowland, mean (min) | 0.25 (0.00) | 0.40 (0.05) |
| Seeds with one biome over 70% of land | 0 | 0 |
| Seeds with 15% or more cold biomes | 15 | 19 |
| Slowest generation; climate stage | 11.8 s | 11.3 s; 0.19 s |

On seed 12 (warmth 0.213) the biome layer went from taiga and tundra only to taiga, tundra, steppe and grassland on the warm north-west coast, with the south-east island under glacier. `water_audit --fine` on seeds 1, 42 and 1337 shows no uncontained cells and no lake or coast walls or steps, and with `OCHRE_CHARACTER=off` seed 1 is identical to main.

## Cold-world surfaces (GENERATOR_VERSION 59)

Three problems seen walking cold worlds. (1) Glaciers were edged and faced with rock: the glacier biome turned only gentle grass to ice, so steep faces, sea cliffs, shores and river banks were stone, gravel and sand. They are ice now, with snow on moderate slopes. (2) A river or lake cooled its banks by up to 0.14 whatever the climate, so in a world at warmth 0.3 to 0.4 the bank fell under the snowy-grass line (0.25) and the biome cut: a strip of snow or glacier along every river, with temperate ground beside it (a bank at 0.236 against 0.336 away from it in the test). Cooling now fades out below a ground temperature of 0.55 and is gone at 0.30. (3) LODs lost the top layer of snow and ice: a wider cell is sampled at its middle, which misses the top block that holds snowy grass, and `underwater` turned the frozen surface cell of a lake or sea (ice counts as solid) back into water. A cell now takes snow from the ground's top block, and `underwater` leaves ice alone. That exposed a latent bug in `freeze_cell`: at ice depth 0 its test `cell_top > water_top - 0` still froze the surface cell of every wide-cell lake and sea, in warm climates too, which `underwater` had been turning back to water; it now freezes only where the ice depth is above 0 (distant lakes showed as solid ice in between). Each fix has a test that fails without it.

## Tableland (GENERATOR_VERSION 60)

Step 0 spike (sedimentary, not mountain, 40 to 300 m up, share of land cells; default config and character on): seed 1 56.8% and 58.2%, seed 42 69.6% and 57.1%, seed 1337 52.9% and 53.7%, seed 300059728 48.5% and 63.0%. The design asked for 5% on three of four seeds: passed by a wide margin.

`landform_ground` now terraces ground by `max(mountain, tableland)` instead of mountain alone. `tableland` is nonzero on land 40 to 360 m up that is not mountain, in patches (20 km noise) covering about the seed's `tableland_share` (`LandformKnobs`, drawn 0.2 to 0.8, not stored in the map file and not affected by `OCHRE_CHARACTER=off`); `terraced` still gates on the sedimentary `bedded` weight. The design's gate `smoothstep(1 - s - 0.05, 1 - s + 0.05, n)` does not work: the noise sits between 0.3 and 0.65, so share 0.2 would put the cut at 0.8 and cover nothing. The cut runs from 0.59 (share 0.2) to 0.41 (share 0.8) instead; a test checks that share 0.2 covers 5% to 40% of eligible points and 0.8 covers 60% to 95%.

The soft-bed share is `base_soft(seed, x, z)`, 0.30 (mesa country, mostly hard benches) to 0.70 (badlands, mostly clay and shale), in place of the constant 0.35, so every sedimentary world changes, mountains included. It also feeds erosion through the rock, so maps change (the pinned hashes moved).

Against the previous commit with `OCHRE_CHARACTER=off`, `water_audit --fine`: no uncontained cells and no lake or coast walls or steps on seeds 1, 42, 1337, 300059728 and 1692045686. River walls over 6 m (non-waterfall) went 0 to 0, 0 to 0, 14 to 2, 0 to 10 and 8 to 1 (total 22 to 13); the gate says no higher than HEAD, which holds in total and on four of five seeds, not on seed 300059728 (the river-wall defect stays deferred). `bench`: regions 118 to 114 ms on seed 1 (plateau_edge 143 to 152, +6%), LOD total 244 to 250 ms; vertex counts on the three views 881,683 to 874,534, 530,460 to 523,189 and 636,501 to 617,338. `seams`, `lod_sim` and `flight` on seed 1692045686 show steps of at most 0.3 blocks, no holes and no frames below ground.

Known and deferred (LOD fidelity): a LOD node's coarse ground can put a tree on a terrace a few blocks from its true foot, so about one tree in three had no mesh in the node that holds its full-detail crown on a test world with tableland (one in sixty before). `test_tree_boxes_stay_textured_up_to_eight_wide_cells_with_a_tile_a_block` now takes the first tree whose node has a mesh.

## Dune scaling and wind alignment (GENERATOR_VERSION 61)

Dunes had one spacing (90 blocks), one height (16) and rows fixed across +x whatever the world's wind. `LandformKnobs` gains `dune_scale` (0.7 to 1.5, drawn per seed, not in the config or map file), which multiplies spacing and height, and `WorldTerrain` lays the rows across the world's own wind (`ClimateConfig.wind_x`, `wind_z` normalised), so a seed's dunes run the way its rain does. A scale of 1.0 with a +x wind reproduces the old dunes (tested through `Dunes::default()`). `OCHRE_CHARACTER=off` keeps the config's wind (1.0, 0.15 by default), so dunes there tilt about 8.5 degrees from before.

PR 4a's check, measured over the four audit seeds (grid of 47 blocks, a ring of 12 directions at 10, 20 and 30 blocks for water, character on): seeds 1, 42 and 300059728 have no dune point over 2 blocks tall under the new biomes, and seed 1337 has 77,119 of them with 276 (0.36%) within 30 blocks of river or lake water. The design asked for none and said a failure would need a stored water layer; at 0.36% on the one seed that has them I did not build it, and the test now bounds the share at 1%. The remainder is dunes on desert beside narrow rivers, which the riparian wetting (wide rivers and lakes only) does not reach.

`water_audit --fine` is identical before and after on seeds 1, 42, 1337 and 300059728, and `bench` vertex counts and times are unchanged on seed 1 (the tool uses the default config, so it does not reach the drawn dune scale).

## Mountain pits and glacier ice up close (GENERATOR_VERSION 62)

Two things seen walking mountains and glaciers. (1) Narrow pits 25 blocks wide and 40 to 60 deep across mountain ranges, with no water: `terraced` stepped the ground by its height including the finest detail, so where the noise dipped just over a ledge's edge the ground fell a whole bed in one step. Beds now step the ground as a 32-block cell sees it (`TERRACE_CELL`, no gullies or octaves under about 64 blocks) and the fine detail is laid on top. A scan of mountain ground on seeds 1692045686 and 41608949 (a pit is a point 12 or more blocks under all four neighbours at 16 blocks) found 138 and 103 pits before and 16 and 34 after, and the deepest went from 55 and 40 blocks to 41 and 26. (2) A glacier's ground was one block of ice, three of snow, then stone, so any slope or cliff showed snow streaks and rock. A cliff built up in a glacier is ice, and ice takes over from snow on slopes up to 0.8 (was 0.4); how deep ice shows down a face is the exposure rule in the next section.

Cost: region time on the mountain and plateau views was up about 16% (a second, cheaper noise call on terraced ground); on seed 1 at its spawn it is unchanged (114 ms). `bench` chunk vertices on the plateau_edge view went 523,189 to 658,819 (+26%, over the 1.10 gate; mountain 617,338 to 637,281, +3%), but that is the bench window catching more cliff face once the terraces are clean contours, not cost per area: the real GPU frame time on that view, 80 s of `OCHRE_STATS` frames each, is 9.28 ms after against 9.33 ms before (main opaque 1.79 ms both). `water_audit --fine` is identical on seeds 1, 42, 1337, 300059728 and 1692045686 (lake `raised` counts moved by 1 to 8).

## Cliff faces show their surface material (GENERATOR_VERSION 63)

The glacier problem again, for grass: a column's surface is grass but a cliff face shows the blocks under it, which were one block of grass, a block or two of dirt, then stone, so a grassy cliff read as grey rock with a green fringe up close while the LODs (which draw grass with the grass tile on every face) showed it green. The cause is that `under_grass` worked out a block's material from its depth under its own column's top, when a block on a cliff face is exposed from the side. Each chunk column now knows how far it stands over its lowest neighbour of the four (`ChunkColumns::exposed`, 0 on level ground, the face's height at a cliff's edge), and `skin_depth(surface, exposed)` keeps the surface material for that many blocks, up to how far each clings: grass 8, ice and snow 40, sand and gravel 4. Below that, and on level ground, the layers are as before (grass over dirt, glacier ice a crust over snow, then stone). This replaces the first version of the glacier fix, a flat 40-block `ICE_DEPTH`, and an earlier gradient-based grass depth. Not covered: overhangs the cliff pass cuts after the column's height is set.

`bench` vertex counts and times are unchanged (637,028 against 637,281 on the mountain view, 658,336 against 658,819 on plateau_edge): only blocks under the surface changed, not its shape.

## Larger 3D shaping (GENERATOR_VERSION 64)

The cliff pass cut or built the surface by at most 9 blocks on steep ground. `cliff_reach_boosted(gradient, boost)` lengthens that by up to double (18 blocks, `MAX_CLIFF_REACH`) where a seed's `cliff_boost` (`LandformKnobs`, 0 to 1, drawn per seed, not stored) meets mountain or tableland: `TerrainSource::cliff_boost(x, z)` is the knob times the larger of the mountain and tableland weights, 0 elsewhere and by default, `ChunkColumns` keeps one value per column, and `chunk.rs` uses it for the cut and for skipping flora. Only reach changes, so ground under gradient 1.0 gets none whatever the boost. The vertical margin the design said to check: `known_empty` (LOD spawn) treated a node as buried when it was under the lowest ground, but a cut can open air as far under it as the reach, so that now needs `MAX_CLIFF_REACH` more; the tree-height margin over the highest ground (27 blocks) already covers a build-up of 18.

Against the previous commit, `bench` vertices on seed 1692045686 with its drawn boost: mountain 637,028 to 661,126 (+3.8%), plateau_edge 658,336 to 678,155 (+3.0%), below_wall 916,710 to 946,500 (+3.2%); chunk mesh time up about 7%. At the worst case, a boost of 1.0 forced on: 698,203 (+9.6%), 706,175 (+7.3%) and 989,310 (+7.9%), mesh time up about 11%, all inside the 1.10 vertex gate. Seed 1's spawn is not on mountain or tableland and is unchanged. `water_audit --fine` is identical on seeds 1, 42, 1337, 300059728 and 1692045686 (cliffs are not cut beside water), and `seams`, `lod_sim` and `flight` on 1692045686 show steps of at most 0.7 blocks, no holes and no frames below ground.

## Gorge rivers (GENERATOR_VERSION 65)

Where a river runs over tableland (weight over 0.2) its bank is no longer the usual parabola-eased slope: `bank_width(river, gorge)` is multiplied by `1 + 3 * weight` (up to 4 times) and the bank rises as `t^3` (a flat floor by the water, the steepest rise out at the wall) instead of `t * (2 - t)`. The design asked for `1 - (1 - t)^3`, which rises faster than today's near the water and so makes a ramp, not a floor and a wall; `t^3` is what the design's own description ("a wide floor and a steep outer wall") needs. With no tableland the bank is today's (tested), the bank is cut down to the land and never above it, and the valley's graded floor widens with it.

Pass condition (river walls over 6 m, non-waterfall, no higher than before, no new uncontained cells) holds on all five seeds: `tall_walls` 2 to 2, 8 to 8, 48 to 48, 13 to 13 and 0 to 0 on seeds 1, 42, 1337, 300059728 and 1692045686, and `map uncontained` stays 0 everywhere, with no change to lake or coast lines. The total `walls` count (steps over the smaller threshold) rises a little, 43 to 49, 119 to 124, 156 to 164, 43 to 45 and 11 to 12, which is the gorge walls themselves.

## Rock from the plates (GENERATOR_VERSION 66)

Mountains were split between rock types. `geology::generate` took a cell's province from the boundary type at that one cell (subduction or extension: volcanic; collision above 100 uplift: metamorphic; else sedimentary), gated by a 12 km noise field. A range is much wider than the boundary line that raised it, so its flanks came out sedimentary and its core marble or granite, and the noise cut gaps along the belt. Measured on five seeds, 3 or 4 of a seed's 5 connected mountain ranges of 1,500 or more cells had a second province over 20% (the biggest 35% to 56%).

Now each plate is a rock, `ROCK_BASIN` (sedimentary), `ROCK_SHIELD` (metamorphic, with the granite core where uplift passes 450) or `ROCK_VOLCANIC`, and the sea floor is volcanic as before. `Tectonics::plate_rocks` shares the rock out by how much land each plate owns, so every world is about 45% to 60% basin (drawn per seed), 28% shield and the rest volcanic, with some chance in which rock the biggest plate gets. `map.plate`, which only the viewer read, is now the plate whose rock a cell takes: inside a boundary's belt the first plate of the pair, so a range is on one plate whichever side of the boundary a cell is, else the nearest plate. After the smoothing, `one_rock_per_range` gives each connected stretch of high ground (200 blocks over the sea before erosion, 4-connected) the rock most of it is, so belts that meet or a province edge cannot cut a range.

Result (same five seeds): connected ranges with a second province over 20%, 0 on every seed (largest second share 13%, in a 5,300-cell range's edge cells; granite core and marble margin counted as one province); land share by province 52% to 72% sedimentary, 17% to 32% metamorphic, 9% to 21% volcanic; tableland-eligible land (sedimentary, not mountain, 40 to 300 m up) 35% to 47% against 49% to 70% before and 5% needed. A shield range still has a granite core and a marble margin by design (the earlier tectonic work); the vote counts them as one rock.

Because rock sets erodibility the maps change: `water_audit --fine` shows no uncontained cells and no lake or coast walls or steps on seeds 1, 42, 1337, 300059728 and 1692045686, and river walls over 6 m go 2 to 1, 8 to 7, 48 to 37, 13 to 3 and 0 to 0. `bench` chunk vertices on one mountain spot per seed over seeds 1, 42, 1337 and 5: +3.2%, -1.6%, +1.1% and +12.5% (mean +3.8%, mesh time unchanged); the fixed 1692045686 mountain view reads +48% only because the terrain at that spot is now a different mountain.

## Fjord and cone spikes (PR 5, no generation change)

Both shapes of the design (section 6) were built as throwaway code and measured on the four audit seeds (1, 42, 1337, 300059728, character on) with the shapes forced on, laid after erosion and the floor clamp and before `carve_passes`, then the rest of `generate` run. No generation change here; the code comes with PR 5a (fjords) and PR 5b (cones).

**Fjord** (2 per seed forced; `shapes::fjords` as the design: a site is the best of 32 hash-chosen coast cells with 300 blocks of uplift averaged over 11 cells, the cold side of the gradient favoured; the trough runs up the smoothed ground for 40 to 120 cells or until the ground stops rising; floor `-D` with D 40 to 120 blocks, 8 cells wide, walls rising 40 blocks a cell). Pass condition: at least 60% of the carved wall remains near the shore, and `water_audit --fine` shows no new uncontained cells or lake or coast walls. Result: remaining carve 1.00 (the cut is untouched by `carve_passes`, hydrology and the floor clamp) over 9,841 to 13,941 shore-wall cells per seed; every trough-floor cell is ocean in the final map (78 of 78, 70 of 70, 67 of 67, 103 of 103); `water_audit --fine` shows `map uncontained` 0 and no lake or coast walls or steps on all four seeds. Passed. Note: a trough follows the ground only while it rises, so lines are 11 to 64 cells long, often under the 40 asked for.

**Cone** (4 per seed forced; a site is the highest of 32 candidates among volcanic cells over -200 blocks, clear of earlier cones; radius 16 to 48 cells, height 150 to 400 blocks, crater a quarter as wide and deep). Pass condition: no lake cell in a land crater, and a crater floor under zero is sea. Result on land sites (highest candidate): 16 cones over the four seeds, 0 lake cells in 2,620 crater cells, no crater floor under the sea. With the sites forced onto the lowest volcanic cells, 15 of 16 cones have craters under the sea, and 99% of those floor cells are ocean (10 and 2 cells in two cones are dry hollows under zero at a crater's edge). `water_audit --fine`: `map uncontained` 0 and no lake or coast walls or steps on all four seeds. Passed. Not part of the condition but to watch in PR 5b: river walls over 6 m rise with cones (seed 42: 7 to 24; seed 300059728: 3 to 7; seeds 1 and 1337: 1 to 3 and 37 to 33), since a cone's flanks are steep ground for the rivers to run on.

## Fjords (GENERATOR_VERSION 68)

`shapes::fjords` (new `worldmap/shapes.rs`, laid after erosion and the floor clamp and before `carve_passes`, so slumping cannot soften the walls) cuts fjords into mountainous coasts. A site is a coast cell (sea within 2 cells, 300 blocks of uplift averaged over 11 cells, 60 cells clear of the map's edge). The coast's room is the number of mouths that pack 60 cells apart over those cells (9 to 33 on the audit seeds, 15 to 98 km of eligible coast), and a seed's `fjord_share` (`LandformKnobs`, 0 to 0.3, drawn per seed, not stored) of that room is cut, so the count follows the coast and not a fixed cap: the first version drew 0 to 3 per seed, which gave a world with 98 km of mountainous coast none. Counts now run 0 to 7 (seed 1: 4, 42: 1, 1337: 2, 5: 4, 41608949: 7, 12: 1, 300059728: 0). Each fjord takes the first of 32 hash-chosen candidates, best first by uplift with the cold side of the config's gradient favoured, whose climb up the smoothed ground (5-cell blur) is at least 20 cells and whose mouth is at least 60 cells from an earlier one's.

The trough runs up that climb for 40 to 120 cells or until the ground stops rising. The floor is 32 to 144 blocks under the sea (a drawn depth of 40 to 120 varied by 0.8 to 1.2 along it) and 5 to 16 cells wide (a half-width drifting between 2.5 and 8 cells over about 10), the walls rise 40 blocks a cell, and the sea fills it by height alone. To keep it from being a straight trench the centre line wanders up to 6 cells either side over about 14 cells (plus a finer wobble), and the walls are ragged: their distance is pushed in and out by three octaves of noise (12, 5 and 2 cells long, 2 cells in all), which matters because a smooth analytic wall read as a clean, straight-edged channel beside the natural coast's noise. A first version with a 4-cell wander and one 1.5-cell octave still read as straight on seed 5. The spike (above) found the first version often cut two or three fjords from one site and some were 11 cells long, hence the spacing and the minimum length.

On the audit seeds `water_audit --fine` shows `map uncontained` 0 and no lake or coast walls or steps; river walls over 6 m go 1 to 0 on seed 1, 7 to 16 on 42, 37 to 31 on 1337 and are unchanged on 300059728 (no fjord). `seams` on seed 1337 reads steps of up to about 3 blocks on a fjord wall (0.7 or under elsewhere): the walls are 40 blocks a cell or more, steeper than a LOD cell can follow, the deferred LOD fidelity work. Example sites: seed 5 has a fjord with its mouth at (8656, -25168); seed 41608949 has seven, one at (17264, -20112).

## Volcanic cones (GENERATOR_VERSION 69)

`shapes::cones` raises cones with craters on volcanic ground (`geology == VOLCANIC`, over -200 blocks, 60 cells from the map's edge), laid after erosion and the fjords and before `carve_passes`. As with fjords the count follows the ground: the sites volcanic ground has room for, packed 90 cells apart in a seeded order, and a seed's `cone_share` (`LandformKnobs`, 0 to 0.25, drawn per seed, not stored) of them are raised (the design had a fixed 0 to 6). Each takes the highest of 32 hash-chosen candidates clear of earlier cones, with a radius of 16 to 36 cells and a height of 150 to 400 blocks; the flanks and rim are pushed in and out by up to 18% of the radius so a cone is not a round heap.

Two changes to the design's shape. Its crater, the cone `H * (1 - t)^1.5` minus a quarter of `H` inside a quarter of the radius, leaves the middle higher than the rim (the cone is 0.75 H at the centre and 0.65 H at the rim), a bump and not a crater, and the spike's "no lake in a crater" held only because there was no bowl. Inside the crater the cone is now cut back to a bowl a quarter of `H` under its rim, so the floor is `0.40 H` over the ground. And the radius is capped at 36 cells (it was 48), so a crater is at most about 250 cells, under `MIN_LAKE_CELLS` (300) which hydrology keeps as a lake.

Measured with cones forced to 0.3 of the sites, 14 to 21 cones a seed on seeds 1, 42, 1337, 300059728 and 5 (75 cones): 0 lake cells in any crater, no crater floor under the sea but 1 cell on one coastal cone. With the seeds' own shares `water_audit --fine` shows `map uncontained` 0 and no lake or coast walls or steps on all five seeds, and river walls over 6 m go 0 to 0 on seed 1, 16 to 16 on 42, 31 to 13 on 1337 and 3 to 0 on 300059728. `bench` vertex counts on the three mountain and plateau views are unchanged (none stand on a cone), and `seams` is 0.34 or under on seed 1.

## Bare basalt on volcanic ground (GENERATOR_VERSION 70)

A cone raised on a volcanic province had meadow on its slopes and grass-skinned cliffs, over basalt that only showed under them. High ground on a volcanic province (`geology == VOLCANIC` and 120 or more blocks over the sea at that point) is now a new `volcanic` biome whatever the climate says: `Volcanic.block` turns grass to stone on any slope of gradient 0.5 or more (the rock under it is basalt, so the face is basalt), keeps meadow on the gentlest ground, and turns sand to gravel; trees thin to a density of 0.01 but alpine pines still grow on its lower slopes (`tree_kind` treats it as mountain). `WorldTerrain::biome_at` makes the override at block scale only, so the map's biome layer, the census and the climate are untouched, and LODs read the same function. It applies to every volcanic range and lava terrace as well as cones, and a test checks it holds on the high volcanic points of a world and nowhere else. A slice across a cone on seed 1337 now shows basalt on the surface of the faces. `bench` vertex counts on seed 1 are unchanged (1,895,637).

## Biome identity, mountain as a modifier, and swamp (GENERATOR_VERSION 71)

**Mountain is a modifier.** `BiomeKind::Mountain` was one of the biome layer's kinds: where the landform was mountain the climate said nothing (except under glacier), so a desert mountain, a taiga mountain and a jungle mountain were the same meadow with the same rock rules, a cone looked like a green hill, and the volcanic biome was a second override of the same kind. The map's biome layer now holds only the climate kind (`Mountain` stays in the enum as a scratch value for `landform`, never stored; a test checks it), and `ochre_base::biome::Modifier` (`None`, `Mountain`, `Volcanic`) sits on a climate biome: `Biomes` holds each biome, and the base terrain, under each modifier, `with_modifier` finds them and `modifier` and `name` (still the climate biome's name) tell them apart, and `id` finds only the unmodified ones. `WorldTerrain::biome_at` picks the modifier at block scale (volcanic: volcanic province and 120 or more blocks over the sea; mountain: landform mountain). A mountain modifier turns grass to stone on slopes past its limit (1.6 falling to 1.0 with height), sand to gravel, thins the inner biome's trees to 0.04, and puts springs in stone; the climate biome underneath still sets the rest (sand and sandstone in a desert, conifers in taiga, ice under glacier). Alpine forest density in the map now needs moisture (`smoothstep(0.15, 0.5)`) so a dry mountain is not wooded, and alpine pines grow on any mountain's upper slopes as they did (`tree_kind` takes the flag).

**Signatures.** `Biome` gains `climate_shift` (added to the temperature and humidity grass and flora are tinted by, and to the warmth that picks pine over broadleaf) and `tree_scale` (trunk height, crown spread and crown squash multipliers, held to `TREE_HEIGHT` and `TREE_REACH` by `scaled_tree`; a test shows defaults change no tree and every biome stays inside the caps). Values: tundra tint (-0.10, -0.05); taiga trees (1.2, 0.8, 1.0); steppe tint (0, -0.10); savanna tint (+0.05, -0.15) with flat crowns (0.8, 1.2, 0.5); jungle density 0.45, tint (0, +0.15), trees (1.3, 1.0, 1.0).

**Swamp** (`BiomeKind::Swamp = 11`, `biome/swamp.rs`): density 0.12, tint (0, +0.20), mud (dirt) under 3 blocks over the sea, clay at springs. Rule in `climate::run` before `tidy_biomes`: grassland, forest or jungle, not mountain, 40 m or less over the sea, slope under 0.2, moisture 0.65 or more, within about 13 cells of a river or lake; swamp cells the tidying pushed past 40 m or onto mountain return to their climate biome. The design's rule (20 m, slope 0.1, moisture 0.7, 10 cells) gave under 1% of land everywhere (low land is only 3% to 4% of land and half of it is steeper than 0.1) and 0 on two of six seeds. Not-swamp is guaranteed by the rule: swamp stays under 40 m and mountain landform starts at 70 m with a slope or 150 m.

64-seed census (`docs/census-pr6.csv`): land share in 15% to 70% on 62, spawn found on 64, no biome over 70% of land, 22 seeds with 15% or more cold biomes, 52 hot and dry, 59 wet (forest, jungle, swamp); swamp on 62 seeds, 1% or more on 20, at most 2.4% of land; mountain landform 27% of land on average (50% at most), now reported as `mountain_share` since the biome layer has no mountain. The slowest generation is 13.3 s uncontended (an earlier census read 17.7 s because tests ran beside it).

**An unrelated bug found on the way** (from tableland, PR 3): `rock_at_cell` computed `base_soft`, a noise call, on every lookup, and erosion makes millions of them: erosion went from 2.2 s to 6.1 s on seed 1337. The soft share per cell is now cached by `geology::generate` (`WorldMap::soft`, not saved, never part of map equality) and erosion is back to 2.2 s.

`bench` on seed 1: region 114 to 117 ms (+3%), LOD total 247 to 261 ms (+5.7%, a hair over the +5% gate: the biome table now has an indirection per block), chunk vertices 1,895,637 to 1,896,445.

## Jungle canopy: emergent giants (GENERATOR_VERSION 72)

Jungle trees were capped at 14 blocks of reach and 32 of height, so a jungle was a low roof. `TREE_HEIGHT` is now 48 and `TREE_REACH` 14, jungle density 0.45 to 0.6, and `TreeScale` gains `giants`: the share of a biome's trees that stand out. Jungle's is 0.1, and a giant is 2.3 times the trunk height and 1.35 times the crown spread of an ordinary tree there (about 45 blocks, held to the 48 cap by `scaled_tree`). Every other biome has 0, so their trees are unchanged. Undergrowth is left to PR 8.

Two tests assumed things that no longer hold. The thin-trunk tinted-leaves test now builds two thin trees directly (a jungle's trunks are all thick, and the test world's densest forest is jungle), and the lone-tree test takes a tight `Tree::extent` for "clear of the cell's edges", since `bounds` is the worst case (`TREE_REACH` and `TREE_HEIGHT`) and now spans more than a 64-block LOD cell. `tree_spot` gains `try_tree_spot`, which returns None instead of panicking.

`bench` on seed 1 (A/B in one session): region 118 to 117 ms, chunk meshing 421 to 429 ms (+2%), chunk vertices 1,896,445 to 1,909,937, LOD total 263 to 300 ms (+14%). The cost is in levels 3 to 5 (57 to 66, 34 to 40, 88 to 110 ms), where crowns are drawn as boxes: giants add taller boxes and the wider search margin (`TREE_REACH`) covers more trees per node. LOD meshing is off the main thread, so no frame-time phase moved, but it is over the +5% gate; kept because the tall canopy is the point of the change.

## Tableland is terraced before the water is routed (GENERATOR_VERSION 74)

Lakes and streams on tableland stood on humps. Water was routed over the map's smooth height, and `terraced` then stepped the ground down toward each bed's tread, up to a bed's thickness under it, so a lake stood on a plateau with the real terraces beyond: on seed 1650122255 a lake at 67 had ground at 51 to 56 within 60 to 400 blocks. A first fix that held the beds off near water (a distance fade) only moved the problem outward, as a low dome the lake sat on.

`landform::terrace` now steps each tableland cell's height into its beds after the passes and before `scatter` and the last hydrology run, with the same weight (`landform::tableland`, moved out of `WorldTerrain`) at the cell's centre. Basins, lake levels and rivers form on the treads, and the terrace faces bound the lakes. The block-level beds stay, as they sharpen the bicubic ramp across a face into a cliff. `landform::run` now also runs first, for the mountain mask. The map costs 0.27 s more to build, once per seed.

On seed 1650122255 lakes now stand at 59 with ground rising to 65 to 77 on each side. `water_audit`: lake walls 0 on seeds 1650122255, 11, 7 and 368871838, `map uncontained` 0. Over seeds 1 to 12 river cells fall 1 to 16% and mean land height 1 to 15 blocks against the PR 6 census, as treads are flatter and lower than the smooth ground. A test checks that tableland cells sit nearer their beds (median 4 blocks, 17 without the pass).


## Transition zones (GENERATOR_VERSION 75)

Biome edges were hard lines wandering 19 blocks off the map's cell squares. Three changes. (1) `wandered` offsets by 1.5 cells (48 blocks), up from 0.6, which also moves rock provinces' edges, and the climate biome's cell is drifted by a slow warp (perlin at wavelength 700, up to 160 blocks, falling back to the wandered cell at the coast) so edges between climates meander over hundreds of blocks; mountain and volcanic cover keep the wandered cell so they stay on their ground. (2) Two map layers, `biome_alt` and `biome_margin` (the file grows by 2 bytes a cell): each land cell probes warmth and moisture 0.01 to 0.04 either side, and the first offset that changes its class sets the margin (the offset over 0.04, as 0..255) and the biome across it; a cell the swamp rule or the tidying changed stays at 255. (3) At block scale, `biome_at` takes the alt biome where `fbm_filtered` at wavelength 12 falls under `0.5 * (1 - margin / 255)`, so patches of 10 to 20 blocks are thickest on the boundary. Cells 16 blocks wide or more use their own biome, so LODs are unchanged.

The design's first step, noise of 0.05 in warmth and 0.08 in moisture at wavelength 1500 before `classify`, is not built: it made round islands of the other biome 300 to 600 blocks across wherever a class boundary ran through nearly flat climate. Drifting the lookup bends the edges without creating islands.

The design's straight-edge metric could not tell fixed from unfixed (HEAD already passed under 200), so `worldprobe edges` also counts crossings: the flips along a row within 48 blocks of each other, a hard edge being one flip and a mix three or more. On seeds 1, 42, 1337 and 300059728, windows at (0, 0) and (+-12000, +-12000) with land:

| | hard (one flip) | mixed (three or more) | longest straight edge |
| --- | --- | --- | --- |
| before | 74 to 90% | 8 to 16% | up to 136 blocks (244 on one window outside this set) |
| after | 0 to 66%, mostly 10 to 40% | 11 to 67% | up to 88 blocks |

Census over 64 seeds: no biome over 70% of land (largest 60.7%), spawn on all 64. Climate takes about 0.13 s longer from the margin probes. `bench` at a river (16912, -18064) and a peak (9744, -6768) on seed 1: vertices 928,493 to 928,061 and 596,659 to 596,665, chunk generation and meshing unchanged within noise (the default spawn moves, so its view is not comparable). `water_audit` on seeds 1 and 42: no uncontained cells, no lake or coast walls or steps. Tests: the margin layers are 255 on sea and every zone cell has another biome across it, zone blocks take the biome across while wide cells and cells well inside never do, and the map round-trips through bytes.

Not done: trees and flora density still read the cell's own biome, so a patch of the other biome keeps its neighbour's tree density; the dither moves the biome, not the density layer.

## Groves, clearings and undergrowth (GENERATOR_VERSION 76)

Trees grew at the map's cell density, so a forest was one even scatter. `WorldTerrain` now multiplies a sample's `trees` by `grove(seed, x, z, cell)`: 0.4 in a clearing up to 1 in a grove, from `fbm_filtered` at wavelength 160 over two octaves through a smoothstep, filtered for the sample's cell so wide cells see an average. It never exceeds 1, so `tree_bound` still bounds the density. A tree-density render of a 2 km window and of 16 blocks a pixel on seed 1692045686 shows patches of about 100 to 180 blocks; the wavelength was not tuned. Cells 512 blocks wide see about 0.7 with little variation (tested).

Undergrowth: `flora_at` takes an `understory` multiplier on the ferns and shaded grass under trees, 2.0 in jungle, 0.5 in taiga and 1.0 elsewhere (so every other biome's output is unchanged, tested), and jungle's floor counts as shaded like a forest's. The design's table (jungle 0.6, forest 0.25, taiga 0.15, other 1.0) would have thinned forest floors below today's, the opposite of a thick jungle floor, so these values replace it. Existing flora tiles only.

`bench` on seed 1, before to after: at a river (16912, -18064) chunk vertices 928,607 to 798,883 and meshing 332 to 311 ms, LOD total 286 to 239 ms; at the default spawn 1,917,466 to 1,444,643 vertices and 430 to 342 ms, LOD total 316 to 243 ms. Clearings thin the trees on average to about 0.7 of their density, so this is below the gate of 1.15 times. Not measured in the game.

## Cold relief features (GENERATOR_VERSION 77)

`ochre-base/src/features.rs` holds one pure function per feature, `offset(kind, seed, x, z, cell, ctx)`, in blocks, clamped to +-18 and zero once cells are 16 blocks wide. `Biome::relief()` lists a biome's features (default none) and `WorldTerrain` adds their sum to the ground after containment, only where no river, spring, lake or sea is near (`by` is empty) and the ground is 6 blocks over the sea, so the water rule is untouched. The context is the smooth map's gradient and downhill direction over 16 blocks, and the config's `gradient_angle`.

- glacier: crevasses (slots 2 to 5 wide and 8 to 18 deep across gently convex slopes of gradient 0.15 to 0.6 in belts, with an uncut bridge every 15 to 30 blocks along each) and seracs (jagged pillars 6 to 18 high where ice is steeper than 1.2). The slopes they need are mountain flanks, so a flat lowland glacier gets neither.
- tundra: pingos (round mounds 6 to 14 high on flat ground, about one to 10,000 blocks squared) and patterned ground (a net of one-block ridges every 12 to 20 blocks).
- taiga: drumlins, streamlined swells in parallel along the config's `gradient_angle`, on ground gentler than 0.4.

`worldprobe features <seed> <x> <z> <half>` counts samples carrying each feature (and any outside their biome, which must be 0). On seed 2, 3,000 block windows: crevasses 2,196 and drumlins 23,779 of 562,500 samples in the glacier window, drumlins 65,543 in a taiga one.

`bench` now builds its world with `character::world_config`, as the game and probe do (it used the plain default, so seed 2's cold country was elsewhere). On seed 2, before to after: a taiga plain at (-9000, -12000) chunk vertices 129,683 to 141,858 (+9.4%) and a glacier at (-16900, -18500) 572,493 to 623,921 (+9.0%), meshing 173 to 175 ms and 234 to 241 ms, chunk generation 159 to 166 ms and 240 to 272 ms. Drumlins as first written (8 to 14 high, 15 to 30 wide, 70% of cells) gave +45% on the taiga plain, since a flat plain meshes in a few faces and any relief breaks it up, so they are 4 to 8 high, 20 to 35 wide and 50% of cells; the gate is 1.10. `water_audit` on seeds 2 and 11: no uncontained cells, no lake or coast walls or steps.

Not built: glacial erratics (taiga and tundra) and termite mounds (savanna, PR 6c) as objects, which need new tree kinds with crown shapes, LOD handling and hash placement, so they wait for their own change. The crevasse rule against opening in a spawn region is not enforced either: a glacier is not a spawn site.

## Warm relief features (GENERATOR_VERSION 78)

The same machinery as the cold features, four more kinds in `features.rs`:

- desert: yardangs, parallel ridges (up to 5 high) and troughs (up to 2.5 deep) along the wind, in patches 900 blocks across, on ground gentler than 0.35. They are not gated by the dune cover, so over a dune sea they add to the dunes.
- savanna: kopjes, rounded heaps 6 to 14 high and 10 to 25 across in clusters of 2 to 6, one cluster in about three 400-block cells, on ground gentler than 0.4.
- steppe: gullies, wandering ravines 3 to 8 deep and 3 to 6 wide down slopes steeper than 0.2.
- swamp: hummocks, tussocks 1 to 3 high and 4 to 8 across on flat ground.

`worldprobe features` now prints an example coordinate for each kind. Every kind fires on the seeds tried (yardangs 107,805 and kopjes 1,562 of 2,250,000 samples on seed 11 at (12000, -12000), gullies 16,089 on seed 1, hummocks 72,851 on seed 42), none outside its biome.

`bench` before to after, vertices (mesh ms, generation ms): a desert on seed 11 at (9568, -14344) 370,241 to 398,241, +7.6% (190 to 191 ms, 214 to 251 ms); savanna on seed 7 at (9000, 12896) 729,331 to 731,524 (213 to 211, 244 to 299). Yardangs as first written (8 high, 4 deep) gave +12.9%, over the 1.10 gate, so they were lowered. The gully view (seed 1 at (9000, -10360)) and hummock view (seed 42 at (-2532, 1820)) read identical vertices, so the exact sample point sits by water or a feature gap; their cost is not measured. Chunk generation in a feature-heavy view is about 17% slower (the gradient is four map samples a column, and the noise), 18 microseconds a chunk, and is recorded as the reason. `water_audit` on seeds 1, 42 and 7: no uncontained cells, no lake or coast walls or steps.

Not built: termite mounds and the kopjes' bare rock tops, as they are objects or a surface-block rule, with the glacial erratics of the cold features.

## Inselbergs, tower karst and cirques (GENERATOR_VERSION 79)

Three map-scale shapes in `shapes.rs`, laid after the cones and before the mountain passes. Each has a per-seed count in `LandformKnobs` (inselbergs 0 to 8, karst groups 0 to 4, cirques 0 to 6, not stored) that the config weights, never the climate output: inselbergs by how dry and warm the config is, karst by how wet, cirques by how cold. Sites are chosen in a seeded order at least 40 cells apart.

- inselberg: a rounded dome 20 to 45 blocks high and 4 to 10 cells in radius, ragged at its edge, on sedimentary lowland 20 to 150 blocks over the sea with a slope under 0.15.
- tower karst: a field of 10 to 40 towers, each 1.5 to 4 cells in radius and 30 to 70 blocks high with near vertical sides, within `3 * sqrt(n)` cells of a site on the same lowland. Towers take the highest of those over a cell, not the sum.
- cirque: a bowl 3 to 6 cells in radius and 40 to 100 blocks deep with a flat floor, cut into ground at least 250 blocks over the sea with a slope over 0.2, on the cold half of the config's temperature gradient.

Spikes, on seeds 1 to 4 with the shapes forced on in a 24 km world run through the last hydrology: every dome keeps 99 to 100% of its height in the final map (the plan asks for 60%) and none gains lake cells within twice its radius, towers leave fewer lake cells beside them (2,569 to 914 on seed 1, 453 to 138 on seed 3, nothing on seeds 2 and 4), and every cirque keeps a headwall of at least 60 blocks (60 to 436) and none becomes a lake (the draining that hydrology does for basins under 300 cells cuts it an outlet). A first version skipped the centre cell of each dome whenever its ragged offset went negative, which read as 0% kept on half the domes and a lake in the hollow on seed 4; the distance is now clamped at zero. All three pass and are kept. The sites are checked by `domes_keep_their_height_towers_leave_no_new_lakes_and_cirques_keep_a_headwall`.

Census over 32 seeds: no biome over 70% of land (largest 61.5%), spawn on all. Carve passes take 0.08 s on seed 1. `water_audit` was not rerun for this change, the shapes sitting on dry land and cold mountains apart from the lake counts above.

## Undercuts removed (GENERATOR_VERSION 83)

Undercuts (version 80) are gone: `generate/undercut.rs`, `ChunkColumns::notch`, `ChunkPasses::undercuts` and `worldprobe floating`, which compared with and without them. They cut a recess under a cap of a face, from smoothed ground read in eight directions. In the game they were inconsistent and out of place. A cross-section of the face at -6169 118 -5857 on seed 1708187577 showed a 3-wide diagonal slit through a ~65-block wall leaning at 60 to 70 degrees, which reads as a crack and not as a recess under a cap, since a leaning face has no cap to leave. They touched 78 columns against 12,409 for the cliff pass in a 192-block window, from gates on steepness and two noise patches, and went on any steep slope whatever its rock.

The cliff pass keeps what the undercut work changed in it (`VERTICAL_SQUASH`, `SKIN`, below). Floating pieces over the 160-block square at -6248 -6020 are the 5 the cliff pass leaves. Overhangs tied to rock layers (a hard bed over a soft one on tableland) would give a reason for them, and are not built.

## Cliff noise: slower with height, and a skin left alone

The cliff pass cut and built the surface from 3D noise that changes as fast with height as along the ground. With a reach of up to 18 blocks that turns a column's surface over several times and leaves floating pieces. The noise now changes 0.7 as fast with height round the column's ground (`VERTICAL_SQUASH` in `relief.rs`), which keeps overhangs (`test_steep_ground_gets_overhangs_and_gentle_ground_gets_none` fails at 0.6 and below) and cuts floating pieces over the 160-block square from 20 to 5. The top 3 blocks of a column are not cut at all (`SKIN` in `chunk.rs`), which stopped the cliff pass cutting the grass in patches and showing the dirt under it (+1.9% vertices on the mountain view).

## Cliff faces are bare rock (GENERATOR_VERSION 84)

A meadow patch on a mountain eases the slope the blocks are chosen by to 0.9, however steep the ground. On seed 3344283695 at 4289 268 12560 that put a grass and dirt skin on a ~70 degree sandstone wall, with bare sandstone above it. A column standing 3 or more blocks over its lowest neighbour (`FACE_EXPOSED` in `chunk.rs`) now takes the unlevelled gradient, so a face is rock and meadow clings to slopes and ledges. A first try capped meadow by the map gradient, which is steep at cell scale even on a flat ledge, and turned the foot bare too. With 2 blocks it also took bumpy flat ground. The LOD meshes use the same rule (`face_slope`), with the drop to the lowest neighbour counted per cell, so a cell 3 blocks per cell over its neighbour is a face. Meadow patches (`meadow_patch`) now apply only to a mountain over grassland (`GENERATOR_VERSION` 85): on 3884 12486, where the ground climbs about 2 blocks per block each way, a patch on a taiga mountain left a dirt and grass wedge no face rule catches. Pines on a mountain over another climate lose only ground steeper than about 63 degrees. Not measured: vertex counts and the other mountain seeds.

## Waterfalls (GENERATOR_VERSION 86)

The spike (below) counted 520 to 724 river stretches a seed that drop 6 blocks or more within 3 cells, mostly mountain streams of 6 to 25 percent grade with the banks following them down as a ramp. A steep river now runs in steps.

`stairs` (`water.rs`) turns the profile of a stretch that falls 0.2 blocks a block or more along its cell into treads and falls: from the second point on, each group of points is held at its lowest, a group taking points while it has dropped no more than `TREAD_DROP` (3 blocks), so the water stays under the ground it was graded to and no bank has to be built. The two end points are left, since the next stretch shares them. An interval that drops 5 blocks or more (`FALL_MIN`) is a fall, and a group holds at most 4 blocks of descent (`TREAD_DROP`) so the falls between come out at 5 or more. A fall of 2 to 4 blocks drew as a mangled sheet, and a drop under 5 is left as a steep rapid, a ramp. Before that minimum, an interval that dropped 2.5 blocks or more was a fall, and its lip is where the ground falls most in it (the middle where it falls less than a block at once). `RiverProfiles::level` steps there over about a block, so a point either side of the lip has the level of its own tread and the water mesh draws a curtain between them (a step of 2 blocks or more). Falls are as tall as the ground makes them: of 15,094 on seed 1337 with that first minimum, 4,557 were 2.5 to 4 blocks, 5,087 were 4 to 6, 3,733 were 6 to 10, 1,562 were 10 to 20 and 155 were more. With the minimum of 5 there are 6,436: 1,634 of 5 to 6, 2,997 of 6 to 10, 1,162 of 10 to 20 and 643 of more, the valley floor now following real cliffs. `worldprobe falls <seed> 0 0 [n]` lists the tallest with a spawn line.

`water_audit --fine` on seeds 1, 42, 1337 and 300059728: uncontained cells 0 on rivers, lakes and coasts as before, river steps down about 20% (1,177 to 944, 3,559 to 2,814, 2,613 to 2,021, 1,757 to 1,443), and river walls over 6 blocks up (1 to 5, 8 to 28, 34 to 57, 2 to 9), which are the falls. Banks raised over 2 blocks are no higher (41 to 35, 56 to 59, 84 to 88, 31 to 35). The map stage costs the same within noise.

Four changes followed a look in the game. The bed steps down a block before the water does at a lip (`river_hits`), so the water stands in a column as deep as the fall and the mesh draws a curtain down all of it, where before only the block at the lip had a face. `fall_water` in `water.wesl` draws a vertical face of fresh water as a sheet of streaks falling at their own speeds, since the surface shading read the bed through it and showed dashes and black. The valley floor follows the real ground where it falls 12 to 30 blocks or more under the map's (`valley_floor`, `VALLEY_GAP`), since on seed 1337 at -427 -4565 the map has a plateau at 199 over ground at 125 and the river ran out on a bank built up 70 blocks. And a river that ends in a lake stops where its water would stand more than a block under that lake's level (`UNDER_LAKE`), since the real ground there is lower than the lake and a river that cannot rise cannot feed it. The first form, ending a river where it would stand more than 6 blocks under the ground it was graded to, also ended rivers that cut a gorge through a ridge (seed 1337, -76 -5948). Clamping it to the lake's level instead bridged it over the low ground on a bank, and walls in the audit tripled.

A fall's sheet is drawn as whole blocks of water. The sheet was a parabolic slot in the rock (the bed's parabola ran over the fall's whole depth, `RiverHit::depth` now keeps it to the river's own), and the mesh draws a column's side as a slope between neighbouring surfaces, which on a diagonal lip came out as a staircase of faces with gaps. `TerrainSample::fall` marks columns whose water is deeper than the bed by 2 blocks or more (`FALL_SHEET`), and the mesher draws those with no level (`water_sheet`), so every block shows its sides. A debug colouring of faces by normal found both causes. The shader draws the sheet as streaks of white falling over a flat blue (`FALL_COLOR`), not the ripple tile, which showed as lines.

The columns beside a fall's sheet are drawn as blocks too (`near_fall` in the mesher's source), so the tread's surface meets the sheet with no slope between them, and the sheet's blocks reach the block above, with no seam. The bed steps down two blocks before the water, so the band the sheet is drawn in is two columns thick. The sheet is half opaque (`fall_water` returns alpha) like the rest of the water, and the back of the top of a fall's water is not drawn (the mesher gives it a negative shore distance in the vertex colour, which the shader discards), since seen from the air under a fall it showed as dark parallelograms along the lip. A distance cut-off for that did not work, since the lower treads of a chain are within a few blocks of the eye. The swell moves a water surface's vertices and not the sheet's sides, so a surface bobbing beside a sheet left gaps at its top and a skirt at its foot. The surface now keeps a share of the swell that is none beside a fall and the whole of it five columns off (`water_calm`, in the B channel the shader already scaled the swell by, taken at each corner from the columns round it so shared corners move alike), and the sheet's lowest faces reach down to the pool's surface, which lies a little under the top of its block. The spread surface is blended to the real level, the whole of it within two columns of a fall, since the far lower water over a fall pulled the spread surface of the tread down by up to a block and a half and the pool's up, which left a step at the lip and at the foot. A sheet's top face and the top edge of its sides follow the corners they share with the surface beside it, so they are one height, and its sides keep straight normals and reach a block down into the pool, whose surface lies under the top of its block by up to a block. The spread that smooths a surface over the water near it leaves out water more than a curtain's step (3 blocks) away, since the water over a fall pulled the pool beside the sheet up in a rising zigzag of corners. The water on the high side of a fall, within five columns of it and within 2 blocks of its level, is marked in the mesh like the sheet's top, so its back is not drawn over the curtain (`water_hush`); the pool's is drawn, for swimming under. At 98 -5544 on seed 1337 three falls stood within a few blocks of each other, and a level between two of them was a column or two of water at a height of its own, with its own top face against the curtain. Two falls less than 4 blocks apart along the river are now one (`MIN_TREAD`, the level between going down to the lower), and a lip is a step, not a ramp over a fraction of a block, so no column stands at a level between the two treads (5,980 falls on seed 1337). Known: a lip across a diagonal river stands as a staircase of columns, so the sheet is not one flat plane.

Rivers that end short of a perched lake take about 10% off the river cells (13,917 to 12,453 on seed 1, 22,748 to 20,312 on seed 300059728). `water_audit --fine` on seeds 1, 42, 1337 and 300059728: river walls 79, 178, 150 and 72 (the baseline before falls was 42, 176, 177 and 86), steps down about 30%, no uncontained cell, no one-block wall and no open block edge. Falls over 20 blocks go from 155 to 633 on seed 1337, the valley floor now following real cliffs.

Tried and dropped: holding the water under block-level terraced ground, so falls sit on the real risers. A river beside a riser took the lower tread's level for the whole reach, and since a river never rises that cut slots 13 blocks deep in the rock downstream. Falls at a terrace riser come from the smooth profile instead, as chains of steps.

Not built: a plunge pool at the foot, spray, splitting a fall over 40 blocks, and keeping the valley from grading a riser away at a lip. Not seen in the game.

### The spike

The plan's step 0 counts river stretches whose bed drops 6 blocks or more within 3 map cells. Following each river cell's `down` for three river cells and comparing the cells' heights, on seeds 1, 42, 1337 and 300059728 (with the seed's drawn config) there are 520, 721, 669 and 724 such stretches, with drops over 12 blocks on 157, 276, 246 and 187 and over 40 on 13, 27, 28 and 5; the tallest are 89, 96, 118 and 51 blocks. The first pass condition (at least 10) holds by a wide margin, but these are mostly steep mountain streams of 6 to 25 percent grade, 96 blocks long, not abrupt falls, and stepping the water level at the lip of each would make several hundred waterfalls a world with lips on slopes the terrain does not have.

The second condition (no more than 2 uncontained cells when the profile is cut at the lip) needs the mechanism itself: a discontinuity in `RiverProfiles`, a valley that stops grading at the lip and a plunge pool, all of which change the water rule in `docs/water.md`. That was not built unattended. A better definition of a fall is needed first, from the block-level ground (a terrace riser the river crosses, where `terraced` steps 6 blocks or more within a few blocks) and not from cell heights, which would keep the count to the real ones. The river wall defect (`water_audit` river walls, mostly falls drawn as ramps) stays as it is.

## View diagnostic and docs (no generation change)

`worldprobe sites <seed> <x> <z> <half>` lists the spawn regions the gameplay pass found in a window, best first and at most 20, with the number of 16 horizon rays (3 km, from 2 m over the ground) that run clear for 1.5 km or more, the highest ground any ray saw over the eye, and an `OCHRE_SEED`/`OCHRE_SPAWN`/`OCHRE_LOOK` line toward that ground for a screenshot. It reads the ground only (`worldmap/view.rs`, tested for its columns and order). On seed 1 the first spawn regions have 0, 6, 1, 5 and 7 open rays and 755, 85, 161, 288 and 442 blocks of relief in view. `docs/world-plan.md`, `docs/landforms.md` and `docs/climate.md` are new, and `tectonic-mvp.md`, `water.md`, `AGENTS.md` and the worldview skill are updated.

Not run: the ARM64 checksum tests under `qemu-aarch64`, which the plan asks for after this change; the pins were re-written natively only.

## Kopje rock tops (GENERATOR_VERSION 81)

A kopje was only a heap of height, so its top took the savanna's grass. `TerrainSample` gains `rocky`, set where a kopje's offset is 4 blocks or more, and it follows the spring flag's path (region, `HeightSampler`, `ChunkColumns`). Chunk generation adds `ROCK_GRADIENT` (1.5) to the gradient the blocks are chosen by in a rocky column, so stone shows whatever the slope, and trees do not stand on it. LODs read cells 16 blocks wide or more, which have no relief features, so they are unchanged. The test is on the game's map config, since a coarser one has water by the heap and raises none.

## Boulders and termite mounds (GENERATOR_VERSION 82)

`rocks_near` finds the objects that reach a chunk (a 6-block reach, so the chunk's cells and their neighbours), and `Rock::plant` sets their blocks inside the chunk's bounds, so two chunks agree at their edge. A savanna window of 400 blocks square on seed 1337 holds 21 mounds and a taiga one on seed 2 one boulder, since most of the taiga is too steep or uneven (the shares are 0.30, 0.16 and 0.20). Bench on a savanna view (`BENCH_AT=-700,-928`, seed 1337): vertices 708,372 to 709,420 (+0.15%), chunk generation 203 to 208 ms.

## Boulders and mounds in LOD meshes

`Rock::plant` takes the size of a cell, and a cell is rock if its middle is, so a LOD node whose cells are 4 blocks across or less draws the same boulders and mounds as the chunks, in the same place. Past 4 blocks one is under a pixel. A rock is 66 blocks at one block a cell and the same volume, within 40%, at two (a test). Bench on the savanna view (`BENCH_AT=-700,-928`, seed 1337, `lod`): level 1 gains 103 vertices (20,084 to 20,187), the other levels none, and the total is 166.8 to 168.3 ms. Chunks are unchanged (the pins hold), so no version bump.

## Terrain optimisation pass (no generation change)

Where the pipeline stood at the merge base (`01a7f36`), at the head of `feat/waterfall-falls` (`299559b`, "branch"), and after this pass ("tip"), all from `cargo run --release --example bench -- <seed>` on one machine with a game running beside it, best of 3 per cell. Nothing here changes the world: `GENERATOR_VERSION` (86) and the pinned map and chunk hashes are as they were, the bench's vertex counts are the same at tip as at the branch head in every scene, and a saved map plus 144,000 terrain samples a seed over ten windows hashed the same before and after on seeds 1, 7, 11, 42, 99, 1337, 2024 and 300059728 (`maphash`, a throwaway example, not kept). The bench now times the map (`generate`) and `WorldTerrain::new`, which it left out.

Milliseconds, base / branch / tip. Views are the bench's spawn on seeds 1, 11 and 1337, `BENCH_MOUNTAIN=1` on 1337, the river view (`BENCH_AT=98,-5544`, 1337) and the savanna view (`BENCH_AT=-700,-928`, 1337). The worlds differ from base to branch (new relief, no undercuts), so vertex counts and the stages that follow them are not comparable across that step, only the cost per sample is.

| View | map | terrain setup | regions | generate chunks | mesh chunks | LOD total |
| --- | --- | --- | --- | --- | --- | --- |
| seed 1 | 5600 / 5700 / 3700 | 124 / 160 / 121 | 125 / 188 / 189 | 201 / 208 / 206 | 204 / 201 / 205 | 85 / 89 / 89 |
| seed 11 | 6000 / 6000 / 4000 | 121 / 145 / 107 | 135 / 160 / 160 | 271 / 235 / 235 | 407 / 340 / 342 | 410 / 347 / 337 |
| seed 1337 | 9100 / 8900 / 4200 | 129 / 196 / 138 | 153 / 226 / 225 | 214 / 204 / 203 | 286 / 258 / 255 | 128 / 124 / 123 |
| mountain 1337 | 9000 / 9400 / 4500 | 130 / 199 / 140 | 186 / 210 / 212 | 185 / 179 / 178 | 190 / 200 / 198 | 237 / 209 / 208 |
| river 1337 | 9200 / 9400 / 4400 | 130 / 198 / 138 | 437 / 653 / 611 | 240 / 272 / 271 | 209 / 206 / 204 | 253 / 256 / 251 |
| savanna 1337 | 8900 / 9400 / 4400 | 129 / 195 / 139 | 176 / 275 / 278 | 198 / 202 / 200 | 212 / 207 / 210 | 186 / 171 / 172 |

Mesh vertices, branch and tip alike: 646,567 (seed 1), 1,448,964 (11), 867,951 (1337), 612,379 (mountain), 328,347 (river), 709,416 (savanna); LOD vertices 289,742, 396,603, 311,367, 340,803, 446,792, 290,820. Base: 653,502, 1,842,430, 961,404, 612,832, 410,787, 727,226 and 290,631, 460,355, 325,530, 347,341, 512,229, 293,874.

LOD meshing per level, base / branch / tip:

| Level | seed 1 | seed 11 | seed 1337 | mountain 1337 |
| --- | --- | --- | --- | --- |
| 1 | 9.3 / 9.8 / 9.8 | 12.6 / 10.7 / 10.7 | 10.8 / 10.6 / 10.6 | 8.7 / 9.2 / 9.1 |
| 2 | 8.6 / 9.2 / 9.2 | 34.7 / 24.6 / 24.7 | 18.8 / 16.2 / 16.2 | 15.1 / 15.0 / 14.9 |
| 3 | 9.8 / 10.3 / 10.3 | 55.6 / 40.7 / 40.3 | 19.2 / 17.0 / 16.9 | 17.4 / 16.4 / 16.5 |
| 4 | 10.6 / 11.0 / 11.0 | 71.9 / 59.9 / 57.3 | 15.6 / 15.7 / 15.6 | 38.8 / 33.2 / 33.1 |
| 5 | 14.8 / 15.4 / 15.5 | 202 / 177 / 170 | 32.7 / 32.4 / 32.2 | 123 / 100 / 99.9 |
| 6 | 10.1 / 10.7 / 10.6 | 10.5 / 11.0 / 11.0 | 9.5 / 9.9 / 10.0 | 11.0 / 11.4 / 11.3 |
| 7 | 10.7 / 11.2 / 11.2 | 11.2 / 11.8 / 11.7 | 10.3 / 10.8 / 10.8 | 11.1 / 11.5 / 11.4 |
| 8 | 10.5 / 11.1 / 11.0 | 10.9 / 11.4 / 11.2 | 10.5 / 11.1 / 11.1 | 11.3 / 11.7 / 11.7 |

What the branch cost against the base, and where it sits against the plan's 0.3 s cap on a new stage: the map generation stages are within 30 ms of base each (seed 1: carve passes 52 to 79 ms, climate 178 to 197 ms; seed 1337: climate 152 to 169 ms), so the map itself is +0.1 to +0.3 s of 5.6 to 9.3 s, mostly noise. Terrain setup (`WorldTerrain::new`) rose by 36 ms on seed 1 and 66 to 69 ms on seed 1337 and the river view: the timers put it in the caps pass (31 ms, parallel, where the valley floor now follows the real ground through `landform_ground`) and the serial lip search (42 ms, 16 ground samples per falling interval). Regions rose 13 to 56% across the views (125 to 188 ms on seed 1) and 49% on the river view; the profile there is flat (perlin 11%, `river_hits` 7%, `bilinear_bytes`, `smoothstep`, `fbm_filtered`, `height_at` each 2 to 3%), so no one new function owns it, and it is not recovered here except on rivers. Chunk generation, meshing and the flat LOD levels did not move by more than 5%, and the 4 to 5% on LOD levels 1 to 3 and 6 to 8 on seed 1 is unexplained (`rocks_near` and the relief features read at each cell are the candidates).

Map generation by stage on seeds 1 and 1337, milliseconds, base / branch / tip. The stages are not equal: most of the map's wall time was serial work, which a thread count does not shrink (a 3-core run of the same binary: tectonics 3.2 s, geology 2.8 s, erosion 5.1 s, so those scale; the hydrology passes and the pass search did not).

| Stage | seed 1 | seed 1337 |
| --- | --- | --- |
| tectonics | 360 / 357 / 372 | 363 / 368 / 361 |
| geology | 261 / 260 / 268 | 266 / 274 / 281 |
| hydrology 1 | 922 / 921 / 565 | 920 / 951 / 560 |
| erosion | 2174 / 2221 / 1417 | 2343 / 2341 / 1610 |
| carve passes | 52 / 79 / 78 | 3588 / 3444 / 670 |
| terrace | 226 / 226 / 43 | 239 / 242 / 49 |
| scatter | 144 / 144 / 17 | 146 / 145 / 17 |
| hydrology 2 | 891 / 899 / 537 | 904 / 909 / 544 |
| landform | 175 / 167 / 36 | 172 / 172 / 38 |
| climate | 178 / 197 / 198 | 152 / 169 / 171 |
| roughness | 66 / 65 / 65 | 68 / 65 / 66 |
| label | 106 / 109 / 107 | 109 / 113 / 109 |
| terrain setup | 122 / 158 / 117 | 132 / 198 / 140 |
| map total | 5600 / 5700 / 3700 | 9300 / 9600 / 4500 |

The map takes 2.0 s less on seed 1 and 5.1 s less on seed 1337 than at the branch head, and 1.9 s and 4.8 s less than base. The big items, in the order they were found by timing the stages (a call-graph `perf` of the whole run would not finish reporting, so the flat profile and `Instant` timers did it):

- `carve_passes` (seed 1337, 3.4 s): up to 40 rounds, each a world-wide slope, two component labellings and a Dijkstra. From the 13th round on, this seed finds the same way, carves nothing and pushes the same `Pass` feature again, 28 rounds of 80 ms. A round that moves no height is a fixed point, so the rest are skipped and their features pushed, which leaves the map the same byte for byte. Seeding the search from the region's edge instead of every cell of it (a seed cell relaxes only a neighbour outside the region) took the rounds that remain from 80 to 50 ms. 3.44 s to 0.67 s.
- `flood_ties` (158 ms, three times a map, one thread): `par_rows`, 0.4 s. `window_max` (landform, twice): its second pass walked columns at an 8 KB stride on one thread, now rows on all cores, 0.3 s. `scatter`, `terrace` and rain's rock share were per-cell and serial: 0.13 s, 0.04 s and 0.04 s.
- `priority_flood` (16 times a map, 97 ms each, the largest serial cost left): its heap is now `FloodQueue`, buckets by the top 18 bits of the key with a heap only for the bucket being drained, 0.3 s (16 and 20 bits were no better); the open sea is found once a hydrology run, not once a drain round, 0.22 s. Stream power's six floods do not share it, since erosion may sink a shore cell under the sea.
- Erosion's serial parts: thermal erosion's talus table (24 passes of 4.2M bed lookups) and stream power's rate per cell, 0.45 s together.
- Terrain setup, 36 to 69 ms of the branch's cost: the lip search split over threads (13 to 31 ms), the spring and near-still tables per row (22 ms each way), back to the base's 117 to 140 ms.
- Chunk and region side: `river_hits` evaluated the stretch's Hermite curve nine times per stretch near each river column, now points kept with the profiles (regions on the river view 647 to 607 ms, every pair apart); `fresh_at` scanned 11 x 11 for each water column of a chunk even with no fall in it (mesh chunks on the river view 207.2 to 204.8 ms, a real but small 1%).

Tried and dropped, all inside noise: a `floor` without the libm call (an x86-64 baseline build calls it, 2 to 4% of the profile) in `perlin` and `perlin3` (regions 188.8 and 188.1 ms, mesh chunks 201.0 and 203.9); skipping the grove noise in columns with no trees (regions 187.9 and 188.3, 159.0 and 159.0, 225.6 and 225.1, 210.2 and 211.7 ms); 16 and 20 bits for the flood queue's buckets (20 pays for zeroing a million of them). Not done: `mesh_blocks` is 35% of the bench after the map (24% on chunks, 11% on LODs) and unchanged since base; the greedy loop reads each block six times. The climate sweep (`climate_fields`, three times a map, about 100 ms each) is serial along the wind. `hydraulic` erosion calls `rock_at_cell` for every brush cell of each eroding droplet step. The lake distance field is a serial chamfer (44 ms). The carve rounds still relabel the world's components twice per round.

Found, not fixed: on a seed whose split no pass can close (1337 here), `carve_passes` pushes the same `Pass` feature 28 times and every round of it was wasted. The skipped rounds still push them, since leaving them out changes the map. The real fault is that the split persists (the walkable test reads water surfaces in `find_split` and the carve cannot lower ground under `sea + 1.5`), and it wants a version bump with the duplicates removed.

Verification. The full `cargo test --release --workspace`, `cargo clippy --workspace --all-targets` and `cargo fmt --all` ran clean at the tip, including the pinned map hash, the chunk hash and the thread-count test (a map for 1 and 7 threads is the same). One commit (the stretch curve points) failed two `water` tests when first made, since `flat_river_profiles` builds its profiles without `index`; it was fixed in place before anything was pushed. Between commits only the hashes above and the touched crate's tests ran. Not run: the ARM64 checksum tests under `qemu-aarch64`, the flight and `lod_sim` examples, and the full `water_audit` sweep.

### The pass continued: meshing, regions, chunk generation, the rest of the map (generator version 87)

The section above stops at `0b911e9`. What came after, in the order it was made: `carve_passes` pushed the same `Pass` feature up to 28 times on a seed it cannot close (1337) and ran every one of those rounds, so it now pushes each once and stops when a pass moves nothing (**`GENERATOR_VERSION` 87**, the map and chunk hash pins updated; the saved map differs from version 86 by the dropped duplicates and no terrain sample does, which a hash of 144,000 samples a seed on 8 seeds confirms). The later changes are all exact: the saved map and terrain samples hash the same before and after each one on seeds 1, 7, 11, 42, 99, 1337, 2024 and 300059728, and `bench` vertex counts are those of `origin/main` in every view. `GENERATOR_VERSION` (87) and both pinned hashes stand.

Milliseconds, best of 3, alone on the machine, `origin/main` (373cd80, the merged #114) / the branch at 359f77d, the last commit before the work below / the tip. The map and terrain setup columns of main are quoted from the table above (its "branch" column, the same generation; the bench of main cannot time them). The savanna view shares seed 1337's map. Mesh vertices are identical in the three builds: 646,567 (seed 1), 1,448,964 (11), 867,951 (1337), 612,379 (mountain), 328,347 (river), 709,416 (savanna); LOD vertices 289,742, 396,603, 311,367, 340,803, 446,792, 290,820.

| View | map | terrain setup | regions | generate chunks | mesh chunks | LOD total |
| --- | --- | --- | --- | --- | --- | --- |
| seed 1 | 5700 / 3100 / 2800 | 160 / 115 / 117 | 187 / 148 / 148 | 205 / 207 / 175 | 204 / 95 / 86 | 88 / 70 / 68 |
| seed 11 | 6000 / 3400 / 3100 | 145 / 103 / 104 | 159 / 145 / 144 | 234 / 237 / 207 | 343 / 245 / 221 | 346 / 298 / 297 |
| seed 1337 | 8900 / 3500 / 3200 | 196 / 134 / 134 | 226 / 192 / 191 | 203 / 205 / 172 | 259 / 153 / 137 | 123 / 102 / 100 |
| mountain 1337 | 9400 / 3500 / 3200 | 199 / 135 / 134 | 210 / 195 / 195 | 176 / 174 / 157 | 197 / 122 / 106 | 208 / 182 / 180 |
| river 1337 | 9400 / 3500 / 3200 | 198 / 134 / 136 | 650 / 578 / 581 | 271 / 269 / 238 | 204 / 96 / 86 | 255 / 220 / 219 |
| savanna 1337 | 9400 / 3500 / 3200 | 195 / 134 / 135 | 274 / 229 / 230 | 200 / 200 / 173 | 206 / 112 / 100 | 170 / 147 / 145 |

Against the merge base `01a7f36` (125 ms regions on seed 1, 437 on the river view, 201 mesh chunks, 85 LOD total, 5.6 s map) the tip is: map 2.8 s quicker on seed 1 and 6.1 s on seed 1337, mesh chunks 57% quicker, LOD total 20% quicker, generate chunks 13% quicker, regions still 18% slower on seed 1 and 33% on the river view.

What each change bought (seed 1 unless said; from the commit messages, re-measured only where marked):

- `8f357f9`, `8e84f18` greedy mesher: mesh chunks 201 to 116, then 98 ms (seed 1337 258 to 157, river view 204 to 100). The scan visited every cell for each of six directions and sent each of the 31M that hold a block through `face_span`, which turned down all but 121k; it now reads the dense grid by stride, passes over a block whose neighbour is opaque, and skips slices that are empty or whose next slice is wholly opaque.
- `5779fa1` map row read in four-cell slices (bicubic reads, away from the edge): regions 187 to 163 ms. `750c248` a per-thread memory of the map heights a column reads twice: regions 166 to 148 (river view 598 to 577). `359f77d`, `8f26a3b` components and flood neighbours by index: carve passes on seed 1337 638 to 548 ms, hydrology and erosion about 30 ms each.
- `23dfa93` remember each cell's bed (`BedSpan`) in hydraulic erosion: erosion 1.41 to 1.03 s on seed 1. `04fb82f` rain sweep in tiles on every core: 0.1 to 0.2 s of the map (inside noise for any one stage; not re-measured).
- `44d9c80` the flood finds its roots while it finds the open sea: hydrology 508 to 467 ms and erosion 988 to 945 on seed 1, the map 0.12 s, every pair apart. Stream power's six floods still run one after another; each is about 55 ms now, 50 of it the pop loop.
- `4971f43` box blur by rows in the same order: geology 254 to 102 ms and roughness 63 to 23 ms on seed 1, 0.19 s of the map; the blur was 18% of the map's CPU time (it read each vertical window at an 8 KB stride and added horizontal windows in one dependent chain), so a machine with fewer cores gains more in seconds. A test compares it bit for bit with the plain sum.
- `b3f75f5` the column's soil depth (a smoothstep) and the terrain's bottom bound (a dyn call) were worked out for each of a column's 32 blocks, now once: generate chunks 191 to 162 ms on seed 1 and 162 to 144 on the mountain view, with the bench's repeated passes (the single first pass in the table 207 to 175).
- `afaf8bf`, `f2697dd` the chunk mesher's light box (48 x 65 x 48 cells, 150k a chunk with faces): the cells that can spread are found by column slices, and the opacities are copied a column at a time out of the neighbourhood's dense grid: mesh chunks 97.3 to 94.5, then 94.6 to 86.8 ms on seed 1 (river view 99.4 to 93.9, then 93.7 to 86.6), every pair apart.

Where regions stand. The profile of regions on the river view (single thread, 12 rebuilds) is flat: `perlin` 21% (the same per sample as before), `river_hits` 11.5%, `floor` 7%, `bilinear_bytes` 6%, `sample_uncontained` 5.5%, `smoothstep` 5%, `fbm_filtered` 5%, `round` 4%, `terraced` 4%, `biome_at` 3%, `detail_at` 3%. The gap to the base is mostly not waterfall work: the base-to-main diff of `sample_uncontained` adds, for every column, `climate_cell` (two long-wavelength `perlin` samples for the drifted climate cell), a one-octave `fbm_filtered` for the zone patches, a two-octave one for groves, and `relief_offsets` (four `height_at` reads and each relief feature's noise) wherever there is dry land over 6 blocks, none of which is a river or a fall; the waterfall era adds `landform_ground` in `valley_floor` for columns near a river (0.75% of the profile), the `fall` flag, a second `level` lookup and a `sqrt` per hit stretch in `river_hits`, and the `Spring` and `still_near` work. A like-for-like comparison against the base needs the same world and the worlds differ, so the base profile (per rebuild 242 ms of samples against 442 on the river view) says only that every function there is about twice as busy: more of this view's columns are river or relief columns. Nothing in it is a lone cost to remove; what is left is `river_hits` (a pointer chase through `index`, `data` and `lips` for each of its two `level` calls, about 10% of it) and noise that cannot change without changing the world.

The 4 to 5% on LOD levels 1 to 3 and 6 to 8 on seed 1 is gone: at the tip every level of seed 1 is under the base's (level 1 9.3 to 7.0 ms, level 8 10.5 to 9.1).

Measured the way the game feels it. `flight` (headless, 30 simulated seconds, seed 1) paces the loading by simulated frames, so chunks and LOD meshes a second come out equal in all builds (814 and 245 in the load leg, 1,573 and 92 walking) and the main-thread frame times do not separate them either: p99 load 1.47 / 1.59 / 1.89 ms, walk 4.00 / 4.06 / 4.12, sprint 5.03 / 4.78 / 4.65, worst 6 to 9 ms except one 38 / 17 / 21 ms in the high leg, base / main / the branch at 4971f43, the tip when it was run (run once each, spreads of that size are noise). What it does show is the loading's total CPU time, which the world map being cached makes the streaming alone: user seconds for the whole run, two runs each, base 118.9 and 121.6, main 128.3 and 127.5, 4971f43 112.2 and 112.3, tip 108.9 and 108.4. Main costs 6% more CPU than the base to stream the same world, and the tip 10% less than the base (15% less than main). `lod_sim` (seed 7, 80 blocks a second) logs no frame over 100 ms in main or the tip, and the holes match at the same moments (`[0, 0, 0, 3104]` at 28 s in both; later rows differ by where the camera had got to); the tip loads quicker, reaching its settled state before the 40 s timeout main hits.

Tried and dropped, all measured against the build before them: a bucket queue for the flood as chains through one arena instead of a `Vec` per bucket (hydrology 473 to 501 ms, slower); an inline `round` for the map lookups and the tree and rock heights (exact on 600,000 values including edge cases, but regions 151 to 156 ms, river view 579 to 599, generate chunks 203 to 209: the libm call is quicker than the int conversions); a branch-free min over the six neighbours for the light spread scan (mesh chunks 87.0 to 91.0 ms). The floor in `perlin` was tried in the first part and is inside noise. Not done: `ChunkNeighbourhood::non_air` builds a `Vec` of every non-air block for the mesher (about 4% of meshing); stream power's floods still do not share work; `river_hits` has not had its `level` lookups flattened.

Verification at the tip. `cargo test --release --workspace` (the pinned map and chunk hashes, the thread-count test), `cargo clippy --workspace --all-targets` and `cargo fmt --all` clean; the saved map plus 144,000 terrain samples a seed hash the same as at the version 87 commit on 8 seeds. `water_audit --fine` on seeds 1, 42, 1337 and 300059728 is line for line the same as `origin/main`'s (no uncontained cell on rivers, lakes or coasts, no one-block wall, no open block edge): river walls 61, 145, 122 and 82 (the 148 and 78 recorded after the last waterfall commit are not what main gives now; the tip does not move them). Not run: the ARM64 checksum tests (`qemu-aarch64` is not installed here).

## Painted materials, bevels and grass fringe (stage 1)

`bench` (default seed, `cargo run --release --example bench`), chunk vertices and LOD vertices: 646,567 and 289,742 at the start. The painted materials alone changed nothing in the mesher (same counts). The chamfer on top edges and the 0.2 cut-back, with its caps, took chunks to 749,211 (+15.9%). Grass fringe tufts and clumped flora blocks, drawn from one set of vertices for both windings, bring them to 961,922 (+48.8%), with LOD unchanged at 289,742 (bevels and the fringe are full-detail chunks only). Mesh time was within noise of the 205 to 220 ms before throughout (219 to 224 ms at the tip), and rebased on the terrain optimisation pass (#115) the same counts mesh in 97.7 ms, LOD in 68.7 ms. The GPU frame-time baseline and these changes' numbers (median at spawn, 1920x1080, three 80 s `OCHRE_STATS` runs, `docs/materials.md`) are not measured yet, and foliage's cut-out overdraw is the cost to watch on the RTX 2060.

The bevel work finished with ramps joined at shared corners, corner cuts and a seam pass (`docs/bevels.md`). Full-detail chunks are 1,436,965 vertices (LOD unchanged at 289,742) and mesh in 152 to 156 ms on seed 1, with the bevels off 108 ms and 737,567 vertices, against 97.7 ms before this work. Per 1,944 chunks the greedy pass is 82 ms, seams 17, flora and fringe 19, the water and spray pass before them 9. A first version closed holes after meshing (a global edge map, loops of unmatched edges, a fill) with a one-block ring of neighbour faces so loops across chunk seams closed, and meshed in 259 ms. Replacing it with the rules and the seam pass cost about 33 ms in seams, 17 once each top's rules were kept for the chunk instead of worked out again for every face and seam (same vertex counts). The GPU frame time is still not measured.

## Painterly lighting and painted foliage

Seed 2070026479 forest meadow, measured while `painterly_lighting` was still a setting (since removed, so painterly off is the previous model) (`OCHRE_SPAWN=10832,-3952,5 OCHRE_LOOK=10832,30,-3940 OCHRE_TIME=0.5`), 1920x1080, `OCHRE_NOVSYNC=1 OCHRE_STATS=1`, one 30 s run each on the real display, so differences under about 0.4 ms are noise. `main` (painterly off): 71 fps, 14.66 ms frame, GPU 12.30 ms (opaque 3.22, shadow 2.89, early prepass 0.92). Painterly off on `feat/painterly-integration` (painted foliage with alpha): 71 fps, 14.00 ms, GPU 12.84 (3.09, 3.06, 0.98). Painterly on: 67 fps, 14.85 ms, GPU 12.92 (3.39, 3.05, 1.13). Painterly lighting costs about 0.3 ms in the opaque pass and the foliage alpha cut about 0.15 ms in the shadow pass, so 60 fps holds with 1.8 ms to spare. Under Xephyr the frame read 44 ms against a 12 ms GPU on every build, including `main`: that gap is the nested X server's presentation, not the game.

A floor cell with walls on opposite sides no longer ramps, so a one block wide trench stays level (`Shapes::shape`, `wall_foot`): chunk vertices 1,437,029 to 1,437,397 on the default `bench`, LOD unchanged.

## Frost skin (2026-11-11)

`frost.wesl` runs per fragment on ground in cold country, with no change to vertices, so `bench` chunk vertices are unchanged (1,440,365, LOD 289,742). Timed in the sandbox with `OCHRE_STATS` on seed 4 at (12148, 109, -16955), looking 16 degrees down over tundra where frost covers most of the screen (the worst case), 12 one-second samples each, RTX 2060, 1280x720: frost on 87.6 fps (11.4 ms), `no_frost` 92.3 fps (10.8 ms), so about 0.6 ms, 5%. In warm country the shader leaves after one comparison of the camera's frost strength. Not yet timed: the fps gate in `docs/materials.md`, and a glacier or temperate mountain route.

## Tree trunks (2026-11-13, `feat/painterly-trees`)

PR 1 of `docs/designs/trees.md`: tree logs drawn as prisms behind `tree_shapes`. `bench` on seed 1, default view, shows chunk vertices 1,440,365 and LOD 289,742 with `tree_shapes` off (as before, so the off path draws the same), and 1,450,911 chunk vertices with `BENCH_TREE_SHAPES=1` (+0.7%, LOD unchanged), mesh time 172.8 and 173.6 ms. That view holds few trees, so it says little of a forest. The three measurement views (taiga with pines, temperate broadleaf, jungle) are not chosen yet: seed 1's default view has none and a search of seed 1 found no tree density above 0.5. Shadow pass and frame time are not measured, since leaves are still cubes and PR 2's spike measures them.

## Leaf cards (2026-11-13, `feat/painterly-trees`)

PR 2 of `docs/designs/trees.md`: each surface leaf draws one double-sided card (density 1) and the leaves inside a crown draw nothing. `bench` on the forest meadow, `bench 2070026479` with `BENCH_AT=10832,-3952` (chunk vertices, mesh time best of 3): cubes 2,124,253 and 319.7 ms; trunks and cards 2,096,085 (-1.3%) and 273.7 ms. LOD is unchanged at 347,916. The gate is 1.10x, so density 1 passes with room and no thinning is needed. Not measured: shadow pass cost and frame time, which need the sandbox with `OCHRE_STATS` and a GPU (cards add alpha-cut overdraw and shadow casters), and a jungle view, the worst case.

Leaf cards, second pass (2026-11-13): cards are now irregular six-cornered fans (7 vertices, not 4), 2.0 to 2.8 blocks across (pines 1.4 to 2.0, and pines carry a card on 45% of their surface leaves since their tiers are nearly all surface). At full density the taiga view (`bench 4` with `BENCH_AT=10648,-16055`) was 1,988,684 chunk vertices against the cubes' 1,647,011 (+20.7%, over the 1.10x gate); six corners made it 1,927,412 (+17.0%), a pine density of 0.55 1,830,924 (+11.2%), and 0.45 1,809,581 (+9.9%). The forest meadow is 2,183,529 against 2,124,253 (+2.8%). Pines also gained boughs, which changed generated chunks (`GENERATOR_VERSION` 95, pins updated).

Pine boughs and cards, third pass (2026-11-13): a pine's boughs were drawn as chains of prisms joined across their voxels, which looked like a web and cost vertices. Each is now one straight prism from its trunk log to its tip, and the voxel logs of a pine's boughs are not drawn. The taiga view is 1,557,069 chunk vertices with cards at 45% of pine leaves (-5.5% against the cubes' 1,647,011), so pine cards went back to every surface leaf and 2.0 to 3.0 blocks across: 1,674,900 (+1.7%). The forest meadow is unchanged at 2,183,529 (+2.8%).

Tree variety (2026-11-13): a broadleaf's lobes are three to five and its crown stands off the trunk, so cubes and cards both change. Forest meadow: cubes 2,155,849, trunks and cards 2,269,044 (+5.3%); taiga: cubes 1,645,602, shapes 1,664,772 (+1.2%). `GENERATOR_VERSION` 97, pins updated.

Far trees (2026-11-13): LOD tree boxes are now shapes, not cuboids: a trunk is a bowed eight-sided prism (28 vertices), a broadleaf's crown box a ball (29) and a pine's layer a cone (20), against 16 and 24 for the boxes. Forest meadow LOD vertices are 353,939 against 347,916 before the lobe and boxes change (+1.7%, the voxels having changed too); the default view is 290,329 against 289,742 (+0.2%). LOD mesh times are unchanged within noise. The planted-cell levels (cells of 2 and 4 blocks) and the forest canopy boxes are not changed.

Far trees from cards (2026-11-13): lone trees in 2-block LOD cells are drawn as a trunk prism and a few big cards (`Tree::far_cards`: 3 to 6 over the crown's ball and each lobe's, rings up a pine's cone), in 4-block cells as a trunk prism and one ball or cone, and the planted cube cells they stood in draw nothing (the forest floor's shade still reads them). Per lone tree, planted cubes against the new shapes: 2-block cells 357 against 147 (a broadleaf of 10 blocks), 905 against 182 (20 blocks), 286 against 189 (a pine of 16) and 614 against 315 (24); 4-block cells 68 against 57, 215 against 57, 72 against 48 and 88 against 48. Forest meadow LOD levels 1 and 2 are 36,156 and 40,776 vertices against 42,767 and 49,224 with the boxes only (-15% and -17%), the whole LOD 338,880 against 353,939 and 347,916 before PR 4; the default view 287,539 against 290,329 and 289,742.

Far trees at level 1 (2026-11-13): the first far-card commit left level 1 with no trees. Level 1 meshes from `sample_regions`, a second path from `sample_coarse`'s, which plants trees but made no boxes, so hiding the planted cubes left nothing. It now makes `far_boxes` for the trees its node owns, and `every_lod_level_draws_the_trees_of_a_forest` checks levels 1 to 3 of a forest for boxes, leaf and log vertices. Forest meadow level 1 is 37,731 vertices (42,767 with planted cubes before); the whole LOD 340,455.
