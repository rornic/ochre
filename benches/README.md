# benches

## Overview
One criterion benchmark, `terrain.rs`, registered in `Cargo.toml:[[bench]]` as `terrain` with `harness = false`. It times the terrain pipeline on seed 1 with `WorldConfig::default()`. For single-thread stage timings and vertex-count checks use `examples/bench.rs` instead (see [../examples/README.md](../examples/README.md)).

## Interface
- `terrain.rs:main` runs `benches()` (criterion groups below), then `Criterion::final_summary`, then `report_percentiles`.
- Criterion groups (`terrain.rs:criterion_group!`): `chunk_generation` (`chunk_gen`, `Plains|Mountains|Coast/cold`), `terrain_samples` (`terrain_sample`: `height`, `biome`, `climate`, `coarse_cell_64`), `world_map_samples` (`worldmap_sample`: `bicubic_height`, `bilinear`, `terrain_height`, `terrain_biome`, `terrain_water`), `world_generation` (`worldmap_generate/default_world`, sample size 10), `lod_nodes` (`lod_node/level_1|3|5|7`).
- `terrain.rs:report_percentiles` prints p50 and p99 in ms over `CHUNKS_PER_KIND` (128) chunks of each `Kind`, warm (shared `RegionStore`) and cold (fresh store each).
- `terrain.rs:classify` and `chunks_of` pick surface chunks on a 192-block lattice over 80% of `WORLD_HALF_SIZE`.

## Invariants
- `terrain.rs:SEED` is 1 and the world is `WorldConfig::default()` passed straight to `worldmap::generate`, without `worldmap::character::world_config`. Examples such as `bench`, `worldprobe` and `seams` do apply it, so their worlds differ from this one for the same seed.
- `terrain.rs:WORLD_HALF_SIZE` (32_768) is hard-coded and matches the default `size` of 65536. A different default size makes `chunks_of` sample off the map.
- `terrain.rs:chunks_of` asserts that every `Kind` finds at least one chunk.
- The cold chunk benchmark builds a new `RegionStore` per iteration (`BatchSize::PerIteration`), so it includes region setup. The warm percentile run uses `with_resident_cap(100_000)`.
- Perf results belong in `docs/terrain-perf.md` (per `AGENTS.md`).

## Side Effects
- Generates one world map in a process-wide `OnceLock` (`terrain.rs:terrain`), plus a second one in `world_map_samples` and repeated maps in `world_generation`. Memory and CPU heavy.
- Criterion writes reports under `target/criterion`. No source or asset is modified.

## Verification
- Run: `cargo bench --bench terrain`
