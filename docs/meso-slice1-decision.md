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
