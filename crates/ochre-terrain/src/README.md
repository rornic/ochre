# src

## Overview
Crate root of `ochre-terrain`: chunk storage, the Bevy systems that stream, mesh, group, edit and draw terrain, and the materials and renderer plumbing it draws with. Generation and meshing algorithms are in [`generate`](./generate/README.md); octree LODs are in [`lod`](./lod/README.md). `lib.rs` declares the modules (all `pub` except `shadow_mesh`), provides `TerrainPlugin` (headless-safe systems) and `TerrainRenderPlugin` (GPU materials), the task-pool tuning and `insert_terrain_resources`.

Full-detail pipeline, per chunk: `chunk_loader.rs:gather_chunks` spawns a `generate_chunk` task (nearest, camera-facing first) -> `generate_chunks` stores the result in `world.rs:World` and marks it `DirtyChunk` -> `mark_chunks` waits for all 26 neighbours then adds `GenerateChunkMesh` (buried chunks are skipped via `World::is_chunk_buried`) -> `load_chunks` runs `generate_chunk_mesh` and parks the meshes in `ChunkLoader::held` -> `group.rs:build_groups` merges the meshes of a level-1 group (2x2x2 chunks) once none is unsettled -> `load_groups` draws it -> `lod` systems fade it in over the LOD node it replaces. Edits (`World::set_block_at`) feed `remesh.rs` and water `flow.rs`.

## Interface
- `lib.rs`: `TerrainPlugin`, `TerrainRenderPlugin { renderer }`, `task_pool_plugin()` (async-compute gets 60% of cores, io capped at 1), `insert_terrain_resources(commands, renderer, debug, world_bounds, inputs, assets)`.
- `chunk.rs`: `ChunkCoordinate` (`containing`, `centre`, `adjacent`, `neighbors_26`), `neighbor_26_index`, `ChunkData` (run-length encoded blocks; `from_cells`, `get_block_at`, `set_block_at`, `empty`, `opaque`, `iter_non_air`, `decode_into`, flow accessors, public `trees: TreeTints`), `CHUNK_SIZE` (16), `CELLS`.
- `world.rs`: `World` (`Resource`; `with_terrain`, `seed`, `spawn`, `layers`, `insert_chunk`, `get_chunk_data`, `clear_chunk`, `adjacent_chunk_data`, `is_chunk_generated`, `is_chunk_buried`, `get_block_at`, `set_block_at`, `break_block`, `flow_at`, `take_edits`; public `bounds`, `region_store`).
- `chunk_loader.rs`: `ChunkLoader` (`Resource`; `new`, `group_entity`, `chunk_entity`, `anchor`), components `Chunk`, `DirtyChunk`, `GenerateChunkData`, `GenerateChunkMesh`, query alias `Unsettled`.
- `group.rs`: `ChunkGroup`, `BuiltGroup`, `MeshedChunk`, `GroupedChunk`, `merge_chunk_meshes`.
- `remesh.rs`: crate-internal `remesh_edits`, `apply_remeshes`, `chunks_reading`, `groups_reading`, `mesh_group`.
- `flow.rs`: `FALLING` (8), `SPREAD` (7), `surface(level)`; crate-internal `step`, `flow_water`.
- `material.rs`: `ChunkMaterial`, `WaterMaterial`, `SprayMaterial`, `TerrainMaterials`, `ShaderInputs`, `CloudLayer`, `TerrainAssets`, `block_atlas`, `material_array`, `MATERIAL_SIZE` (512), `water_level`, `push_shader_defs`.
- `fade.rs`: `FadeMaterials`, `Fade`, `SolidWater`, `FULLY_VISIBLE`, `set_shown`, `fade_out_and_despawn`.
- `curvature.rs`: `Curve` (`is_flat_at`, `horizon_distance`, `edge_dip`), `WORLD_CURVE`, `Curvature`, `CurvaturePlugin`. `horizon.rs`: `MAX_RENDER_DISTANCE` (2048), `render_distance_at`, `update_horizon`.
- `settings.rs`: `RendererSettings` (`lod_layout`, `fog_distance`, `material_period`), `DebugSettings { still_leaves, solid_leaves }`, `CloudResolution`.
- `pack.rs`: `set_packed`, `pack`. `raycast.rs`: `raycast`, `RayHit`. `shadow_mesh.rs`: crate-private `split`.

## Invariants
- A group is wholly shown or hidden because level 1 is the full-detail/LOD swap point (`group.rs:ChunkGroup`). A group merges only when all its chunks are settled (`group.rs:build_groups`) and is despawned only when none of its chunks remain loaded (`chunk_loader.rs:unload_chunks`).
- Chunk data is generated `NEIGHBOR_DATA_PADDING` (2) chunks past the meshed distance so every meshed chunk has all 26 neighbours (`chunk_loader.rs`); a chunk is unloaded only when the LOD covering it is ready (`LodLoader::chunk_covered`).
- Blocks below the world are solid rock and above it air, standing in for never-generated chunks (`world.rs:World::chunk`). `World::bounds` is validated at construction (`with_terrain`, panics on invalid bounds).
- `ChunkData` runs are in `linear_index` order (x outer, z middle, y inner); `set_block_at` and `get_block_at` panic on a coordinate outside the chunk (`chunk.rs:linear_index`).
- `World::take_stirred` returns blocks sorted, so water flow is deterministic (`world.rs`); `flow::step` computes each block from the water as it was before the step. Still water (generated) never moves (`flow.rs` module doc).
- `remesh::chunks_reading` must match the light box `generator::LIGHT_MARGIN` and the 26-neighbour read of meshing (`remesh.rs`).
- Fades are staggered by `FADE_IN_SHARE` so a mesh and its replacement never both leave a pixel undrawn (`fade.rs:set_shown`).
- `pack` must run as late as possible: after it float attributes can't be read, so `shadow_mesh::split` runs first on the opaque mesh (`pack.rs:pack`, `group.rs:pack_group`, `shadow_mesh.rs:split` panics otherwise).
- `RendererSettings::material_period` must be a power of two up to 64 (asserted in `settings.rs`).
- `render_distance_at` never exceeds the horizon or `MAX_RENDER_DISTANCE`, nor goes under the set distance (`horizon.rs`).
- Per-frame work is time-budgeted: `GATHER_TIME_BUDGET` 3 ms, `MESH_APPLY_TIME_BUDGET` 4 ms (`chunk_loader.rs`); in-flight generation is capped by `max_in_flight`.

## Side Effects
- Spawns async-compute tasks for chunk generation, meshing, group merging and remeshing (`chunk_loader.rs`, `group.rs`, `remesh.rs`).
- Spawns/despawns chunk, group and mesh entities; mutates `World` (`insert_chunk`, `clear_chunk`, edits, flow) and `Assets<Mesh>`.
- `World::with_terrain` reads env var `OCHRE_SPAWN` (`x,z[,lift]`) to override spawn (`world.rs`).
- `TerrainRenderPlugin::build` sets the global `pack::PACKED` flag and inserts a `MeshAllocatorSettings` resource into the render sub-app (`lib.rs`, `settings.rs:mesh_allocator`).
- `material_array` and `block_atlas` build mipmapped images from PNGs under the given directory (`material.rs`); `ChunkMaterial`/`WaterMaterial` bind curvature (binding 8), clouds and fade uniforms to shaders under `assets/shaders/`.
- `update_horizon` mutates `LodLoader` render distance, camera `DistanceFog` and projection far plane (`horizon.rs`); it is scheduled by `ochre-gameplay` (`player.rs`), not `TerrainPlugin`. `CurvaturePlugin` runs in `PostUpdate` and loosens camera frusta each frame (`curvature.rs`).

## Verification
- Run: `cargo test --release -p ochre-terrain`
