# worldmap

## Overview
The world's geography as a coarse grid, made once per seed: continents, ranges, rock, rivers, lakes, climate and gameplay features. `WorldMap` (`mod.rs:WorldMap`) is a plain value of row-major `[z][x]` layers (default 32-block cells over a 65 536-block world, `mod.rs:WorldConfig::default`) that saves to disk and loads back, so nothing downstream depends on the float maths that made it. `WorldTerrain` (`source.rs:WorldTerrain`) wraps a map as the `ochre_base::terrain_source::TerrainSource` that `ochre-terrain` generates chunks and LODs from, adding block-scale detail, rivers, lake holds, dunes, relief features and biome selection on top.

Pipeline, in the order of `mod.rs:generate`:

| Stage | Entry point | Writes |
|---|---|---|
| tectonics | `tectonics.rs:generate` | `height`, `continentalness`, `plate`, `boundary`, `uplift`, `geological_age` |
| geology | `geology.rs:generate` | `geology`, `strata_offset`, `facies`, `bedded`, `flowed` |
| hydrology 1 | `hydrology.rs:run(map, false)` | lakes, drainage, rivers (no carving) |
| erosion | `erosion.rs:run` | `height` (stream power, hydraulic, thermal) |
| landforms | `shapes.rs:fjords, cones, inselbergs, cold_uplands, karst, cirques` | `height` |
| passes | `gameplay.rs:carve_passes` | `height`, `Pass` features |
| beds | `landform.rs:run`, `landform.rs:terrace` | `landform`, terraced `height` |
| scatter | `hydrology.rs:scatter` | `height` |
| hydrology 2 | `hydrology.rs:run(map, true)` | `flow`, `down`, `water_kind`, `water_surface`, `river_width`, carved `height` |
| landform again | `landform.rs:run` | `landform` |
| climate | `climate.rs:run` | `precipitation`, `temperature`, `moisture`, `biome`, `biome_alt`, `biome_margin`, `trees` |
| roughness | `detail.rs:roughness_layer` | `roughness`, `range` |
| label | `gameplay.rs:label` | `features`, `ford` |

`character.rs` draws the plate count, relief, warmth, humidity, gradient, continentality and wind from the seed before generation (`character.rs:derive`), and the landform counts and shares that are not stored in the map (`character.rs:LandformKnobs::from_seed`). Design background lives in `docs/water.md`, `docs/landforms.md`, `docs/climate.md` and `docs/world-plan.md` at the repo root.

## Interface
- `generate(seed, WorldConfig) -> WorldMap` (`mod.rs:generate`); `io.rs:load_or_generate(dir, seed, config)` is the cached entry point that `../lib.rs:load_world` uses.
- `WorldConfig` with `validate`, `dim`, `half_size` (`mod.rs`); `WorldMap` with `empty`, `cell_centre`, `to_cell`, `index`, `dim`, `bilinear`, `bilinear_bytes`, `bicubic`, `height_at` (monotone cubic), `shared`; `WaterCell`; `NEIGHBOURS`; `NO_FLOW`; `GENERATOR_VERSION`; `SoftCache` (`mod.rs`).
- `WorldTerrain` (`source.rs`): `new(Arc<WorldMap>)`, `map`, `dry_ground`, `relief_at`, and the `TerrainSource` impl (`bounds`, `seed`, `biomes`, `sample_for_cell`, `rock_column`, `climate`, `temperature`, `tree_bound`, `cliff_boost`, `find_spawn`).
- `Feature`, `FeatureKind` (`Pass`, `Ford`, `BridgeSite`, `Spawn`, `Iron`, `Coal`, `Clay`, `Salt`, `Cave`, `Hotspot`) (`features.rs`), re-exported from `mod.rs`.
- `tectonics.rs`: `TectonicConfig`, `Plate`, `Tectonics`, `Sample`, boundary codes `COLLISION`, `SUBDUCTION`, `EXTENSION`, `SHEAR`, plate rock codes `ROCK_BASIN`, `ROCK_SHIELD`, `ROCK_VOLCANIC`, `Tectonics::plate_rocks`.
- `geology.rs`: `Rock` (with `hydraulic_erodibility`, `stream_erodibility`, `block`), `RockColumn`, and on `WorldMap`: `rock_at_cell`, `rock_at`, `rock_column`.
- `hydrology.rs`: `run`, `scatter`, `river_width(area)`, `river_depth(area)`. `water.rs`: `RiverHit`, `RiverProfiles` (`index`, `near`, `falls`), and on `WorldMap`: `river_at`, `river_hits`, `river_tangents`, `flat_river_profiles`, `build_river_profiles`.
- `climate.rs`: `ClimateConfig`, `BiomeKind`, `classify(temperature, moisture)`, `treeline(warmth)`, `LAPSE_PER_BLOCK`, `run`.
- `detail.rs`: `detail`, `gullies`, `gully_relief`, `foothill_weight`, `Dunes`, `dunes`, `roughness_layer`. `erosion.rs`: `run`. `landform.rs`: `MOUNTAIN`, `run`, `terrace`.
- `gameplay.rs`: `carve_passes`, `label`, `validate(&WorldMap) -> Vec<String>`. `view.rs`: `Site`, `MOST_SITES`, `view_sites(terrain, x, z, half)`.
- `character.rs`: `LandformKnobs`, `derive`, `world_config`, `world_config_with`.
- `mod.rs:test_terrain(seed)`, `forest_spot`, `tree_spot`, `try_tree_spot`: only under `cfg(any(test, feature = "test-support"))`.
- `shapes` and `source` are private modules (`mod.rs`); `shapes.rs` functions are `pub(super)`. `shapes.rs` also holds `cold_uplands`, `inselbergs`, `karst` and `cirques` although its module doc names only fjords and cones.

## Invariants
- A seed gives the same map on every machine and for any thread count: `portable_maths.rs:test_the_map_is_the_same_on_every_machine_and_for_any_thread_count` pins `MAP_HASH` of `generate(1, default)` at 1 and 7 threads. A change in output needs a `GENERATOR_VERSION` bump (comment on `MAP_HASH`; `mod.rs:GENERATOR_VERSION` doc).
- World generation calls no platform `exp`, `powf`, `sin`, `cos` and the like; use `libm`. `portable_maths.rs:test_world_generation_calls_no_platform_maths` scans `ochre-base/src`, `ochre-world/src/worldmap` and `ochre-terrain/src/generate`, skipping `generator/greedy.rs` and `#[cfg(test)]` modules.
- Parallel work uses `par_rows` (rows split across `worker_threads`: logical CPUs minus two, at least 1) and erosion droplets run in tiles, each tile kept apart from the next of its round, so no tile races another (`mod.rs:par_rows`, `mod.rs:worker_threads`, `erosion.rs` module doc, `erosion.rs:TILE`).
- Block-scale detail at full detail is integer hash noise with no `sin`, `exp` or `powf` (`detail.rs` module doc); wider LOD cells may use `resolved` noise.
- `WorldConfig::validate` demands 3..=64 plates, relief in 0..=2, a size that is a multiple of `map_cell` and at least 8 cells across; `generate` panics on an invalid config (`mod.rs:WorldConfig::validate`, `generate`).
- `ClimateConfig::validate` demands warmth 0..=1, humidity 0.1..=3, gradient 0..=0.5, continentality -0.3..=0.3, a finite nonzero wind vector (`climate.rs:ClimateConfig::validate`).
- Stage order matters: fjords, cones and the other landform shapes run after erosion so thermal slumping cannot soften them; passes run before rivers settle; `terrace` and landform run before water so rivers lie over the treads they stand on (comments in `mod.rs:generate`, `shapes.rs` module doc).
- `landform::run` reads ground only, never climate or water (`landform.rs` module doc). `soft` (`SoftCache`) is never saved and never compared: `PartialEq` always returns true (`mod.rs:SoftCache`).
- Map file: magic `RCWM`, a version equal to `GENERATOR_VERSION`, each layer length-checked against the cell count, a trailing FNV-1a checksum, and no trailing bytes; any failure is an `InvalidData` error (`io.rs:from_bytes`, `checksum`, `Reader::layer`).
- `WorldTerrain::sample_for_cell` raises a dry full-detail block beside river water to the water's top block, using uncontained neighbour samples so the result is independent of sample order (`source.rs:sample_for_cell`).
- Water containment constants and rules (`source.rs:HOLD`, `DROWN`, `JOIN`, `contain`, and `water.rs:freeboard`) keep lakes and rivers from leaking; see `docs/water.md` before changing them (`source.rs`, `water.rs`).
- `BiomeKind::Mountain` is a scratch value; the saved `biome` layer never holds it (`climate.rs:BiomeKind::Mountain` doc).
- Gameplay `validate` only reports; it does not stop a seed loading (`gameplay.rs` module doc).

## Side Effects
- `io.rs:WorldMap::save` creates the parent directory, writes `<path>.tmp` and renames it over `path`, so a crash never leaves a half map. `load` reads the whole file. `load_or_generate` regenerates on a missing, damaged or other-version file, saves the result, and logs with `tracing::warn!` on read or write failure. Cache name: `map-v{GENERATOR_VERSION}-seed{seed}-{size}m-{cell}cell-{config checksum}.rcmap` (`io.rs:cache_path`).
- `OCHRE_CHARACTER=off` makes `character.rs:world_config` keep the config as given; `OCHRE_STAGE_TIMES` prints per-stage timings to stderr in `mod.rs:generate`.
- Spawns scoped OS threads for map generation (`mod.rs:par_rows`, `water.rs:build_river_profiles`).
- `source.rs` keeps a 65 536-entry thread-local `HEIGHT_MEMO` keyed by terrain id and coordinates, and takes ids from a global `NEXT_TERRAIN` atomic. `WorldTerrain::new` precomputes river profiles, springs, lake fields and `near_still` before returning.
- `generate` mutates the map in place stage by stage; every stage takes `&mut WorldMap`.

## Verification
- Run: `cargo test --release -p ochre-world worldmap`
- Terrain's tests borrow maps through `cargo test --release --workspace` via the `test-support` feature (`mod.rs:test_terrain`).
