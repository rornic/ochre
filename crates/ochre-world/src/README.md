# src

## Overview
Source of `ochre-world` (`../Cargo.toml`): the world map and terrain source (`worldmap/`), and the sky above it: clock, sun, moon, atmosphere, clouds, weather, tonemapping and sound (`ambience/`, `music.rs`, `weather.rs`). `lib.rs` declares nine public modules and one function, `load_world`, which turns a seed into an `ochre_terrain::world::World`. `sky.rs:SkyPlugin` is the umbrella for everything overhead and installs `clouds.rs:CloudPlugin`, `weather.rs:WeatherPlugin` and the two sky domes from `sky_dome.rs`; `ambience::AmbiencePlugin` is separate.

## Interface
- `lib.rs:load_world(region_distance, WorldConfig) -> World`: reads `OCHRE_SEED` (else `rand::random`) and `OCHRE_WORLD_DIR` (default `worlds`), applies `worldmap::character::world_config`, calls `worldmap::io::load_or_generate`, and wraps the map as `World::with_terrain(seed, region_distance, Arc<WorldTerrain>)`.
- `music.rs`: `MusicPlugin` and `Mood`. One track at a time, picked by `Mood::of(&Surroundings, rain)` (storm, desert, cold, night, day, first that holds), started after a random silence (10 to 40 s first, then 60 to 600 s skewed short), played once from `assets/sounds/music/<mood>.ogg` with fades baked into the files. Fades out early when its mood stops fitting for 20 s or the camera goes underwater. Reads `Surroundings`, so it needs `AmbiencePlugin`.
- Subdirectories: [`ambience/`](./ambience/README.md) (wind, loops, owls, `Surroundings`); [`worldmap/`](./worldmap/README.md) (map pipeline and `WorldTerrain`).
- `sky.rs`: `SkyPlugin { shadow_distance, shadow_cascades, cloud_resolution, godrays }` and `SkyPlugin::new(&RendererSettings)`; `TimeOfDay(u64)` with `from_days` and `days`; `Sun`, `Moon`; `sun(shadow_distance, cascades)` and `moon()` bundles; `atmosphere_camera()`, `camera_exposure()`; `sun_direction(time)`, `daylight(&TimeOfDay)`, `sun_illuminance()`, `moon_illuminance()`; constants `DAY_SECONDS` (21 600), `TICKS_PER_SECOND` (20), `DAY_TICKS`, `START_TIME` (0.4), `FOG_MAX_OPACITY`.
- `clouds.rs`: `CloudPlugin { resolution }`, `CloudMaterial` (`set_shade`), re-exported `CloudLayer`, `CLOUD_BASE` (2400.0). A low-resolution ray-march of a seamlessly tiling cover pattern is drawn by `march_clouds` in the render graph and blended up by a dome (see `docs/clouds.md` at the repo root).
- `sky_dome.rs`: `SunGlareMaterial` / `SunGlarePlugin` (`shaders/sun_glare.wesl`), `NightSkyMaterial` / `NightSkyPlugin` (`shaders/night_sky.wesl`), `dome(meshes, material, camera)`, `SKY_DEPTH_BIAS`.
- `godrays.rs`: `GodrayPlugin`, `volume()` (the box of air a camera carries as a child), `shining()` (marks the sun volumetric), `REACH`. Bevy's volumetric fog, thickened by `thicken` with the sun's height and thinned by rain, which also sets the camera's march steps with the air's thickness (`steps`: 1 in clear air, 12 at noon, 32 at a low sun); needs the patched `bevy_pbr` in the root `Cargo.toml` so the sun sorts before the moon, and `SUN_LIGHT` and `MOON_LIGHT` in `sky.wesl` follow that order.
- `weather.rs`: `WeatherPlugin`, `Weather` resource (`pub rain`, `sheltering()`), `RainMaterial`, `RainbowMaterial`, `ThunderDecoder`; `pub(crate)` `Thunder` and `weather_seconds`.
- `tonemap.rs`: `TONEMAPPING` (`SomewhatBoringDisplayTransform`), `dry_color_grading()`, `cold_color_grading(&ColorGrading, cold)` (exactly the input at 0), `displayed(LinearRgba)`, `scene_colour(Color)`; `pub(crate)` `EXPOSURE`.

## Invariants
- The clock counts whole ticks (`sky.rs:TimeOfDay`, `TICKS_PER_SECOND`) so it stays exact however long the game runs; `days()` is `ticks % DAY_TICKS / DAY_TICKS`. One day is six game hours, three of daylight (`sky.rs:DAY_SECONDS`).
- The sun's path is the great circle through `SUN_DIRECTION` at noon and the horizon; the moon rides `PI + MOON_LAG` behind it, so it is opposite the sun and nearly full (`sky.rs:path_basis`, `moon_direction`; tests `test_path_basis_matches_the_shaders`, `test_moon_stays_opposite_the_sun_and_rises_as_it_sets`).
- Shadow-map orientation holds for `SHADOW_STEP_TICKS` (60 ticks) then steps, so texels do not crawl while the light itself moves smoothly (`sky.rs:shadow_days`, `aim_sun_shadows`, `aim_sun_light`; test `test_shadows_hold_for_a_step_then_jump_to_the_next`).
- Constants mirrored in shaders must match them: `CLOUD_BASE`/`CLOUD_TOP` with `BASE`/`TOP` in `cloud_march.wesl`, `REFLECTION_SIZE` with `REFLECTION_TEXELS`, `SUN_DISC_SIZE` with `DISC_SIZE` in `sun_glare.wesl`, `MOON_BOOST` with `lighting.wesl`, `path_basis` with `EAST` and `PEAK` in `stars.wesl`, `RAIN_BOX` with `BOX` in `rain.wesl` (doc comments in `clouds.rs`, `sky.rs`, `weather.rs`).
- Cloud drift `WIND` is a whole number of patterns per hour each way so it stays seamless when `globals.time` wraps hourly (`clouds.rs:WIND`; test `test_wind_carries_whole_patterns_an_hour`). The cloud pattern wraps without a seam via a 4D torus (`clouds.rs:torus`, test `test_pattern_wraps_without_a_seam`).
- `CloudMaterial::set_shade` is the only storm hook on clouds; `darken_sky` writes it only when the shade changes by over 0.002 (`weather.rs:darken_sky`).
- Weather runs on game time, so winding the clock winds weather on and holding the clock holds it; lightning uses real time so wound-on storms do not strobe (`weather.rs:run_weather`, `weather_seconds`; test `test_weather_keeps_the_games_time`). The first storm arrives `FIRST_STORM_IN` (600 s) after start and a rainbow follows a cleared storm (test `test_the_first_storm_comes_clears_and_leaves_a_rainbow`).
- Rain is hidden underwater and above `CLOUD_BASE` (`weather.rs:fall_rain`). The rainbow shows only when the sun is low enough that part of it clears the horizon (`weather.rs:show_rainbow`).
- The effect and sky-dome materials cast no shadows, skip the prepass and are drawn from both sides (`weather.rs:effect_material!`, `sky_dome.rs:sky_dome!`).
- `tonemap.rs:scene_colour` inverts `displayed` by fixed-point iteration (`INVERSE_STEPS` = 32); tests assert the round trip and that mid grey stays about mid grey. `EXPOSURE` is read by `water.wesl` from the view's colour grading.
- `match_sky_horizon` sinks the atmosphere's planet under the camera so the sky's horizon dips as far as the land's edge (`sky.rs:match_sky_horizon`, `WORLD_CURVE.edge_dip`).

## Side Effects
- Env: `OCHRE_SEED`, `OCHRE_WORLD_DIR`, `OCHRE_CHARACTER` (via `character::world_config`), all in `lib.rs:load_world`; `OCHRE_SEED` again picks the cloud pattern in `clouds.rs:spawn_clouds`; `OCHRE_TIME` sets the starting clock in days in `sky.rs:SkyPlugin::build`.
- `load_world` reads and may write a map file under the world dir, logs the seed and map time through `tracing::info!` (see `worldmap/README.md`).
- Input: `[` and `]` wind the clock back and forward at `SCRUB_SPEED` 0.1 days a second (`sky.rs:move_sky`).
- ECS: inserts `TimeOfDay`, `Weather`, `CloudLayer`, `CloudMarch`, `CloudTargetResolution`, `WeatherHandles`; spawns the atmosphere, sun, moon, rain curtain, rainbow, sky domes, bolts, flashes and thunder timers; mutates `DirectionalLight::illuminance` of sun and moon and the camera's `Exposure::ev100` (`weather.rs:darken_sky`).
- Render: adds a render-world system `march_clouds` between `Core3dSystems::Prepass` and `MainPass`, and allocates `Rgba16Float` target, history and reflection images (`clouds.rs:CloudPlugin::build`, `spawn_clouds`). Refits the target on window resize (`clouds.rs:fit_cloud_target`).
- Audio: loops `sounds/rain.ogg` from `Startup` (`weather.rs:RainLoop`; `mix_rain` sets its volume from `Weather.rain`, square-rooted and eased over 1.5 s, and pauses it when there is no rain) and spawns thunder sources; `ThunderDecoder` synthesizes samples on the audio thread.
- Queries `World` for the block at the camera and for ground height under strikes (`weather.rs:fall_rain`, `strike_lightning`).
- Randomness: `rand::random*` for storm timing and strike placement, so weather differs per run (`weather.rs:Weather::advance`).

## Verification
- Run: `cargo test --release -p ochre-world`
