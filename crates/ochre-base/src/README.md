# ochre-base src

## Overview
The lowest layer of the Ochre workspace: the types every other crate shares. It defines blocks and their texture mapping, the biome table, deterministic noise, relief-feature maths, the `TerrainSource` trait that decouples chunk generation from the world map, and the world's vertical bounds and sea level. Other crates depend on it; it depends on no workspace crate (`../Cargo.toml`: `bevy`, `serde`, `libm`).

## Interface
- `lib.rs`: `WORLD_HEIGHT` (2048), `WATER_LEVEL` (0), `MAX_LIGHT` (15), `WorldBounds { min_y, max_y }` (serde `Deserialize` with `#[serde(default)]`, default -1024..2048) with `validate()` and `layers()` (16-block layer range).
- `block.rs`:
  - `BlockType` (`#[repr(u8)]`, 34 variants from `Air` to `CompactedShale`) with `occlusion_weight`, `light_opacity`, `is_solid`, `is_opaque`, `is_targetable`, `is_breakable`, `is_compacted`, `compacted`, `name`, `is_flora`, `tile(Face)`, `lod_tile`.
  - `Face` (`Top`/`Side`/`Bottom`, `Face::of_normal_y`).
  - `Tile` (`#[repr(u8)]` atlas cells) with `material_layer`, `foliage_layer`, `atlas_cell`, `tinted`, and consts `LEAVES_LAYER` (22), `FIRST_FLORA_LAYER` (23), `FIRST_FLORA` (`Tile::TallGrass`).
  - `TILE_COUNT` (37), `MATERIAL_FILES` (31 painted-material names in array-layer order), `MATERIAL_LAYERS`.
- `math.rs`: `smoothstep` (f32), `smoothstep_f64`, `ease`.
- `noise.rs`: `hash`, `unit`, `perlin`, `perlin3`, `fbm`, `fbm_filtered`, `ridged`, `resolved`.
- `features.rs`: `ReliefKind` (10 variants: Crevasses, Seracs, Pingos, PatternedGround, TundraHills, Drumlins, Yardangs, Kopjes, Gullies, Hummocks), `ReliefCtx { gradient, downhill, ridge_angle, wind }`, `offset(kind, seed, x, z, cell, ctx)`, `MAX_OFFSET` (18.0).
- `relief.rs`: `cliff_reach`, `cliff_reach_boosted`, `MAX_CLIFF_REACH` (18.0), `cliff_density`, `meadow_patch`, `outcrop_patch`.
- `terrain_source.rs`: trait `TerrainSource: Send + Sync` (required `seed`, `biomes`, `sample_for_cell`, `climate`, `temperature`, `find_spawn`; defaulted `bounds`, `rock_column`, `cliff_boost`, `tree_bound`, `sample`, `get`, `height`, `biome`, `water`), trait `RockColumn` (`bed`, `at`), `TerrainSample`, `Water`, `WaterKind` (`None`/`Ocean`/`Lake`/`River`), `SpringPart` (`None`/`Rim`/`Pool`), and a blanket `impl TerrainSource for Arc<T>`.
- Subdirectory: [biome](./biome/README.md) - `Biomes` table, `Biome` trait, modifiers, grass/leaf colour.

## Invariants
- `WorldBounds::validate` requires `min_y <= -64`, `max_y >= 128`, `min_y < max_y`, both multiples of 16, and both within +-1,048,576 (`lib.rs:WorldBounds::validate`); `max_y` is exclusive.
- `BlockType` is `#[repr(u8)]` and `Tile` is `#[repr(u8)]`; `Tile` order is the atlas cell order and is read by shaders (`block.rs` comments on `Tile`, `FIRST_FLORA`; test `test_tile_count_matches_the_last_tile` pins `TILE_COUNT == Heath + 1` and `Heath == Shrub + 1`).
- Flora tiles are the atlas's last 8 (`TILE_COUNT - FIRST_FLORA == 8`); flora has `light_opacity` 0, `occlusion_weight` 0.0, is not solid and is not opaque (test `test_flora_hides_and_shades_nothing`).
- `MATERIAL_FILES` order must match `Tile::material_layer` and `Tile::foliage_layer`; each tile's CamelCase name snake-cased equals its file (test `test_material_layers_follow_the_file_list`), and every layer is either world-space or foliage (test `test_foliage_layers_follow_the_file_list`). `Tile::Stone` and `Tile::CompactedStone` share shale's layers 9 and 21.
- `is_opaque` means `light_opacity == MAX_LIGHT`, so Leaves (1), Water (2) and Ice (3) are non-opaque; Ice is still solid with occlusion weight 1.0 (`block.rs:BlockType::is_opaque`, test `test_ice_lets_light_through_but_is_solid`).
- Compacted rocks are targetable but not breakable; `compacted()` maps each of the eight rocks (`Rock`, `Limestone`, `Clay`, `Basalt`, `Marble`, `Sandstone`, `Granite`, `Shale`) to a distinct tile (test `test_every_rock_has_a_compacted_form_that_cannot_be_broken`).
- Noise is cross-platform deterministic: `noise.rs` uses only integer hashing and `+ - * /`, `floor`, plus `libm::sin` in `resolved`; `hash(1, 2, 3) == 1_640_221_175` and `perlin(7, 1.3, 2.7)` are pinned, so changing them changes every world (test `test_noise_is_fixed_for_a_seed`). `features.rs` uses `libm::sin`/`libm::cos` and `relief.rs` uses only `noise` and `smoothstep_f64`.
- `fbm_filtered` equals `fbm` bit-for-bit when `cell <= 1.0` (test `test_filtered_fbm_matches_fbm_at_full_detail_and_calms_in_wide_cells`); `resolved` returns 0 once `cell >= wavelength`.
- `features::offset` is pure in `(kind, seed, x, z, cell, ctx)`, is clamped to +-`MAX_OFFSET`, returns 0 for `cell >= 16` (detail fades over cell 4..16) and on ground whose gradient the kind does not suit (tests `features_stay_within_the_most_and_are_pure_in_the_seed`, `features_vanish_in_wide_cells_and_on_ground_they_do_not_suit`). The scattered kinds (`tundra_hills`, `seracs`, `pingos`, `patterned_ground`, `drumlins`, `kopjes`, `hummocks`) scan only the 3x3 hashed cells around the point, so a feature larger than its cell size would be cut off.
- `cliff_reach_boosted` clamps `boost` to 0..1, so it never exceeds `MAX_CLIFF_REACH`; `cliff_reach` is 0 at gradient <= 1.0 and `CLIFF_REACH` (9.0) from 2.8 (test `test_cliffs_only_on_steep_ground_and_never_past_their_reach`). `cliff_density` stays in -1..1.
- `terrain_source.rs:TerrainSource::sample` equals `sample_for_cell(x, z, 1.0)`; `water()` returns `Water::default()` unless `water_kind != WaterKind::None`, and its `depth` is `water_surface - height`. `WaterKind`, not `water_surface == 0`, identifies wet columns (comment on `TerrainSample::water_surface`).
- The `Arc<T>` impl forwards `bounds`, `seed`, `biomes`, `sample_for_cell`, `rock_column`, `climate`, `temperature`, `cliff_boost`, `find_spawn` and `tree_bound`; the remaining methods use the trait defaults, which derive from `sample_for_cell`. A new overridable method on the trait must be added to that impl or an `Arc` source silently uses the default.
- The default `rock_column` yields `BlockType::Rock` over `i64::MIN..i64::MAX` (`terrain_source.rs:StoneColumn`); default `tree_bound` is 1.0 (no cheap rejection).

## Side Effects
- None. No file, network, thread or global state; every function is pure and the crate only declares types and the `TerrainSource` contract that implementers satisfy elsewhere.

## Verification
- Run: `cargo test --release -p ochre-base`
