# ochre-base

## Overview
The bottom crate of the Ochre workspace (`Cargo.toml`: package `ochre-base`, `publish = false`, depends on `bevy`, `serde`, `libm`). It holds the shared vocabulary the higher crates build on: blocks and tiles, biomes, deterministic noise, relief-feature maths, the `TerrainSource` trait, `WorldBounds` and `WATER_LEVEL`. The crate root has no source files of its own; everything is under `src/`, and `src/lib.rs` declares the modules `biome`, `block`, `features`, `math`, `noise`, `relief`, `terrain_source`. Per the repo `AGENTS.md`, `ochre-terrain`, `ochre-world` and `ochre-gameplay` depend on it, and the root `ochre-engine` re-exports it.

Run its tests with `cargo test --release -p ochre-base` from the repo root.

## Subdirectories & Boundaries
- src: [./src/README.md](./src/README.md) - all modules of the crate: block/tile mapping, noise, relief features, terrain-source contract, world bounds.
- src/biome: [./src/biome/README.md](./src/biome/README.md) - the `Biomes` table with Mountain/Volcanic modifiers, the `Biome` trait and its per-biome block, tree, spring and relief rules, grass/leaf colour.
