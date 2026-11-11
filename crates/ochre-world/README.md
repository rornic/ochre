# ochre-world Blueprint

## System Overview
`ochre-world` is the third crate in the Ochre dependency stack: it depends on `ochre-base` and `ochre-terrain` (and `bevy`, `noise`, `rand`, `serde`, `libm`, `tracing`) and nothing above it, per `Cargo.toml`. It owns two things. First, the world map: a per-seed geography grid (tectonics, rock, erosion, hydrology, climate, gameplay features) and `WorldTerrain`, the `TerrainSource` that `ochre-terrain` generates chunks from, so terrain never sees the map directly (`src/worldmap/source.rs:WorldTerrain`). Second, everything over the terrain: day/night clock, sun, moon, atmosphere, volumetric clouds, storms, tonemapping and ambient sound.

Typical use, per the root `AGENTS.md`: add `SkyPlugin` and `AmbiencePlugin`, then call `load_world` from a `Startup` system and hand the `World` to the terrain resources.

## Local Interface & Invariants
- `src/lib.rs:load_world(region_distance: u32, config: WorldConfig) -> World`: picks the seed (`OCHRE_SEED` else random), applies `worldmap::character::world_config`, loads or generates and caches the map under `OCHRE_WORLD_DIR` (default `worlds`), and builds `World::with_terrain(seed, region_distance, Arc::new(WorldTerrain::new(Arc::new(map))))`.
- `src/lib.rs` declares public modules `ambience`, `clouds`, `sky`, `sky_dome`, `tonemap`, `weather`, `worldmap`. Nothing is re-exported at the crate root.
- Feature `test-support` (`Cargo.toml`) exposes `worldmap::test_terrain`, `forest_spot`, `tree_spot`, `try_tree_spot` to other crates' tests; `ochre-terrain` and `ochre-gameplay` enable it in their `Cargo.toml`. Only `ochre-base` types may cross back to terrain.

## Subdirectories & System Boundaries
- src: [./src/README.md](./src/README.md) - crate modules, `load_world`, the sky stack (`SkyPlugin`, `CloudPlugin`, `WeatherPlugin`, sky domes, tonemapping).
- src/ambience: [./src/ambience/README.md](./src/ambience/README.md) - `AmbiencePlugin`, `Surroundings`, `Loop`, the wind synth and the `Dial` shared with weather.
- src/worldmap: [./src/worldmap/README.md](./src/worldmap/README.md) - `WorldMap`, the generation pipeline, `io` cache, `WorldTerrain`.

## Global Invariants
- A seed makes the same world on every machine and thread count; generation uses `libm`, never platform maths, and the saved-map hash is pinned (`src/worldmap/portable_maths.rs`, `src/worldmap/mod.rs:GENERATOR_VERSION`). Bump `GENERATOR_VERSION` whenever generation output changes.
- Sky constants shared with shaders in `assets/shaders/` (cloud heights, sun disc size, moon boost, sun path basis, rain box) must stay equal on both sides (comments in `src/clouds.rs`, `src/sky.rs`, `src/weather.rs`).
- Game time is the integer `TimeOfDay` tick count; weather follows it, lightning does not (`src/sky.rs:TimeOfDay`, `src/weather.rs:run_weather`).
- Docs for the water and landform rules referenced by code live in `docs/water.md`, `docs/landforms.md`, `docs/climate.md`, `docs/clouds.md` at the repo root; read `docs/water.md` before changing water.

## Global Side Effects
- Disk: map cache files `worlds/map-v{GENERATOR_VERSION}-seed{seed}-...rcmap`, written through a temp file and rename (`src/worldmap/io.rs`).
- Environment variables: `OCHRE_SEED`, `OCHRE_WORLD_DIR`, `OCHRE_CHARACTER`, `OCHRE_STAGE_TIMES`, `OCHRE_TIME`.
- Threads: map generation spawns scoped worker threads, all logical CPUs minus two (`src/worldmap/mod.rs:worker_threads`); audio synths run on Bevy's audio thread through `Dial`.
- GPU and audio: sky domes, a cloud march in the render graph and rain/thunder/wind/loop playback (`src/clouds.rs`, `src/sky_dome.rs`, `src/weather.rs`, `src/ambience/`).
- Keyboard: `[` and `]` wind the clock (`src/sky.rs:move_sky`).

## System Verification
- Run: `cargo test --release -p ochre-world` (or `cargo test --release --workspace`). Always `--release`: debug generation is unusably slow (root `AGENTS.md`).
