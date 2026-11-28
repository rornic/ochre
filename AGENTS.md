# Ochre

A voxel engine on Bevy 0.20. Rust workspace, no binary: the game is the `sandbox` example.

## Run

```
cargo run --release --example sandbox      # the game; reads assets/settings.toml
cargo run --release --example worldview -- [seed] [port]   # browser world viewer
cargo run --release --example bench        # single-thread stage timings
cargo test --release --workspace
cargo clippy --workspace --all-targets
```

Always `--release`: debug generation is unusably slow. Run from the repo root, since assets and shaders load by relative path. Rust 1.97.1+ (Bevy 0.20), plus `libudev-dev` and `pkg-config` on Linux. Wayland is deliberately off (X11 only).

Env vars, read by `ochre_engine::DevEnv` for the sandbox and tools (the plugins read none): `OCHRE_SEED` (fix the world seed), `OCHRE_WORLD_DIR` (saved maps, default `worlds/`, gitignored), `OCHRE_SPAWN`, `OCHRE_FLIGHT` (with `OCHRE_FLIGHT_WARMUP` and `OCHRE_FLIGHT_FRAMES`, see `flythrough.rs`), `OCHRE_CLEAR_SKY` (no weather, so no storm), `OCHRE_STATS`, `OCHRE_TIME`, `OCHRE_TIMESCALE` (game seconds per real second, for watching weather as a timelapse), `OCHRE_LOOK` (a direction `x,y,z`, not a point to look at), `OCHRE_REMOTE` (serve the remote control on localhost, `1` for the default port or a port number; off unless set, since any web page can reach it; see the `game-remote` skill). The sandbox also reads `OCHRE_FLY` (start flying, to hover at the spawn), `OCHRE_SHOT=seconds[,path]` (save the window that far in, whatever covers it on the desktop), `OCHRE_RESOLUTION=WxH` and `OCHRE_NOVSYNC` (uncapped, so it can starve the compositor: keep runs short).

## Crates

Dependencies point down only. `ochre-engine` (root `src/`) re-exports all four and owns `Settings` (`assets/settings.toml`).

```
ochre-base      blocks, biomes, noise/math, TerrainSource trait, WorldBounds, WATER_LEVEL
ochre-terrain   chunks, generation, greedy meshing, octree LODs, materials, loaders (depends on base only)
ochre-world     world map (erosion, hydrology, climate), sky, clouds, weather, ambience (base, terrain)
ochre-gameplay  player, collision, debug overlay, flythrough (base, terrain, world)
```

`ochre-terrain` never sees the world map: it generates from a `TerrainSource`, and `ochre-world::load_world` supplies one. Terrain's tests borrow real maps through `ochre-world`'s `test-support` feature, so only `ochre-base` types may cross back.

## Assembling a game

`examples/sandbox.rs` is the reference: `DefaultPlugins` (with `task_pool_plugin()`), then `OchrePlugins::new(settings, ochre_engine::load_world(&settings))`. `OchrePlugins` is a Bevy `PluginGroup`, so `.disable::<CloudPlugin>()` and `.set(PlayerPlugin { .. })` work as on `DefaultPlugins`. Build the world before the window, since it takes seconds. The plugins:

- `TerrainPlugin` owns the `World`, the loaders and the materials, headless-safe. `TerrainRenderPlugin` (needs a GPU) adds the material plugins, textures and mesh buffers. `CurvaturePlugin` and `FrostPlugin` are separate.
- `SkyPlugin` (clock, sun, moon, atmosphere), `NightSkyPlugin`, `SunGlarePlugin`, `CloudPlugin`, `WeatherPlugin`, `GodrayPlugin` (equips the camera itself).
- `PlayerPlugin` (spawns the player and camera), `FootstepPlugin`, `AmbiencePlugin`, `DebugOverlayPlugin`, `FlythroughPlugin`.

Every plugin but the terrain's can be left out: one that reads another's resource makes a blank of its own (`init_resource`), and `tests/plugin_subsets.rs` runs each combination headless. The plugins assume exactly one `Camera3d` (the loaders and sky systems follow it), so a game without `PlayerPlugin` spawns its own. A headless run (`flight`, `lod_sim`) uses `MinimalPlugins`, `AssetPlugin`, `TerrainPlugin::new(world, renderer, debug)` and registers the `Mesh`, `Image` and material assets with `init_asset`.

## Where things are

- Terrain generation: `ochre-terrain/src/generate/` (columns, regions, trees, light), meshing in `generate/generator/`.
- LODs: `ochre-terrain/src/lod/`. Chunk streaming: `chunk_loader.rs`. Both run on `TerrainPlugin`.
- World map pipeline: `ochre-world/src/worldmap/` (tectonics, erosion, hydrology, water, climate, then `source.rs` as the `TerrainSource`).
- Known issues, with what is known and what is a guess: `docs/issues.md`. Delete an entry in the commit that fixes it.
- Lakes, rivers and the sea, and the rule that keeps them from leaking: `docs/water.md`. Read it before changing water.
- Sculpted ground, one surface for grass and rock at every distance, and how the player, plants and water meet it, and the rule that keeps chunk edges exact: `docs/sculpted-ground.md`. Read it before changing the ground's mesh.
- What the world is made of and why: `docs/world-plan.md` (scope, gates, open defects, glossary), `docs/landforms.md` (the landforms and relief features), `docs/climate.md` (climate, biomes, transitions, trees), `docs/terrain-glossary.md` (what each landform and feature is). A seed draws its own plates, relief, warmth, wind, humidity and landform counts (`worldmap/character.rs`); `OCHRE_CHARACTER=off` keeps the config as given.
- The feel of each kind of country (sun, grade, haze, storm depth, blended smoothly by climate, and the geography colour on the ground): `ochre-world/src/atmosphere.rs`, `docs/climate.md` (Geography colour and heat). Far country shows in the haze through a table in the curvature buffer (`ambience/haze.rs`).
- Weather: a pure function of seed, place and game tick in `ochre-world/src/storms.rs`, read by `weather.rs` at the camera and by the clouds through a grid round it: `docs/designs/weather.md`.
- Shaders: `assets/shaders/`. The lighting is `painterly.wesl` plus the haze, grade and AO that go with it: `docs/designs/painterly-lighting.md`. A game's own materials light themselves as the land is with `world_light.wesl`'s `painterly_world_light`, binding the clouds and curvature at the slots `cloud_bindings.wesl` and `curvature.wesl` name. Block textures: the painted materials in `assets/textures/materials/` (one 512px PNG per material, RGBA for leaves and flora, see `docs/materials.md`), built by the `block-textures` skill.
- Examples double as tools: `worldview` (the browser viewer, and a probe of a spot from the CLI or its `/probe` route, plus `edges`, `features` and `sites`), `water_audit`, `lod_sim` (headless loading), `flow_sim` (headless frame times while water flows), `shadow_cull` (what the sun's shadow cascades draw, see `docs/shadow-perf.md`), `flight` (scripted flight report), `weather_map` (rain per biome and maps of a seed's storms).

## Conventions

- Verify output is unchanged with `cargo run --release --example bench`: vertex counts must match exactly before and after a refactor.
- Terrain is deterministic per seed. Don't introduce thread-order or hash-order dependence in generation (`generate/generator/determinism.rs` guards it).
- World generation takes `exp`, `powf`, `sin`, `cos` and the like from the `libm` crate, never `f32::exp` and friends, so a seed gives the same world on x86-64 and ARM64 (`worldmap/portable_maths.rs` scans for it).
- Perf work follows the `profile` skill and is logged in `docs/terrain-perf.md`. A phase that regresses p99 frame time needs a recorded reason.
- Debug the world with the `worldview` skill, not ad hoc probe scripts. To see a spot in the running game, use the `game-remote` skill.
- Record the game with `OCHRE_FLIGHT_FRAMES`, which saves every frame on a fixed clock, not a screen capture, which drops most frames.
- Large test modules (`generator/tests.rs`, `lod/tests.rs`) run real generation, so expect slow `cargo test`.
- Commit messages and PR titles are lowercase conventional commits. PRs target `main`. See `CONTRIBUTING.md`.
- Always launch the sandbox with `OCHRE_REMOTE=1`, so the running game can be read and shot without a restart.
- When driving the GUI, screenshot passively. Don't grab focus or input on a shared desktop.
