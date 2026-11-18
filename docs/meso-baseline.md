# Meso scorer baseline: what the first run shows

**Update after the first look in the game:** the top glacier peak on seed 3 turned out to be a 10-block spike on a glacier, which no landmark is. The scorer had no footprint rule and no absolute relief floor, because ranking is relative within a biome. It now smooths the ground over about 190 blocks (`FOOTPRINT`) and needs at least 40 blocks of landmark-window and 120 of mass-window relief (`Params::floor`). That cut the baseline from 626 to 350 places, and the numbers below are the first run's, before that fix, except where they say "after".

**After the fix:** the best landmark-window relief in any biome is about 122 blocks (glacier), 93 in taiga and forest, 90 in tundra, 78 in steppe and 75 in grassland. The best mass-window relief is 450 in forest, 427 in taiga, 323 in glacier, 317 in steppe, 261 in grassland and 168 in tundra. So the mass scale (a broad hill or basin, 250 or more blocks) occurs naturally in steppe and several biomes, and nothing in any biome reaches 130 blocks at the landmark window. Both figures are the design's unmeasured estimates of what reads from 4 km.

First pass of `worldmap/meso` (see `docs/designs/meso-scale-geography.md`) over 11 seeds, 10 places per biome at most: `docs/meso-baseline.csv`, made by `scripts/meso_baseline.sh` against a running `worldview`. Seeds: 1, 3, 4, 20, 42 and the tundra-rich 34, 35, 12, 64, 62, 30. Every term is provisional. Run `scripts/meso_spawn.sh <biome> [rank] [seed]` to get the command that starts the game at a place.

## What it shows

- **Natural relief falls well short of the readable size on steppe, tundra and grassland.** Of the places picked, none in steppe, tundra, grassland or swamp has both 130 blocks of relief at the landmark window and 250 at the mass window, the size the design says a 4 km landmark needs (an unmeasured estimate). The best steppe place has 116 and 223, the best tundra 148 and 67, the best grassland 124 and 241. Only glacier (22 places), taiga (6) and forest (2) have any.
- **So an amplifier is needed for steppe and tundra, as the review expected.** Emergence alone does not reach the size on the two first biomes, and the steppe's median top place has about 65 blocks of landmark relief.
- **The scorer is only half selective.** Run with the mass-scale relief shifted (the null run), 291 of the 570 places also appear, within about 1.5 km, and the mean score is 0.9898 against 0.9932. Shape and steepness coincide at the same bump whatever the mass is, so "two independent causes" is weaker than it sounds. Tighten the independence (for example require the mass term), or score the three terms against rock and water, before trusting the ranking.
- **Steepness is the weakest cause in 421 of 626 places** and mass in 167, so the tail rule is met mostly by shape plus mass.
- **Coverage of the two first biomes varies by seed.** Steppe has ten places on seeds 1, 3, 20, 42, 12 and 30, and two or three on 4, 35, 64 and 62. Tundra has ten on most seeds and two or three on seeds 1 and 20.
- **Kinds:** 219 raised soft, 200 raised steep, 127 dip steep, 80 dip soft.

## Not shown

The null run shares the grid and the rank rules, so it measures this scorer's discrimination and not the world's. Rock, water and the angular salience term are not in yet. The CSV is one scorer version, so the numbers move when the terms change.

## Places the owner judged

Seen in the game, 2026-11-18, and the scorer's verdict on each. This is the only calibration data so far, so a better scorer should rank the good ones high and the bad one low.

| Seed | Place (x, z) | Biome the scorer gave | Verdict | Why |
| --- | --- | --- | --- | --- |
| 1 | 13408, -4704 | steppe, best on the seed | good | the peak of a steppe mountain over a lake, with a steep cliff (but with holes in its mesh, see `docs/issues.md`) |
| 3 | -10144, 16672 | glacier, top steep peak | bad, before the footprint fix | a 10-block spike on a glacier with 360-degree views, which no landmark is |
| 3 | -3296, 12000 | steppe, lowland dip | very good | a waterfall coming down through a glacier into a mountain valley and on to the sea: terrain, water, ice and the coast lining up on their own. The scorer had no water term, so it found this by relief alone |
