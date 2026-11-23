# src (ochre-engine library)

## Overview
Root crate `ochre-engine` (`Cargo.toml:[package]`). `lib.rs` re-exports the four workspace crates, so a game depends on one crate. `settings.rs` owns `Settings`, the parse of `assets/settings.toml`, `plugins.rs` the `OchrePlugins` group and `load_world`, and `dev.rs` `DevEnv`, the `OCHRE_*` environment as plain values. The only game is the `sandbox` example (see [../examples/README.md](../examples/README.md)). Benchmarks live in [../benches/README.md](../benches/README.md).

## Interface
- `lib.rs:pub use` re-exports `ochre_base`, `ochre_terrain`, `ochre_world`, `ochre_gameplay`; `lib.rs` has three local modules, `settings`, `plugins` and `dev`.
- `settings.rs:Settings` (`Default`, `Deserialize`, `Clone`, `Copy`) holds `renderer: RendererSettings` (required), `sky: SkySettings` (shadows, owned by `ochre-world`), `plugins: PluginSettings` (`frost` and `godrays`, which start those plugins disabled when false), `debug: DebugSettings` and `world: WorldConfig` (all `#[serde(default)]`).
- `settings.rs:Settings::read(file)` reads and parses a TOML file, returning `Result<Self, Box<dyn Error>>`.
- `plugins.rs:OchrePlugins::new(settings, world).with_dev(dev)` is a `PluginGroup`; `load_world(&settings, &WorldOptions)` builds the world for it.
- `dev.rs:DevEnv::from_env()` reads `OCHRE_SEED`, `OCHRE_WORLD_DIR`, `OCHRE_CHARACTER`, `OCHRE_SPAWN`, `OCHRE_TIME`, `OCHRE_TIMESCALE`, `OCHRE_CLEAR_SKY`, `OCHRE_STATS`, and `OCHRE_FLIGHT*` into `WorldOptions` and plugin fields. The plugins read no environment.
- `settings.rs` re-exports `DebugSettings` and `RendererSettings` from `ochre_terrain::settings`, `SkySettings` from `ochre_world`. `WorldConfig` is imported from `ochre_world::worldmap`.

## Invariants
- The `[renderer]` table is mandatory, since `Settings.renderer` has no `#[serde(default)]`; `[sky]`, `[plugins]`, `[debug]` and `[world]` may be absent. Keys missing inside `[renderer]` fall back to defaults (`settings.rs:test_missing_renderer_settings_use_defaults`).
- `material_period` must be a power of two; `renderer.material_period()` panics otherwise (`settings.rs:test_material_period_must_be_a_power_of_two`).
- The shipped `assets/settings.toml` must parse, with `full_detail_distance` 12, `lod_base_distance` 16, `lod_level_scale` 2, `mesh_slab_mb` 64 and the debug switches off (`settings.rs:test_settings_file_parses`).
- `assets/shaders/world.wesl` must import `FAR_VISIBILITY, Painterly, painterly_light, painterly_saturation` from `painterly`, and `painterly.wesl` must define `FAR_VISIBILITY` and `painterly_light` (`settings.rs:test_world_shader_imports_painterly_light`). Renaming either shader symbol breaks `cargo test`.
- The tests in `settings.rs` read `assets/...` by relative path, so run them from the repo root.

## Side Effects
- `Settings::read` reads one file with `std::fs::read_to_string`. Nothing else in `src/` touches disk, the network, or global state.

## Verification
- `tests/plugin_subsets.rs` runs the game headless with each plugin left out.
- Run: `cargo test --release -p ochre-engine` from the repo root (the whole workspace: `cargo test --release --workspace`).
