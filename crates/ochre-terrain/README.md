# ochre-terrain Blueprint

## System Overview
`ochre-terrain` (`Cargo.toml`: "Voxel chunk storage, generation, meshing and octree LODs for the Ochre voxel engine") owns everything between a `TerrainSource` and the GPU: chunk storage, block generation, greedy meshing, octree LODs, terrain materials and the Bevy systems that stream them around the camera. It depends only on `ochre-base` (plus `bevy`, `serde`, `libm`, `tracing`) and never sees the world map: `ochre-world::load_world` supplies the `TerrainSource`, wrapped in `generate/region_store.rs:RegionStore` and held by `world.rs:World`. Tests borrow real maps through the dev-dependency `ochre-world` with feature `test-support`; only `ochre-base` types may cross back.

The crate has no source files at its root; all code is under [`src`](./src/README.md). `src/lib.rs` exposes two plugins: `TerrainPlugin` (loading; runs headless) and `TerrainRenderPlugin` (materials, curvature, mesh allocator; needs a GPU), plus `insert_terrain_resources`, which a game calls from a Startup system.

## Subdirectories & System Boundaries
- src: [./src/README.md](./src/README.md) - streaming pipeline (`chunk_loader`, `group`, `remesh`, `flow`), `World`/`ChunkData` storage, materials, fades, curvature/horizon, settings, mesh packing.
  - generate: [./src/generate/README.md](./src/generate/README.md) - `RegionStore` region/column caches, trees, rocks, sky light.
    - generator: [./src/generate/generator/README.md](./src/generate/generator/README.md) - chunk block generation and shared mesher for chunks and LOD nodes.
  - lod: [./src/lod/README.md](./src/lod/README.md) - `LodKey`, `LodLayout`, node selection, spawn, retire and visibility systems.

## Global Invariants
- Dependencies point down only: this crate may use `ochre-base`, never `ochre-world` or `ochre-gameplay` outside dev-dependencies (`Cargo.toml`).
- Terrain is deterministic per seed: no thread-order or hash-order dependence in generation, guarded by `src/generate/generator/determinism.rs`; transcendental maths in generation goes through `libm`.
- Full-detail chunks and LOD nodes meet at level 1: a `ChunkGroup` is wholly shown or hidden, and a node retires only after its replacement is ready (`src/group.rs`, `src/lod/state.rs:LodState::can_retire`).
- Changing a mesh cut (chamfer, ramp, seam) or water must keep meshes closed and vertex counts as intended: read `docs/bevels.md` and `docs/water.md`; check with `cargo run --release --example bench`.
- Always run `--release`: debug generation is unusably slow (`AGENTS.md`).

## Global Side Effects
- Async-compute tasks for generation, meshing, grouping, remeshing and LOD selection (`src/lib.rs:task_pool_plugin`, `src/chunk_loader.rs`, `src/lod/spawn.rs`).
- Bevy entity, mesh and material asset churn; fade-driven material swaps (`src/fade.rs`).
- Reads env var `OCHRE_SPAWN` (`src/world.rs`) and block/material PNGs from caller-supplied paths (`src/material.rs`).
- Sets a process-global mesh-packing flag and render-world allocator settings (`src/pack.rs`, `src/lib.rs:TerrainRenderPlugin`).

## System Verification
- Run: `cargo test --release -p ochre-terrain` (whole workspace: `cargo test --release --workspace`). `src/generate/generator/tests.rs` and `src/lod/tests.rs` run real generation and are slow.
