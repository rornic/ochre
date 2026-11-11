# lod

## Overview
Octree level-of-detail selection and lifecycle for terrain beyond full-detail chunks. A node is a `lod/key.rs:LodKey` (`level`, `coord`): `CHUNK_SIZE` cells across at every level, so a level-n cell is `1 << n` blocks (`LodKey::cell_size`). Level 0 is a chunk and level 1 is the swap point between chunk groups and LODs (`group.rs:ChunkGroup`). `lod/layout.rs:LodLayout` fixes how far each level reaches; `lod/select.rs` walks the octree to pick nodes; `lod/selection.rs`, `lod/spawn.rs` and `lod/apply.rs` run that pick as Bevy systems that spawn node meshes, fade them in and retire them once what replaces them is ready. Mesh generation itself lives in [`../generate/generator`](../generate/generator/README.md) (`generate_lod_mesh`); this directory only schedules it.

## Interface
- `LodKey` (`key.rs`): `containing`, `width`, `cell_size`, `min_chunk`, `min_block`, `parent`, `children` (8), `self_and_ancestors`, `contains_group`, `groups_in`, `chunks`, `distance` (chunks from an anchor chunk's centre to the node's nearest point).
- `LodLayout` (`layout.rs`): `new(full_detail_distance, render_distance, level_scale)`, `with_full_detail_distance`, `with_render_distance`, `reach(level)`, `render_distance`, `region_reach`, `splits(key, anchor)`, `full_detail(chunk, anchor)`, `last_level_start`, `full_detail_reach`; public fields `full_detail_distance`, `level_scale`, `max_level`.
- `select_lods`, `select_lods_and_stand_ins`, `LodBand { min_y, max_y }` (`select.rs`): the nodes to draw, plus the level-1 stand-ins that cover the full-detail area until its chunks are ready.
- `LodLoader` (`loader.rs`, a `Resource`): `new`, `layout`, `anchor_chunk`, `set_render_distance`, `set_render_distance_chunks`, `chunk_covered`, diagnostics (`node_kind`, `is_retiring`, `is_selected`, `is_stand_in`, `node_is_empty`). `LodNode` and `GenerateLodMesh` are its components.
- `LodSelection` (`selection.rs`), `ChunkState`, `ChunkStates`, `LodState` (`state.rs`): the selection result, a chunk's readiness as LODs see it (`Missing`, `Loading`, `Empty`, `Drawn`), and the retirement/visibility rules (`can_retire`, `node_visible`, `chunk_visible`).
- `SOLID_WATER_LEVEL` (`apply.rs`, = 2): from this level up water uses the solid (batched) material.
- Crate-internal systems re-exported by `mod.rs`: `update_lod_selection`, `spawn_lods`, `load_lods`, `retire_lods`, `refresh_lod_visibility`; `TerrainPlugin` (`../lib.rs`) chains them in that order.

## Invariants
- Level 0 reach is `full_detail_distance`; level n reaches `lod_base * level_scale^n` up to `scaled_levels`, and only doubles beyond that (`layout.rs:LodLayout::reach`). `level_scale` is clamped to at least 2 and level 1's ring never narrows below `LEVEL_ONE_MIN_RING` (4 chunks) (`LodLayout::new`, `with_full_detail_distance`).
- `max_level == 0` selects nothing (`select.rs:select_lods_and_stand_ins`). A node is drawn as itself unless `LodLayout::splits` (within the next level down's reach); level-1 nodes that split become stand-ins instead of children (`select.rs:select_within`).
- A node retires only after the node or chunks replacing it are ready (`state.rs:LodState::can_retire`, `covered`); an empty node (`nodes[key] == None`) counts as ready, so holes never open (`state.rs:node_ready`).
- A selection computed under an older layout is discarded by `LodLoader::generation` (`selection.rs:update_lod_selection`); after the first selection only keys in `LodSelection::changed` are re-examined (`selection.rs:apply_selection`).
- `known_empty` never samples a region: it only consults already-loaded height ranges, and its "buried"/"air"/"water" shortcut must stop counting player edits or caves as empty if LOD meshes ever show them (`spawn.rs:known_empty` comment).
- `populated` counts nodes at or below each key up through `POPULATED_LEVELS` (16) so area walks skip empty subtrees (`loader.rs:LodLoader::add_node`, `remove_node`, `nodes_under`).

## Side Effects
- Spawns async-compute tasks: the octree walk (`selection.rs:update_lod_selection`) and one `generate_lod_mesh` plus `group::pack_group` task per node, capped at `LOD_TASKS_PER_THREAD` (32) times the pool's threads (`spawn.rs:spawn_lods`).
- Spawns and despawns Bevy entities and meshes; applies at most `LOD_APPLY_TIME_BUDGET` (2 ms) of finished meshes per frame (`apply.rs:load_lods`); up to `LOD_EMPTY_NODES_PER_FRAME` (8192) empty nodes are added per frame without an entity (`spawn.rs`).
- Retired nodes fade out and despawn through `fade::fade_out_and_despawn`; visibility changes go through `fade::set_shown` (`apply.rs:retire_lods`, `refresh_lod_visibility`).
- Re-sorts the spawn queue every `LOD_RESORT_INTERVAL` (2 s) (`spawn.rs:spawn_lods`).
- Reads `World::region_store` and `ChunkLoader` chunk entities (`spawn.rs`, `state.rs:ChunkStates`).

## Verification
- Run: `cargo test --release -p ochre-terrain` (`lod/tests.rs` runs real generation and is slow).
