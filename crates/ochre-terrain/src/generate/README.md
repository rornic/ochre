# generate

## Overview
The sampling layer between a `TerrainSource` (the world map, owned by `ochre-world`) and the block/mesh generators. `region.rs` and `region_store.rs` cache the source as 256x256-block `Region`s behind an LRU; `columns.rs` derives per-column data for full-detail chunks (`ChunkColumns`) and coarse LOD nodes (`ColumnGrid`) and caches it; `trees.rs`, `rocks.rs` place single objects from position hashes; `light.rs` floods sky light over a box. Block and mesh construction lives in the child [`generator`](./generator/README.md). `mod.rs` only declares modules.

## Interface
- `region.rs`: `REGION_SIZE` (256), `RegionCoord` (`from_world`), `Region` accessors (`height_at`, `biome_at`, `water_at`, `fresh_water_at`, `fall_at`, `rocky_at`, `spring_at`, `trees_at`, `height_range`), `compute_region(noise, coord)`.
- `region_store.rs`: `RegionStore` (`new`, `with_resident_cap`, `terrain`, `column_grid`, `chunk_columns`, `column_height_range`, `loaded_height_range`, `sampler`, `get_height`), `resident_cap_for`, `HeightSampler` (per-task cache of `SAMPLER_REGIONS` = 4 regions: `height`, `biome`, `water_top`, `ground_top`, `fresh_water`, `fall`, `rocky`, `spring`, `trees`, `gradient`), `water_block_top`, `bed_top`.
- `columns.rs`: `COARSE_CELL` (4), `ColumnGrid`, `ChunkColumns`, `ColumnCache` (`chunk_columns`, `grid`, `height_range`).
- `trees.rs`: `Tree`, `TreeKind` (`Broadleaf`, `Pine`, `Umbrella` for savanna and jungle giants, `Slender` for the jungle canopy), `TreeGround` trait (implemented by `HeightSampler` here and `GridGround` in the generator), `trees_near`, `TreeLook` (the tree a block belongs to: tint, trunk base, height, width, drawn radius and lean; `in_trunk`), `Tree::trunk_radius`, `TreeTints` (`new`, `look_at`, `at`), `Tree::lean` (from the seed, leaning further on cold dry ground: `Tree::with_exposure`), `Tree::card_scale`, `MAX_LEAN` (0.5), `TREE_REACH` (14), `TREE_HEIGHT` (48), `THIN_TRUNK_CELL` (4), `grows_into`, `Tree::plant`, `Tree::crown_boxes`.
- `rocks.rs`: `Rock`, `RockKind` (`Erratic`, `Mound`), `rocks_near(.., scale)`, `Rock::plant`, `ROCK_REACH` (20), `ROCK_MAX_CELL` (8). The tundra has small rocks (`SMALL_CELL` 12), outcrop ledges (`CELL` 24) and giants, lone or an `arch` of two pillars and a capstone (`GIANT_CELL` 96). Biome shares are keyed by biome name in `rocks.rs:share` (tundra, taiga, savanna).
- `light.rs`: `SkyLight` (`new`, `index`, `level`), `MAX_LIGHT` (re-exported from `ochre-base`).

## Invariants
- A region is computed once per slot: the LRU lock is held only to find or create a slot, never while computing, and concurrent askers wait on the slot's `OnceLock` (`region_store.rs:RegionSlot`, `RegionLru::get_or_insert`).
- Resident regions never fall below `RESIDENT_REGION_CAP` (64) and scale with the render radius (`region_store.rs:resident_cap_for`).
- Coarse nodes (cell >= `COARSE_CELL`) sample noise at their cell spacing instead of reading regions (`columns.rs` doc on `COARSE_CELL`, `../lod/layout.rs:LodLayout::region_reach`).
- Trees and rocks are chosen per hash cell (`trees.rs:CELL` = 10, `rocks.rs:CELL` = 24, plus `SMALL_CELL` and `GIANT_CELL` there) and `trees_near`/`rocks_near` scan `min - REACH ..= max + REACH` (`rocks_near` skips the grids a LOD node's cell size can't show), so any chunk or node sees every object that reaches into it; `TREE_REACH`/`ROCK_REACH` must bound object extent.
- `trees.rs:hash` and the salts are pure integer hashing; object placement is thread- and hash-order independent.
- Water surfaces are rounded so a shallow bed stays under its water: `bed_top` keeps ground a block under water (`region_store.rs:bed_top`, `HeightSampler::ground_top`).
- `SkyLight::new` asserts `size.x * size.y * size.z == opacity.len()` (`light.rs`); out-of-box lookups return full light (`SkyLight::level`).
- Nothing in `generate` may call `f32::exp`/`powf`/`sin`/`cos` for world generation; use `libm` (`AGENTS.md`, `Cargo.toml` dependency `libm`).

## Side Effects
- `RegionStore::chunk_columns`, `column_grid`, `get_region` and `ColumnCache::grid`/`chunk_columns` block while another thread samples the same entry: call only from tasks, never systems (doc comments in `region_store.rs`, `columns.rs`).
- `ColumnCache` bounds memory with `GRIDS_KEPT` (4096), `CHUNK_COLUMNS_KEPT` (8192) and `RANGES_KEPT` (500 000, cleared whole when exceeded) (`columns.rs:ColumnCache::grid`).
- `column_height_range` and `loaded_height_range` never block and return `None` when unsampled (`region_store.rs`).
- Reads the `TerrainSource` through `Arc<dyn TerrainSource>` (`region_store.rs:RegionStore::new`); no disk or network access in this directory.

## Verification
- Run: `cargo test --release -p ochre-terrain`
