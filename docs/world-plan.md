# World plan

What the tectonic world covers, what is not built, and the open defects. How each part works is in `docs/landforms.md`, `docs/climate.md` and `docs/water.md`; the numbers behind each change are in `docs/terrain-perf.md`; the log of how it was built is `docs/tectonic-mvp.md`.

## Scope

A seed makes a 64 km world with its own character: plates (8 to 16), relief, warmth, wind direction, humidity, a temperature gradient and continentality, and counts for each landform (`worldmap/character.rs`, drawn by integer hash, so a seed gives the same world on every machine). The world has landforms beyond mountains, a biome for each climate with its own trees and flora, blended edges between biomes, and relief features in cold and warm biomes.

Built, in the order they went in: portable maths; seed-derived config and humidity; temperature gradient, continentality and a warmth-driven treeline; tableland (terraced beds, stepped into the map before water is routed); dune scaling; cliffs reaching up to 18 blocks; gorge rivers over tableland; a rock per plate; fjords; volcanic cones; biome identity with mountain and volcanic as modifiers, and swamp; jungle giants; blended biome edges; groves and clearings and a thick jungle floor; cold and warm relief features; inselbergs, tower karst and cirques.

Out of scope, by decision: gneiss and andesite, scenic routes, elaborate (sealed) caves. Simple surface-reachable caves are a future idea. Undercuts were built and removed: leaning faces have no cap to leave.

## Gates

A change that alters generation bumps `GENERATOR_VERSION` and re-pins the map hash (`worldmap/portable_maths.rs`) and the chunk hashes (`generator/determinism.rs`). It is measured against the commit before it, never against an absolute figure:

- `cargo test --release --workspace` and `cargo clippy --workspace --all-targets` pass.
- `bench` (`BENCH_AT=x,z` and a seed for a view): chunk vertices at most 1.10 times the commit before on a view in the changed biome (1.15 for undergrowth), and the cost of generation recorded.
- `water_audit` on a few seeds: no uncontained cells and no lake or coast walls or steps.
- The census (`scripts/census.sh` against `worldview`): no biome over 70% of land, a spawn on every seed.
- A new stage adds at most 0.3 s to building the map.

## Open defects

| Defect | Evidence | Status |
| --- | --- | --- |
| Waterfalls: steep rivers run in treads and falls. Not built: a plunge pool, spray, falls over 40 blocks split, valley kept off a riser at a lip. Not seen in the game | `water_audit`, `worldprobe falls` | Built in the first form (version 86); a river over a terrace riser or cliff still ramps where the smooth profile does not step |
| LODs are heightfields: terraces' trees, steep fjord walls (seams up to about 3 blocks), the cliff pass's reach at the seam, show only at full detail. Boulders, mounds and kopje tops are in the finest LODs | the LOD seam checks | The rest deferred: needs a spike that measures the seam first |
| Rivers over mountain terraces keep a wall defect | `water_audit` river walls on mountain seeds | Deferred; tableland rivers run in gorges |
| Trees and flora density follow a cell's own biome, so a blended patch keeps its neighbour's trees | looking at a blended edge | Open |
| Hummock views, and gully views since they became wider swales, were not measured | `terrain-perf.md` | Open |
| Seen in the game so far: gullies and the cliff pass. Not yet: groves, relief on the other biomes, inselbergs, karst, cirques, blended edges after the two-cell cap | | Needs a look from the ground |
| The ARM64 checksum run under `qemu-aarch64` | never run; the pins were written natively | Open |

## Glossary

- **Landform:** three things. The map's `landform` layer (mountain or not); a shape laid into the map (fjord, cone, inselberg, karst, cirque), see `landforms.md`; and loosely, any large ground form.
- **Feature:** a relief feature (`ochre-base/src/features.rs`, a small offset a biome lays on the ground) and, separately, `FeatureKind` (a labelled place the gameplay pass finds: pass, ford, bridge site, spawn).
- **Province:** a rock province, the rock a plate or range is made of (sedimentary, volcanic, metamorphic, intrusive), not a political or biome region.
- **Class boundary:** where warmth and moisture change the climate biome the table gives, see `climate.md`.
- **Modifier:** mountain or volcanic ground on top of a climate biome.
