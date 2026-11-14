# ambience

## Overview
What the world sounds like at the camera: a synthesized wind, six recorded loops mixed by what surrounds the camera, and the odd owl. `AmbiencePlugin` (`mod.rs:AmbiencePlugin`) senses the terrain around the camera four times a second into a `Surroundings` resource, and everything audible follows that resource. `wind.rs` holds the wind synth and the `Dial` cell that carries values to the audio thread; `weather.rs` reuses `Dial` and `SAMPLE_RATE` from here.

## Interface
- `AmbiencePlugin` (`mod.rs:AmbiencePlugin`): registers the `Wind` audio source, inits `WindDials` and `Surroundings`, runs `wind::play_wind` and `play_loops` at `Startup`, and chains `listen_from_camera`, `sense_surroundings`, then (`wind::update_wind`, `mix_loops`, `hoot`) in `Update`.
- `Surroundings` (`mod.rs:Surroundings`): `altitude`, `speed`, `underwater`, `water`, `trees`, `trees_overhead`, `warmth`, `desert`, `day`, `breeze`; the fractions are 0..1. `Default` is calm, dry, mild daylight.
- `Loop` (`mod.rs:Loop`): `Forest`, `Crickets`, `Waves`, `Desert`, `Underwater`, `Leaves`, each a component on one looping `AudioPlayer`. `Loop::level(&Surroundings)` is the public loudness rule, 0..1 before gain.
- `Wind`, `WindDials`, `wind_strength(altitude, speed)` (`wind.rs`, re-exported from `mod.rs`): wind loudness from height above ground and flight speed.
- `Dial` and `SAMPLE_RATE` (`wind.rs:Dial`, `wind.rs:SAMPLE_RATE`): an `f32` shared through an `AtomicU32` (`Dial` is private to the wind), and the 44.1 kHz rate, `pub(crate)`, which `../weather.rs`'s thunder reads.
- `smoothstep` (`mod.rs`): `pub(crate)` re-export of `ochre_base::math::smoothstep`.
- `wind::WindDecoder` (`wind.rs:WindDecoder`) is the `Decodable` iterator behind `Wind`: a low roar swelling with gusts plus a whistle that rises in pitch (doc on `wind.rs:Wind`).

## Invariants
- Loop volume is `level.sqrt() * gain * active * sheltering`, eased with a `LOOP_FOLLOW_SECONDS` (1.5 s) exponential follow (`mod.rs:mix_loops`). `gain` converts each file's measured LUFS to its target LUFS (`mod.rs:Loop::loudness`, `Loop::gain`).
- Underwater, every loop is silent except `Loop::Underwater` (`mod.rs:Loop::level`).
- `Forest` and `Crickets` are scaled by `1 - rain` from `weather::Weather`; with no `Weather` resource rain reads 0 (`mod.rs:mix_loops`, `wind.rs:update_wind`).
- A loop whose volume and target are both under `SILENT` (1e-3) is paused, and resumed when wanted (`mod.rs:mix_loops`).
- `Forest`, `Crickets` and `Desert` come and go on a `Gust` random walk (`mod.rs:Loop::coming_and_going`, `Bursts`); `Leaves` follows `breeze`, which the wind synth sets on the audio thread (`mod.rs:Loop::level`, `wind.rs:WindDials::breeze`).
- Wind target is `UNDERWATER_WIND` (0.02) underwater, else `wind_strength(..).max(rain * STORM_WIND)`; `wind_strength` never exceeds 1 and never drops below `GROUND_BREEZE` (`wind.rs:update_wind`, `wind_strength`; test `test_wind_grows_with_altitude_and_speed_and_never_goes_silent`).
- Terrain sampling is throttled to `SENSE_SECONDS` (0.25 s) and reads two rings of 8 columns (radii 24 and 64) plus the camera column (`mod.rs:sense_surroundings`, `RINGS`, `RING_COLUMNS`).
- Owls are spawned only when `Loop::Forest.level` with day inverted is at least 0.25, and wait longer in thin woods (`mod.rs:hoot`).
- `Dial` uses `Ordering::Relaxed` loads and stores: a value read on the audio thread may lag one control step (`wind.rs:Dial::get`, `Dial::set`).

## Side Effects
- Reads `World` terrain: `region_store.get_height`, `terrain().sample`, `temperature`, `biomes()` (name `"desert"`), and `get_block_at` for water (`mod.rs:sense_surroundings`).
- Reads `TimeOfDay` and `sun_direction` for `Surroundings::day`, and `Weather::rain` (`mod.rs`, `wind.rs`).
- Loads `sounds/forest.ogg`, `crickets.ogg`, `waves.ogg`, `desert.ogg`, `underwater.ogg`, `leaves.ogg` and `owl.ogg` through `AssetServer` (`mod.rs:Loop::file`, `play_loops`).
- Spawns six permanent looping `AudioPlayer`s starting silent, one wind `AudioPlayer`, and a despawning spatial `AudioPlayer` per hoot (`mod.rs:play_loops`, `hoot`; `wind.rs:play_wind`).
- Inserts `SpatialListener` on every `Camera3d` that lacks one (`mod.rs:listen_from_camera`).
- Draws from `rand::random*`, so hoots and bursts differ per run (`mod.rs:mix_loops`, `hoot`).

## Verification
- Run: `cargo test --release -p ochre-world ambience`
