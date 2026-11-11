# biome

## Overview
The biome table and the per-biome rules for ground blocks, tree density and size, springs, grass tint and relief features. `Biomes` holds every biome a world map can name, plus each one under the `Mountain` and `Volcanic` modifiers (`mod.rs:Biomes::with_modifiers`); a column stores only a one-byte `BiomeId` into it. It also owns the climate-to-colour functions for grass and leaves.

## Interface
- `mod.rs:Biome` trait (`Send + Sync`): `name` required; defaulted `frost` (1.0, how much of the cold's frost the biome keeps), `tree_density` (None), `block` (identity), `spring_block` (Gravel), `climate_shift` ((0,0)), `tree_scale` (`TreeScale::default`), `relief` (empty).
- `mod.rs:Climate` (`temperature`, `humidity`, each 0..1, default 0.5/0.5), `mod.rs:TreeScale` (`height`, `spread`, `squash`, `giants`; default 1/1/1/0), `mod.rs:BiomeId` (`BiomeId::BASE` is the base terrain).
- `mod.rs:Biomes`: `new(seed)`, `with(Vec<Box<dyn Biome>>)`, `id(name)`, `with_modifier(id, Modifier)`, `modifier(id)`, `name(id)`, `tree_density(id)`, `climate_shift(id)`, `tree_scale(id)`, `relief(id)`, `spring_block(id)`, `block(id, base, above_sea, gradient)`.
- `mod.rs:grass_colour`, `mod.rs:leaf_colour`, `mod.rs:grass_tint`: linear-RGB grass colour from a `Climate` by bilinear blend of four sRGB corners (`GRASS_HOT_DRY`, `GRASS_HOT_WET`, `GRASS_COLD_DRY`, `GRASS_COLD_WET`); `grass_tint` divides by `GRASS_TEXTURE_MEAN`, `leaf_colour` is `grass_tint * LEAF_TEXTURE_MEAN`.
- Re-exports: `Desert` (`desert.rs`), `Forest` (`forest.rs`), `Glacier` (`glacier.rs`), `Plain` (`plain.rs`), `Swamp` (`swamp.rs`), `Modifier` (`modifier.rs`).
- `modifier.rs:Modifier` (`None`, `Mountain`, `Volcanic`). `Grassland` and `Modified` are `pub(super)` only.
- `plain.rs:Plain` is a data-driven biome built with `new(name, trees)` and the builders `shifted`, `scaled`, `with_relief`, `with_thaw(line, scree)`, `with_giants`.
- `Biomes::new` registers, in order: `Glacier`, `Desert`, `Forest`, then `Plain` tundra (`TUNDRA_THAW_LINE` 350, relief Pingos/PatternedGround/TundraHills), taiga (`TAIGA_THAW_LINE` 250, Drumlins), steppe (Gullies), savanna (Kopjes), jungle (`with_giants(0.1)`), then `Swamp` (`mod.rs:Biomes::new`).
- Block rules: `Desert` turns Grass into Sand (`desert.rs:Desert::block`); `Glacier` turns Grass/Snow to Ice below `ICE_GRADIENT` 0.8, Grass to Snow below `SNOW_GRADIENT` 1.6, and steep Grass/Sand/Rock to Ice (`glacier.rs:Glacier::block`); `Swamp` turns Grass at or below `MUD_BELOW` 2 into Dirt (`swamp.rs:Swamp::block`); `Plain` thaws Snow below its line to Grass, or Gravel at gradient >= scree (`plain.rs:Plain::block`).
- Spring beds: Desert Sandstone, Glacier Rock, Swamp Clay, default Gravel; any modifier other than `None` forces Rock (`modifier.rs:Modified::spring_block`).
- Relief per biome: Glacier Crevasses+Seracs, Desert Yardangs, Swamp Hummocks, `Plain` as configured (`ReliefKind` is defined in [`../features.rs`](../README.md)).

## Invariants
- `BiomeId(0)` is the base terrain and has no entry; id `n` maps to `entries[n - 1]` (`mod.rs:Biomes::entry`). Lookups on the base fall back to `BASE_TREE_DENSITY` (0.02), `(0,0)` shift, default `TreeScale`, no relief and Gravel springs.
- `Biomes::with` asserts `biomes.len() < u8::MAX / 3` (`mod.rs:Biomes::with`), since `with_modifiers` triples the table into a `u8` id space.
- Only `Biomes::new` calls `with_modifiers`; `with_modifier` for any modifier other than `None` panics (`expect("every biome has each modifier")`) on a table built by `Biomes::with` alone.
- `Biomes::id(name)` finds only unmodified biomes; `"grassland"` and modifier names are not findable (`mod.rs:Biomes::id`, tested in `modifier.rs` tests).
- `Biomes::name` returns None for the base terrain whatever its modifier, so Mountain-on-base reports no name (`mod.rs:Biomes::name`).
- Modified blocks apply the inner biome's rule first, then the modifier: Grass at gradient >= the bare limit becomes Rock, Sand becomes Gravel (`modifier.rs:Modified::block`). Mountain's limit falls from 1.6 at 150 blocks above sea to 1.0 at 600 (`ROCK_GRADIENT`, `ROCK_ABOVE`); Volcanic's is flat 0.5 (`BARE_GRADIENT`).
- Mountain caps tree density at `MOUNTAIN_TREES` 0.04 (base 0.02 if the inner biome sets none); Volcanic fixes `VOLCANIC_TREES` 0.01; Volcanic drops all relief (`modifier.rs:Modified::tree_density`, `Modified::relief`).
- `Modified` forwards `name`, `climate_shift`, `tree_scale` to the inner biome.
- `Biomes::new` ignores its `seed` argument (`_seed`).
- Grass colour clamps temperature and humidity to 0..1 (`mod.rs:grass_colour`); `grass_tint * GRASS_TEXTURE_MEAN == grass_colour` (test `test_tint_draws_grass_colour_from_texture`).

## Side Effects
- None. Every function is pure; `Biomes` is built once and shared immutably through `Arc<dyn Biome>` entries (`mod.rs:Entry`).

## Verification
- Run: `cargo test --release -p ochre-base biome`
