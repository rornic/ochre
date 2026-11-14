# ochre-gameplay/src

## Overview
What moves through the world (`lib.rs`: module doc): the player and its collision, block picking and breaking, birds and critters, a debug overlay and a scripted flythrough. Every module is `pub` in `lib.rs`. Each is a Bevy `Plugin` or a plain function library over `ochre_terrain::world::World`. `ochre-base`, `ochre-terrain` and `ochre-world` are the only sibling dependencies (`../Cargo.toml`). `examples/sandbox.rs` wires `PlayerPlugin`, `BreakingPlugin`, `FlythroughPlugin`, `DebugOverlayPlugin`, `BirdPlugin`, `CritterPlugin` and calls `spawn_player`.

## Interface
- `player.rs`
  - `PlayerPlugin`: chains `player_move`, `grab_cursor`, `player_look`, `ochre_terrain::horizon::update_horizon` and `update_underwater_effects` in `Update`, and registers `MaterialPlugin::<MurkMaterial>`.
  - `spawn_player(&mut Commands, &mut PlayerAssets, &RendererSettings, position, look)`: spawns a `Player` entity with a child HDR `Camera3d` and a `MurkMaterial` sphere child of the camera. It inserts the `Murk` resource.
  - `Player { pub movement: Movement, .. }`, `Movement::{Fly, Walk}` (default `Fly`), `EYE_HEIGHT` (1.9), `cursor_grabbed`, `grab_cursor`, `PlayerAssets` (`SystemParam`), `MurkMaterial` (shader `shaders/murk.wesl`), `Murk`, `DryDefaults`.
- `collision.rs`
  - `Aabb { min, max }`.
  - `move_and_collide(position, delta, aabb, solid) -> (Vec3, BVec3)`: sweeps y, then x, then z.
  - `move_and_step(position, delta, aabb, step, solid)`: the same, plus ledge climbing up to `step`.
  - Both are pure functions over a `Fn(I64Vec3) -> bool` solidity callback.
- `breaking.rs`
  - `BreakingPlugin`, `LookedAt(pub Option<(RayHit, BlockType)>)` resource, `REACH` (7.0).
  - `block_looked_at(&World, eye, forward)`: raycasts past air and water to the first `BlockType::is_targetable` block.
- `wings.rs`
  - `WingPlugin`, `Flap { beat, sweep, lift, glides }`.
  - `wing_mesh([[f32; 3]; 4]) -> Mesh`.
  - `spawn_winged(commands, creature, at, body, wing, flap)`.
- `birds.rs`: `BirdPlugin` (160 starlings via `STARLINGS`, plus periodic goose skeins). `flock_heading` and `steering` are `pub(crate)`.
- `critters.rs`: `CritterPlugin` (butterflies over `Poppy` and `Dandelion`, fireflies over meadow, both near the camera).
- `debug.rs`: `DebugOverlayPlugin`; F1 (`TOGGLE_KEY`) toggles a text readout.
- `flythrough.rs`: `FlythroughPlugin`; active only when `OCHRE_FLYTHROUGH` is set.

## Invariants
- `collision.rs:EPSILON` (1e-3): overlaps thinner than this are ignored, so a resting box is not read as inside the block under it. Blocks the box already overlaps are ignored too (`move_and_collide` doc), so it can always move out, except on the horizontal axes, where an overlap of up to `STEP_REACH` (0.25) still stops it and pushes it back out.
- `collision.rs:move_and_step` takes the stepped result (`climb`) only if `landed.y` and it gets further along the motion than the flat move, which can be backwards if the box is in a ledge's face. It never steps when `step <= 0.0` or `delta.y > 0.0`. The step comes when a box thinner by `STEP_REACH` a side is blocked, so the box walks up to `STEP_REACH` into a ledge it can climb first, and a wall it cannot climb stops it at contact.
- `player.rs:BODY` is 0.6 x 2.0 x 0.6, and `EYE_HEIGHT` is below `BODY.max.y`. `NEAR` (0.05) is chosen so that at any pitch up to `MAX_PITCH` the near plane stays out of a two-block ceiling; `tests::test_the_near_plane_stays_out_of_the_ceiling_of_a_two_block_gap` enforces this.
- `player.rs:walk` treats ungenerated chunks as solid (`world.is_chunk_generated`) so the player waits for ground instead of falling through. Frames are clamped to `MAX_STEP` (0.1 s). Collision coordinates are offset by `BLOCK_CORNER` (0.5) because blocks are drawn centred on their coordinates; `breaking.rs:show_outline` applies the same convention to the outline.
- `player.rs:update_underwater_effects` snapshots the dry `DistanceFog`, `DepthOfField`, `ColorGrading` and `ClearColor` on entering water and restores that snapshot on exit; it does not hardcode dry values. Water deeper than `WATER_LEVEL + 1` at the eye gets the murk sphere; shallower fresh water does not (`fresh`). The cold grade is `lift_cold_grade` before it and `apply_cold_grade` after it (`ColdGrade` keeps the grade it saved), so the snapshot never holds the cold's shift and warm country is untouched.
- `breaking.rs:BreakingPlugin`: `break_looked_at` runs only under `cursor_grabbed` and `.before(grab_cursor)`, so the click that grabs the cursor does not also break a block. Breaking returns `None` for unbreakable blocks (`tests::test_compacted_stone_is_looked_at_but_not_broken`).
- `wings.rs:WingPlugin` is added at most once: `BirdPlugin::build` and `CritterPlugin::build` guard with `is_plugin_added`. `debug.rs` guards `FrameTimeDiagnosticsPlugin` and `RenderDiagnosticsPlugin` the same way.
- `birds.rs`: birds are out only when `daylight(clock) >= BIRD_DAY` and `!Weather::sheltering()` (`birds_about`). `critters.rs`: butterflies leave below `BUTTERFLY_DAY`, fireflies above `FIREFLY_NIGHT`, and all leave in water or shelter. Counts are capped at `BUTTERFLIES` (14) and `FIREFLIES` (60), and critters beyond `FAR` (44) are despawned.
- `critters.rs:butterfly_at` keeps a butterfly within 2.6 blocks horizontally and 1.0 vertically of its home (`tests::test_butterflies_keep_near_their_flower`).
- `flythrough.rs:KEYS` is a fixed 22-key path on seed 2665537132 (module doc); `path` interpolates with monotone Hermite tangents so it never overshoots a key (`tests::test_path_passes_through_each_key_and_never_overshoots_between`). `smooth_lift` never drops below the per-frame need (`tests::test_lift_clears_every_need_and_changes_gently`).

## Side Effects
- `player.rs`: `player_move` logs "Flying"/"Walking" via `info!` on the F toggle (`FLY_TOGGLE_KEY`) and zeroes velocity. `grab_cursor` mutates `CursorOptions` (`Locked` + hidden on click, released on Escape or focus loss). `player_look` drains `MouseMotion`. `update_underwater_effects` mutates the camera's fog, depth of field and grading, the `ClearColor` resource, and the `MurkMaterial` alpha.
- `breaking.rs`: `break_looked_at` mutates the world through `World::break_block` and logs `broke <name> at <block>`. `spawn_outline` adds a `BlockOutline` entity at Startup, and `show_outline` hides it over a `Log`, since logs are drawn as prisms. `look_at_block` writes the `LookedAt` resource each frame.
- `birds.rs` and `critters.rs`: spawn and despawn entities each frame, draw randomness from `rand::random_range` (not seeded, so non-deterministic across runs), and read `World`, `TimeOfDay`, optional `Weather` and the `Camera3d`. Fireflies spawn a `PointLight` each (`shadow_maps_enabled: false`). `make_bird_assets` and `make_assets` run at Startup and insert `BirdAssets` and `CritterAssets`; `BirdPlugin` also inits `Murmuration`.
- `debug.rs`: inserts schedules `Mark(0..=5)` into `MainScheduleOrder` to time main-world stages, installs `count_draws` in the render sub-app's `RenderSystems::Prepare`, and adds the two diagnostics plugins if absent. Visible at start if `OCHRE_STATS` is set; in that case `update_overlay` also prints `STATS ...` to stdout.
- `flythrough.rs`: with `OCHRE_FLYTHROUGH=<dir>` it creates `<dir>`, writes `frame_NNNNN.png` for every frame after a 5 s warmup (`WARMUP`), switches the clock to `TimeUpdateStrategy::ManualDuration(1/60 s)`, overrides the player and camera `Transform` in `PostUpdate`, and sends `AppExit::Success` when the path ends. Without the env var the plugin adds nothing.

## Verification
- Run: `cargo test --release -p ochre-gameplay` (or `cargo test --release --workspace`). Tests are unit tests inside `player.rs`, `collision.rs`, `breaking.rs`, `birds.rs`, `critters.rs` and `flythrough.rs`; `breaking.rs` tests build a `World` via `ochre_world::worldmap::test_terrain` (`test-support` dev feature).
