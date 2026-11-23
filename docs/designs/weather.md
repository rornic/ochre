# Seeded regional weather

*   **Author:** Rory Nickolls
*   **Status:** Built. See "As built" and "Not built".
*   **Date:** 2026-11-15

## Overview

The weather was one global state machine in `weather.rs`, stepped each frame by `dt` with unseeded `rand::random_range`. It could not be replayed, shared between a server and its clients, or asked about a place other than the camera. Storms came everywhere at once, and every biome got the same rain.

It is now a pure function of the world's seed, a place and a game tick (`ochre-world/src/storms.rs`). Nothing is stored or stepped: a client drawing the sky, a server catching a chunk up (see `docs/designs/block-ticks.md`) and a test all get the same answer for the same question. The clouds, their shadows, rain shafts, rain, lightning and the rainbow all read it.

## How it works

**Storms on a drifting lattice.** Storms drift with the seed's prevailing wind, the same wind the world map carries its rain on (`TerrainSource::prevailing_wind`, from `ClimateConfig::wind_x/wind_z`), at 2.5 to 5 blocks a game second, drawn from the seed. They are laid on a lattice of 12 km cells that drifts with that wind, and 3-hour slots of game time. Each cell may hold one storm per slot. An integer hash of the seed, the cell and the slot decides whether it does, when in the slot it's born, where in the cell, how long it lives (4 to 10 game hours), how wide its rain core is (2 to 6 km, mostly small), how strong it is, whether it thunders, how elongated it is and which way it lies (squall lines lie across the wind). Because the lattice drifts with the storms, a point is reached only by storms from a few nearby cells and slots, so asking about one place costs a couple of hundred hashes.

**Wet spells: where the climate has its say.** Storms themselves are only shapes: a map of precipitation, cloud and thunder. The climate acts in one place, `air_mass`. A slow field of value noise lies over air masses 100 km across that drift with the storms, and changes every game day and a half. Where it clears a bar, the air is in a wet spell. Storms are only born in one, and their rain only lands where the spell is strong enough for the ground's climate.

The bar sets the climate's character:

*   It is high (0.76) almost everywhere, so most of the world is mostly dry, with short spells of a day or two and long dry runs between.
*   Where the ground is both wet and warm, it falls to 0.45, so jungle is wet most days (`SPELL_BAR`, `SPELL_WET`, `SPELL_WARM`). Moisture alone can't tell a jungle from a wet coastal forest; warmth can.
*   On dry ground it's raised out of the noise's reach, easing back over steppe, so deserts stay dry (`SPELL_DRY_GROUND`, `SPELL_DRY_BAR`).

The world is 64 km across, so a spell usually covers much of it.

**Spells of fury: the steppe's storms.** Over steppe (mid-warm and dry, `steppe` in `storms.rs`, not the desert nor the cold) the same air-mass noise has a second, far lower bar (`FURY_BAR`). Where it clears, the whole 100 km air mass is in a spell of fury: `finish` darkens the sky, thunders and clouds over in swells (`fury_swell`, lumps 30 km across and an hour and a half long, drifting with the storms) and leaves the rain to the wet spells, so the steppe's storms are huge, dark and dry, and its rain stays rare (about 0.2% of the time). The test `test_the_steppe_gets_a_few_huge_dry_storms_a_week_and_the_desert_and_cold_none` holds it to 1 to 3.5 a game week (about 2). Measured on seeds 1, 2 and 3 with `weather_map`, the steppe is stormy 14.6%, 6.2% and 11.4% of the time, against 5.8%, 1.0% and 2.9% before. Thunder comes in the swell's upper half (`SWELL_THUNDER`), so a spell thunders for much of its length, and the middle of each thundering lump is a core for lightning seen from afar (`Forecast::fury_cores`, folded into `thunderstorms_near`), so the steppe flashes in the distance too. `Weather::gloom` drives the wind's roar and birds falling silent, so a dry storm blows and hushes the birds as a wet one does; the storm music keys off `precipitation()`, and the steppe's dry storms have their own track, `steppe-fury.ogg` (`Mood::SteppeFury`, when `gloom()` is over 0.5 and the camera is on steppe). Dark cloud alone, over `SHELTER_GLOOM`, sends the birds and butterflies away (`Weather::sheltering`), before the rain or without any.

**Clear skies.** Cloud follows the same noise, past a lower bar (`CLOUDY_BELOW_BAR`): it starts to build 0.25 under the wet bar, is all there by 0.05 under it, and is gone below that, so cloud comes before the rain and lingers after it, and between spells the sky is completely clear. Dry ground raises the cloud bar too (`CLOUDY_DRY_BAR`), so steppe and desert are clear most of the time. Each place has a fair-weather cloud share of its own that applies in a cloudy spell, from 12% over desert to 60% over humid ground. Storm cloud and dark cloud are scaled by the same factor, so a storm thins out over ground too dry for cloudy spells. The shaders honour a zero: `sky_cover` shows no cloud at all at zero cover, and the cirrus fades out with the cloud share.

**Snow.** Where the ground is cold enough for grass to lie under snow (`ochre_base::cold::SNOWY_GRASS_TEMPERATURE`, easing to rain over a narrow `SNOW_FALL_BAND` above it, about 35 blocks of height, so the two rarely fall together), so the taiga, tundra and glaciers, precipitation falls as snow (`Sky::snow`, its share), and a storm there never thunders. `Weather` splits what falls at the camera into `rain` and `snow`.

Over 90 game days on five seeds:

| Biome | Raining | Days with rain | Wet run | Dry run |
| --- | --- | --- | --- | --- |
| Desert | 0 | 0, but for the odd shower on a coast | | |
| Steppe | under 1% | 1 to 7% | about 1.5 days | 17 to 54 days |
| Grassland | 2.5 to 4% | 14 to 25% | about 1.5 to 2 days | 5 to 11 days |
| Forest and taiga | 3 to 5.5% | 18 to 33% | about 1.5 to 2 days | 4 to 8 days |
| Jungle | 10 to 13% | 57 to 65% | 3 to 3.5 days | 2 to 2.5 days |

A wet day in grassland brings a passing storm or two.

How much of the time the sky has no cloud at all (under 1% cover), and the share of days with none all day, over 90 game days on four seeds. The wetter a seed's climate, the less clear it is:

| Biome | Clear sky | Clear all day |
| --- | --- | --- |
| Desert and steppe | 80 to 90% | 60 to 80% |
| Grassland | 50 to 60% | 25 to 40% |
| Forest | 45 to 55% | 20 to 35% |
| Jungle | 7 to 19% | 0 to 4% | Seeds differ, as a seed's own warmth and humidity vary (`worldmap/character.rs`). `weather_map` prints this table for any seed.

**Shape over time.** A storm grows over the first quarter of its life, matures, and dies over the last 30%. Its rain core fades out over its outer half. Its cloud shield reaches 1.9 radii out, stretched 1.6 times farther ahead of it than behind, so the sky clouds over before the rain arrives. Its edge is ragged, by a quarter of its radius. Everything is continuous in place and time: a game second apart, nothing jumps.

**Determinism.** Every value comes from `+ - * / sqrt floor`, integer hashing and `ochre_base::noise::perlin`, which give the same answer on every machine. Climate comes from `TerrainSource::climate` on a 1 km lattice, blended, and cached as it's asked for.

## What reads it

*   **`Weather` (`weather.rs`)**, at the camera each frame: `rain`, `snow`, `cloud`, `storm`, `thunder`, `rainbow` and `wind`. `Sky::precipitation` splits into `rain` and `snow` by `Sky::snow`, the share that falls as snow where the ground is cold enough for snowy grass (`ochre_base::cold`). `gloom()` darkens the sun, moon and clouds as a storm's cloud comes over, before what falls does. The rainbow shows once heavy rain (not snow) in the last 20 game minutes has cleared. The rain's streaks slant with the storm's wind.
*   **Snow (`weather.rs:fall_rain`, `snow.wesl`).** Where it snows, flakes fall round the camera instead of streaks: 18,000 in a 40 by 24 by 40 block box (`SNOW_BOX`, `FLAKES`), as many shown as `snow` is heavy. Each is a soft round white dot facing the camera, 4 to 10 cm across (`RADIUS`) and grown with distance so far ones stay a pixel or two wide. They fall at 1 to 2 blocks a second (`FALL` 1.4), drift with the storm's wind at 0.8 blocks a second in light snow to 2.4 in the heaviest (`SNOW_DRIFT`), and sway in slow loops of about half a block either side (`SWAY`, `SWAY_RATE`). Like rain, they're hidden underwater and above the cloud base. Snow is quiet: the rain loop follows `rain` alone, and thunder is zero where it snows (`storms.rs:finish`), so neither near nor far lightning strikes in it. The ambience, wind and music read `precipitation()`, so birds and crickets hush and the wind rises in snow as in rain, and the storm track plays in heavy snow as in heavy rain.
*   **The weather grid (`clouds.rs:map_weather`)**: 64 by 64 cells of 1.1 km round the camera, on whole cells so they don't swim, reaching past where clouds fade out at 35 km. It is worked out again when the camera crosses a cell or 40 game ticks pass, and written to a storage buffer at binding 11 of the chunk, water and cloud materials and the cloud march: `CloudLayer::weather`, read through `weather_at` in `cloud_bindings.wesl`.
*   **Clouds (`cloud_march.wesl`).** The cloud pattern now holds each cell's rank, 0 for the clearest to 1 for the cloudiest, so any share of sky can be clouded over. `sky_cover` covers the cells ranked last, closes the gaps into an overcast as the share nears all of the sky, and fills in further under a storm. Under a storm, cloud is 2.5 times thicker, keeps less sun and much less sky light on its underside, and its towers reach 6 km instead of the fair-weather slab's 3 km. Rays only march the taller slab while a storm is on the grid. Haze also takes a storm's cloud three times more slowly, so storms stand out from far off.
*   **Cloud shadows (`cloud_shadow.wesl`)** use the same cover, and a storm's shadow is darker.
*   **Rain shafts (`cloud_dome.wesl:rain_shafts`).** At full resolution, 16 steps up to 30 km along each ray, cut off by the scene's depth, sample the grid's rain below the cloud base. Distant rain hangs as grey curtains under its storm, and the rain round you greys the distance, but no curtain hides more than 60% of the land behind it. Where the grid's cells say it snows, the curtain turns pale and white: it takes light faster (`SNOW_EXTINCTION`, 3.5 km against rain's 5), and is lit more by the sun and sky and darkened less by the storm's cloud (`SNOW_SUNLIT`, `SNOW_SKYLIT`, `SNOW_STORM_DARK`), blended by the snow's share at each step. They are skipped while nothing falls on the grid.
*   **Lightning.** In a thunderstorm's core, lightning strikes near the camera as before. Outside it, lightning strikes in the cores of thunderstorms up to 28 km away, drawn wider to stay visible and dropped with the world's curve, with no flash, and a rumble faded by distance out to 12 km.

## Tools

*   `cargo run --release --example weather_map -- --seed N [--days D] [--out DIR]` prints how often and how hard it rains per biome, and the cost of the weather grid and of one ask. It writes maps of the whole world's weather at a few moments.
*   `weather_map ... --storm X,Z` finds when a storm stands longest over a place by day and prints the command to start there in real time. Lightning near the camera comes faster the harder the storm: up to four times as often at full thunder as at its edge (`STRIKE_FRENZY`).
*   `OCHRE_CLEAR_SKY` holds the sky clear: no storms, rain, snow, lightning or rainbow, and the clouds keep their fair-weather cover (the grid is left empty).
*   `OCHRE_TIMESCALE=N` runs the game's clock N times faster (`sky.rs:TimeScale`). At 60, a game day takes six minutes, and a storm seen on the horizon arrives within a few.

## Cost

On the CPU, the grid costs about 2 ms each time it's worked out (every 2 game seconds at normal speed). The camera's weather costs about 15 µs a frame, plus 8 more asks once a game second for the rainbow (`weather_map` measures both).

The GPU cost was not measured: this was built without a GPU. Three costs need a check with the `profile` skill and `OCHRE_STATS`:

*   Each cloud march step reads the grid once (four storage loads).
*   Rays march a slab 3,600 blocks thick, not 600, while a storm is on the grid, with the same steps.
*   Rain shafts run 16 steps per pixel at full resolution whenever rain is on the grid.

## As built

*   `storms.rs`: `Forecast` (a `Resource`), with `new`, `from_terrain`, `from_world`, `sky`, `sky_among`, `storms_near`, `thunderstorms_near`, `map`, `climate_at` and `wind`. Also `Sky`, `Storm` and `seconds`.
*   `TerrainSource::prevailing_wind`, defaulting to +x. `WorldTerrain` gives the map's wind.
*   `weather.rs`: the state machine and `FIRST_STORM_IN` are gone. `start_forecast` builds the `Forecast` once `World` exists.
*   `clouds.rs`: the pattern is ranks, and there is a weather buffer (`WEATHER_*` constants, `weather_map`, `weather_corner`, `map_weather`).
*   Shaders: `cloud_bindings.wesl` (`WeatherMap`, `weather_at`, `sky_cover`, `most_rain`, `most_storm`), `cloud_march.wesl` (`ceiling`, `slab_top`, storm thickening, light and haze), `cloud_shadow.wesl`, `cloud_dome.wesl` (`rain_shafts`).
*   `curvature.rs`: `Curve::drop_at` is public, for distant bolts.
*   `sky.rs`: `TimeScale` and `OCHRE_TIMESCALE`.
*   Snow: `weather.rs` (`SnowMaterial`, `SnowCurtain`, `SNOW_BOX`, `FLAKES`, `SNOW_DRIFT`, `blown`, `scatter_mesh` shared with the rain), `snow.wesl`, and the pale curtains in `cloud_dome.wesl:rain_shafts`.

## Not built

*   **Seasons.** Storm chance and strength don't change through the year, because the game has no seasons yet. `air_mass` is where a season factor would go.
*   **Lying snow.** Snow falls but doesn't settle: the ground's snow and frost come from the climate alone (`docs/designs/snow-and-frost.md`), not from what has fallen.
*   **Fog, mist and wind strength** from the weather. Ambient wind already rises with `rain`.
*   **Cloud drift.** The base cloud pattern still drifts on its own fixed cosmetic wind in real time (`clouds.rs:WIND`), which needs a whole number of patterns an hour. The weather drifts on the seed's wind in game time. Under a timelapse, storms move as regions where the clouds thicken, not as individual clouds.
*   **Server authority.** The `Forecast` is built on each client from the seed. A server would build the same one, see the block-ticks design.
