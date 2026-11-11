# examples

## Overview
Flat directory of Cargo examples (auto-discovered, none declared in `Cargo.toml`) of package `ochre-engine`. One is the game (`sandbox.rs`). The rest are tools: headless simulations of the loading systems, a stage timer, world-generation probes and a browser world viewer. `worldview.html` is the viewer's page, a non-source asset embedded by `include_bytes!` in `worldview.rs:handle`. It is not documented as code. Dev-dependencies come from `Cargo.toml:[dev-dependencies]` (`bevy`, `criterion`, `image`). Shell clients for the viewer live in [../scripts/README.md](../scripts/README.md). The criterion benchmark is in [../benches/README.md](../benches/README.md).

## Interface
Run all from the repo root, with `cargo run --release --example <name> -- <args>`. Assets, shaders and `assets/settings.toml` load by relative path.

Game
- `sandbox.rs:main`: the game. Builds `DefaultPlugins` (nearest-neighbour images, `task_pool_plugin`) plus `TerrainPlugin`, `TerrainRenderPlugin`, `SkyPlugin`, `PlayerPlugin`, `BreakingPlugin`, `FlythroughPlugin`, `DebugOverlayPlugin`, `AmbiencePlugin`, `CritterPlugin`, `BirdPlugin`. `load_world` runs in `main`, before the window exists. `sandbox.rs:setup_scene` (Startup) calls `spawn_player` and `insert_terrain_resources`, feeding it `assets/textures/blocks.png` and `assets/textures/materials` through `ShaderInputs`.
- Env vars: `OCHRE_RESOLUTION=WxH` (`window_size`), `OCHRE_FLYTHROUGH` (1920x1080, no vsync), `OCHRE_NOVSYNC`, `OCHRE_STATS` (`winit_settings`: continuous updates even unfocused), `OCHRE_LOOK=x,y,z` (`look_direction`, a direction, default `Vec3::Z`).

Headless simulations (`MinimalPlugins` + `TerrainPlugin`, no window or GPU, `ShaderInputs::default()`)
- `lod_sim.rs` `[speed=80] [secs=10] [up]`: loads, flies +x (or straight up with `up`, driving `render_distance_at`), reports every 2 s the surface chunks with nothing drawing them (`holes` by distance band), stale full-detail chunks and shown-but-unselected LOD nodes. Logs frames over 100 ms.
- `flight.rs` `--seed --out --secs --fly-height --render-distance`: scripted load, walk, sprint and high legs at 60 fps pacing. Prints `key: value` lines (frame_ms p50/p99/worst, chunks_generated_per_s, lod_meshes_per_s, queue depths, `mesh_memory`) so two runs diff. `--out` also writes the report to a file.
- `flow_sim.rs` `[banks=8] [secs=30]`: times frames with water still, then breaks banks beside water (`banks`, `World::break_block`), then times frames while it flows. Pick a place with `OCHRE_SEED` and `OCHRE_SPAWN`.
- `shadow_cull.rs` `[shadow distance] [cascades] [pitch]`: for 15 sun positions builds Bevy cascades and counts the meshes and triangles each cascade frustum admits against the camera view (see `docs/shadow-perf.md`). CPU-side submission counts only.
- `visibility.rs` `[pitch] [buffer width=480]`: software-rasterises every in-frustum terrain mesh from 8 headings and reports meshes and triangles actually seen versus in the frustum, and fragments per pixel. Set the spot with `OCHRE_SPAWN=x,z[,lift]`.

Stage timers and probes (no Bevy app)
- `bench.rs` `[seed=1] [lod]`: single-thread timings of map, terrain, regions, chunk generation, chunk meshing and LOD meshing (levels 1 to 8, best of 3), with vertex counts. Env `BENCH_MOUNTAIN` (find mountain 150+ blocks over sea) or `BENCH_AT=x,z` moves the site.
- `worldprobe.rs` `<seed> <cmd> <x> <z> ...`: `info`, `line`, `grid`, `png`, `route`, `edges`, `sites`, `falls`, `features`, `slice`, `top`, `passes`. `slice`, `top`, `passes` generate real chunks through `generate_chunk_with`. `--no-cliffs`, `--no-trees`, `--no-flora` (`ChunkPasses`) switch passes off.
- `world_map.rs` `--seed --center x,z --scale --pixels --out --size --cell --biomes`: writes `<out>`, `<out>-labels.png`, `<out>-water.png`, `<out>-biome.png`. Prints land share, biome shares, height percentiles, spawn, feature counts and `gameplay::validate` result.
- `water_audit.rs` `<seed> [--fine]`: samples rivers, lake shores and coasts. Prints `AUDIT seed=...` lines (walls, tall walls, steps, raised banks, uncontained cells, deep river cuts) for before/after diffs. See `docs/water.md`.
- `seams.rs` `<seed>`: counts one-block ground steps beside water and where they fall across a map cell.

Viewer
- `worldview.rs` `[seed=42] [port=8777]`: `TcpListener` on 127.0.0.1 with a thread per connection. Routes: `/` (the html), `/tile` (`size` clamped 1..1024), `/export` (`w`,`h` clamped 1..4096), `/info`, `/summary`, `/seed`, else 404. Query keys `seed x z scale layers` (`layers=name:alpha,...`, `scale` floored at 0.25). Layer names are the match arms of `layer_pixel`, plus `hillshade`, `features`, `rivers`, `motion` in `render`.

## Invariants
- Always `--release` and run from the repo root (`AGENTS.md`). Relative paths: `assets/settings.toml` (`Settings::read` in every Bevy example and `worldview`), `assets/textures/*` (`sandbox.rs:setup_scene`).
- `sandbox.rs` creates the world before `App` and the window. A busy app on a new window has wedged the desktop shell (`sandbox.rs:main` comment). Keep it.
- `OCHRE_NOVSYNC` and `OCHRE_FLYTHROUGH` select `PresentMode::AutoNoVsync`, which can starve the compositor. Keep runs short.
- Headless examples must insert `ShaderInputs::default()` and init the `Mesh`, `Image`, `ChunkMaterial`, `WaterMaterial`, `SprayMaterial` assets by hand. A new asset type read by `TerrainPlugin` needs `init_asset` in `flight`, `flow_sim`, `lod_sim`, `shadow_cull` and `visibility`.
- `shadow_cull.rs` and `visibility.rs` call `ochre_terrain::pack::set_packed(false)` before the app starts. Both read vertex colours and positions as `Float32x4` and `Float32x3` (`cutouts`, `pieces`), which packing would turn to half-floats.
- `shadow_cull.rs:FIRST_CASCADE` (20.0), `FOV_DEGREES` (90) and `ASPECT` (16/9) mirror `sky.rs` and `spawn_player`. They are not read from them, so a change there must be copied here. `visibility.rs` repeats the FOV, aspect and eye height.
- `bench.rs`, `seams.rs`, `water_audit.rs`, `worldprobe.rs`, `world_map.rs` and `worldview.rs` build the map with `worldmap::character::world_config(seed, ...)`. Reuse that call so a seed gives the same world as in the game.
- `bench.rs` is the refactor check: vertex counts must match exactly before and after (`AGENTS.md`).
- `worldprobe.rs:main` demands at least four args, so `falls`, `features` and `sites` still need dummy `<x> <z>`. Its usage line lists only some commands, and its `grid` output subtracts a hard-coded 400.
- `worldview.rs` is localhost only, one world in memory at a time (`Server.world`, replaced on a new seed), and the 4096-byte request buffer in `handle` caps URL length (many `layers` can truncate).
- `seams.rs` hard-codes the 32-block map cell (`/ 32.0`) and the `water_kind` values 2 and 3, and unwraps its seed argument. Change them if `map_cell` or `WaterCell` changes.
- `worldview.html` calls `/tile`, `/info`, `/export`, `/seed` with the same query keys `worldview.rs:handle` parses, and reads the `X-Path` response header set by `/export`.

## Side Effects
- `sandbox.rs` opens a window, reads and writes saved maps through `load_world` (`OCHRE_WORLD_DIR`, default `worlds/`), and plays audio. Do not launch it on a shared desktop uninvited.
- `flight.rs` sets `OCHRE_SEED` in the process env (`std::env::set_var`) and can write the file at `--out`.
- `flow_sim.rs` mutates the loaded world (`break_block`) in memory only.
- `lod_sim.rs`, `flow_sim.rs`, `flight.rs`, `shadow_cull.rs`, `visibility.rs` use `load_world`, which may read saved maps from `OCHRE_WORLD_DIR`, and spawn Bevy task pools sized by `task_pool_plugin`. They poll in real time (sleeps; `shadow_cull` and `visibility` give up after 300 s).
- `worldview.rs` binds a TCP port (panics with "port in use"), and `/export` writes `target/worldview/<seed>-<x>_<z>-<w>x<h>-s<scale>-<layers>.png` and prints its absolute path. `worldview.rs` spawns a thread per request and per-row render threads.
- `world_map.rs` writes four PNGs to `--out`'s path (default `world_map*.png` in the working directory). `worldprobe.rs` writes only for `png` and `top`.
- `bench.rs`, `seams.rs`, `water_audit.rs` generate full world maps in memory and print to stdout.

## Verification
- Compile everything: `cargo clippy --workspace --all-targets` (builds all examples and benches).
- Behaviour check for refactors: `cargo run --release --example bench` and compare the vertex counts.
- No test modules here. `cargo test --release --workspace` covers the crates and `src/settings.rs`.
