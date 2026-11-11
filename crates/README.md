# crates

## Overview

The four workspace library crates behind `ochre-engine`. Dependencies point down only (`ochre-base` at the bottom, `ochre-gameplay` at the top), so a crate never imports from one listed after it. `ochre-terrain` never sees the world map. It generates from the `TerrainSource` trait in `ochre-base`, which `ochre-world::load_world` implements.

## Subdirectories & Boundaries

- ochre-base: [./ochre-base/README.md](./ochre-base/README.md) - blocks, biomes, noise and math, `TerrainSource`, `WorldBounds`, `WATER_LEVEL`. Depends on no workspace crate.
- ochre-terrain: [./ochre-terrain/README.md](./ochre-terrain/README.md) - chunks, generation, greedy meshing, octree LODs, materials and loaders. Depends on `ochre-base` only (`ochre-world` is a dev-dependency for tests).
- ochre-world: [./ochre-world/README.md](./ochre-world/README.md) - world map (erosion, hydrology, climate), sky, clouds, weather and ambience. Depends on `ochre-base` and `ochre-terrain`.
- ochre-gameplay: [./ochre-gameplay/README.md](./ochre-gameplay/README.md) - player, collision, birds, critters, debug overlay and flythrough. Depends on `ochre-base`, `ochre-terrain` and `ochre-world`.
