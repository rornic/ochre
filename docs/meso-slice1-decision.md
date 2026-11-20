# Meso slice 1: what it found, and the decision for slice 2

Slice 1 of `docs/designs/meso-scale-geography.md` built a scorer of distinctive places and a spike, and changed no map. Its question: do the world's systems already make landmarks on steppe and tundra, or is an amplifier needed? Details are in `docs/meso-baseline.md` and `docs/meso-spike.md`.

## Findings

- **Emergence alone falls short on the first two biomes.** Over 11 seeds, the best landmark-window relief (about 190 blocks across) is 78 blocks on steppe and 90 on tundra, against the roughly 130 the design reckons reads from 4 km (an estimate, not measured). The mass scale (250 blocks or more of relief over 750 blocks) does occur: 317 on steppe, 168 on tundra.
- **The scorer tells a real conjunction from chance.** With the mass relief shifted, 12 of 319 places survive (it was 291 of 570 before the independence rule). It picks the three places the owner liked and dropped the bad glacier spike.
- **A player climbs any ledge up to 2.15 blocks and stands on one 0.2 blocks deep.** So a face the player cannot climb needs risers of 2.2 blocks or more with no 0.2-block ledge on them. Contact needs no change.
- **The mesher draws sheer faces and now leaves no hole in them.** The holes the owner saw had two causes, both fixed in the mesher: faces drawn by the wrong chunk (an empty one, never meshed) and folded rock triangles.
- **Around the top place per biome today, 31% to 84% of the drawn ground is walkable** (median 54%), on seeds 1, 4 and 42. The wall share counts climbable one-block risers too, so it is a rough baseline, not a climbability measure.

| Seed | Biome | Walkable | Steep | Wall |
| --- | --- | --- | --- | --- |
| 1 | Steppe | 52.6% | 16.3% | 31.1% |
| 1 | Tundra | 49.2% | 17.9% | 32.9% |
| 4 | Tundra | 56.2% | 16.4% | 27.4% |
| 42 | Steppe | 68.2% | 17.5% | 14.4% |
| 42 | Tundra | 71.3% | 20.1% | 8.6% |
| 1, 4, 42 | the other 11 | 30.9% to 83.8% | 11.9% to 30.8% | 4.2% to 46.2% |

## Decision

**Go to slice 2 on steppe and tundra, as an amplifier.** The data cancels the hope that emergence suffices there, which was the condition the design set for slice 2. The amplifier stays bounded: landmark sites are chosen by rules over the map's own layers, and rock, water and climate still shape what forms.

## Choices for slice 2, settled by the owner 2026-11-18

1. **The landmark stage runs before `carve_passes`,** with the other stamped shapes and their site helpers, so connectivity is checked round the landmarks.
2. **Dips are sited and scored but do not count** toward the landmarks visible from a ridge, which counts raised ones only.
3. **New primitives serve steppe and tundra alike** from one rule table. `cold_uplands` stays as the smaller relief between them.

## Slice 2, first part: built

- **Landmarks** on steppe and tundra from a rule table (`worldmap/meso/landmarks.rs`), every 70 to 110 cells, before `carve_passes`. See `docs/landforms.md`.
- **The flattening and the valleys leave them whole:** about 98% of the height laid survives on steppe, 95% on tundra.
- **Bowls hold lakes** however small (`hydrology::run_keeping`): 23 of 26 on seeds 3, 4 and 20.
- **The mass-scale relief** the scorer finds on steppe and tundra rises to 270 to 317 blocks on seven seeds.

## Slice 2, second part: built

- **Rock picks the form.** A butte needs a hard cap (sedimentary or volcanic), a ridge needs dipping beds (sedimentary or metamorphic), and a massif and a bowl form on any rock, so every site has a row.
- **Every biome is a candidate** through the climate's own classification, and a biome has landmarks if the table has rows for it. Enabled: steppe, tundra, taiga, forest, grassland, desert. Left out until their base terrain is worked out, by the owner's call: jungle, savanna, glacier, swamp.

## Deferred

- **Desert canyons.** A meso-scale linear cut that drains: its own primitive, not a row, with a river through its floor.
- **Micro formations** (hoodoos, standing rocks, boulder fields, small arches) in `rocks.rs`, which only knows glacial erratics and termite mounds. Block-level, so not a map-height stamp. It would help grassland, taiga and forest too.
- **Glacial forms on tundra and taiga** at their real proportions (eskers, kettles).
- **Shelter:** exposure over wider windows feeding temperature and humidity (design item 14).
- **A seam check** at the flattening's edge round each landmark (design item 12).
- **Landmark spacing.** Dart throwing alone packed steppe landmarks into a near lattice (nearest-neighbour spread 6%). Sites are now thrown closer by the square root of the mean chance, then each is kept with a chance of 0.35 to 1.0 that swells and thins over patches 2.5 spacings wide (`SPAWN` and `PATCH` in `meso/landmarks.rs`), so landmarks clump and leave open country. Judge the patch strength by eye.
- **Forest and taiga density.** They get 30 to 70 landmarks a seed, as the biomes are huge and spacing is by area. Judge by eye whether that is too busy.
