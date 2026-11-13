# generator

## Overview
Turns a `TerrainSource` (reached through `RegionStore`) into blocks and meshes. Two entry paths share one mesher: full-detail chunks are generated block by block (`chunk.rs:generate_chunk`) and meshed from a dense 3x3x3-chunk neighbourhood (`generate_chunk_mesh` over `source.rs:ChunkNeighbourhood`); LOD nodes are never stored as blocks, but sampled into `lod.rs:LodBlocks` and meshed directly (`lod.rs:generate_lod_mesh`). Both reach `mesh.rs:mesh_blocks` through the `source.rs:BlockSource` trait, which is the seam between data source and mesher. Greedy merging, bevels and ramps (`greedy.rs`), gap-closing seams (`seam.rs`), shading (`shading.rs`) and vertex buffers (`buffers.rs`) sit under `mesh_blocks`. Chamfer, ramp and seam rules: `docs/bevels.md`; water rules: `docs/water.md`.

## Interface
Re-exported through `mod.rs`:
- `generate_chunk(regions, coord) -> ChunkData`, `generate_chunk_with(.., ChunkPasses)` (`ChunkPasses { cliffs, trees, flora }`, `ChunkPasses::ALL`) (`chunk.rs`): the passes are switchable so tools can tell which one made a block.
- `generate_chunk_mesh(chunk, adjacent, coord, regions, solid_leaves, tree_shapes) -> ChunkMeshes` (`chunk.rs`). With `tree_shapes`, `mesh_blocks` wraps its source in `source.rs:TreeFaces`, which reads tree logs as air (so the greedy pass, seams and bevels neither draw them nor let them hide the ground round them), and `trees_mesh.rs:mesh_trees` draws each log as an eight-sided prism: a tree's trunk bowed along `TreeLook::lean`, any other log as prisms toward each log beside it (diagonals too, into a trunk's centre line). `chunk.rs` raises each log cell's sky light to the brightest cell beside it (`SkyLight::open`), so the ground, plants and leaves that touch a log are not lit by the log's own dark cell. Prisms carry unfolded uv. `TreeFaces` reads leaves as air too, and `mesh_trees` draws one double-sided painted card on each surface leaf (a `Leaves` voxel with an open face): 2.0 to 2.8 blocks across (pines 1.4 to 2.0, on 45% of their surface leaves, since their tiers are nearly all surface), an irregular six-cornered fan whose corners are at uneven distances so it has no square corners, tipped out from the crown and up (a pine's along its crown's cone), lit by a pseudo-normal pointing out of the crown, in the `opaque` buffer with the leaves' atlas cell so `shadow_mesh::split` sends it to the cutout and shadow passes. A leaf with no open side draws nothing but stays solid.
- `generate_lod_mesh(regions, key) -> Option<ChunkMeshes>` (`lod.rs`): `None` when nothing is drawn.
- `ChunkMeshes { opaque, cutout, water, spray, inner_leaves }` and `has_geometry` (`buffers.rs`); `INNER_LEAVES_REACH` (64 blocks) and `INNER_LEAVES_BAND` (16) (`buffers.rs`).
- `surface_block` (the top block as generation lays it, for map tools) and `FREEZE_TEMPERATURE` (`terrain.rs`).
- Crate-visible: `LIGHT_MARGIN` (8, `chunk.rs`), `MAX_SKIRT_CELLS` (40, `lod.rs`); `FACE_NORMAL_OFFSET` and `block_gradient`/`column_block` to the parent module.
- Internal modules: `terrain.rs` (block choice: snow line, ice depth, soil/sand depth, `terrain_block`, `cut_cliff`, `underwater`), `flora.rs` (`flora_at`: grass, ferns, flowers, shrubs, tundra heath), `canopy.rs` (coarse-node canopy boxes, `Canopy`), `buffers.rs` (`MeshBuffers`, `GrassTint` with the cold's ground `mosaic`, `cotton` grass and rock `weathered_at`, `MeshStyle`, `InnerLeaves`, `BEVEL` 0.2, `RAMP` 0.3, `CUBE`).

## Invariants
- Output is deterministic per seed and map; `determinism.rs` (test-only, `#[cfg(test)]` in `mod.rs`) hashes a fixed chunk set from a synthetic and a generated map and must match on every machine.
- Chunk blocks are produced in `ChunkData::from_cells` order (x outer, z, y inner), so terrain bands form long runs (`chunk.rs:generate_chunk_with` comment).
- Trees, rocks and flora are decided from exact noise at the object's position so every LOD level grows the same ones; only a coarse node's tree base is set to the ground it draws (`lod.rs` comment above `GridGround`, `trees.rs:TreeGround::base_height`).
- `ChunkNeighbourhood` treats missing neighbours as air (`source.rs`), so a chunk meshed before a neighbour exists would bake a border; callers (`../../chunk_loader.rs:mark_chunks`, `load_chunks`) check neighbours are generated first.
- The light box used for a chunk is `LIGHT_MARGIN` out sideways, one block below and up to the top of the chunk above; `remesh::chunks_reading` must keep to the same box (`chunk.rs:neighbourhood_light` comment).
- A seam between two tops is closed by the chunk holding the lower block, so each seam is meshed once (`seam.rs:mesh_seams` comment).
- Textured meshing stops at cell size `TEXTURE_MAX_CELL_SIZE` (4) (`lod.rs:generate_lod_mesh`, `buffers.rs`); LOD meshes are unbevelled (`bevel: false`) and hide inner leaves (`InnerLeaves::Hidden`).
- Water fed through `BlockSource` keeps fresh water's real surface (`water_block_level`) separate from its spread display surface (`water_level`) (`source.rs`).
- Mesh output must stay vertex-count identical across refactors (`bench` example, per `AGENTS.md`).

## Side Effects
- Pure computation: no disk, network or ECS access. Intended to run on async-compute tasks.
- `RegionStore` lookups may block while another thread samples a region or column (`../region_store.rs`); do not call generation from a system.
- `NO_BEVELS` (`greedy.rs`, thread-local, test builds) switches bevels off for merge tests via `chunk.rs:bevel_on`.

## Verification
- Run: `cargo test --release -p ochre-terrain` (`tests.rs` runs real generation and is slow). Vertex-count check: `cargo run --release --example bench`.
