# ochre-gameplay/src

## Overview
What moves through the world (`lib.rs`: module doc): the player and its collision, a debug overlay and a recorded flight. Every module is `pub` in `lib.rs` except `collision`, `contact` and `view`. Each is a Bevy `Plugin` or a plain function library over `ochre_terrain::world::World`. `ochre-base`, `ochre-terrain` and `ochre-world` are the only sibling dependencies (`../Cargo.toml`). `examples/sandbox.rs` wires `PlayerPlugin`, `FlythroughPlugin`, `DebugOverlayPlugin` and calls `spawn_player`.

## Interface
- `view.rs`: the camera rig `player.rs:spawn_player` hangs on the player (`spawn_camera`: HDR, atmosphere, TAA, fog, depth of field, vignette), `MurkMaterial` and `Murk`, `update_underwater_effects`, and the cold grade (`ColdGrade`, `lift_cold_grade`, `apply_cold_grade`). `PlayerPlugin` schedules them; `ViewAssets` is the `SystemParam` for the meshes and materials it needs.
- `player.rs`
  - `PlayerPlugin { look }`: chains `player_move`, `grab_cursor`, `player_look`, `ochre_terrain::horizon::update_horizon` and `update_underwater_effects` in `Update`, registers `MaterialPlugin::<MurkMaterial>` and the `Footstep` message, and adds `SurroundingsPlugin` if missing. Its Startup `spawn_player` spawns a `Player` at the world's spawn with a child HDR `Camera3d` and a `MurkMaterial` sphere child of the camera, and inserts the `Murk` resource. `FootstepPlugin` is separate.
  - `Player { pub movement: Movement, .. }`, `Movement::{Fly, Walk}` (default `Fly`), `EYE_HEIGHT` (1.9), `cursor_grabbed`, `grab_cursor`.
- `collision.rs`
  - `Aabb { min, max }`.
  - `move_and_collide(position, delta, aabb, solid) -> (Vec3, BVec3)`: sweeps y, then x, then z.
  - `move_and_step(position, delta, aabb, step, solid)`: the same, plus ledge climbing up to `step`.
  - Both are pure functions over a `Fn(I64Vec3) -> bool` solidity callback.
- `debug.rs`: `DebugOverlayPlugin`; F1 (`TOGGLE_KEY`) toggles a text readout. `LookedAt(pub Option<(RayHit, BlockType)>)` is the block a game says the player looks at, shown when set.
- `flythrough.rs`: `FlythroughPlugin { flight }`; active only when a `FlightConfig` is given (`OCHRE_FLIGHT` through `DevEnv`).
- `remote.rs`: `RemoteControlPlugin { port }`; active only with a port (`OCHRE_REMOTE` through `DevEnv`, off unless set). Adds Bevy's `RemotePlugin` and `RemoteHttpPlugin` on 127.0.0.1 with `ochre.teleport`, `ochre.time`, `ochre.state`, `ochre.screenshot` (see the `game-remote` skill). Reuses `flythrough.rs:facing` and `ground_height`.

## Invariants
- `collision.rs:EPSILON` (1e-3): overlaps thinner than this are ignored, so a resting box is not read as inside the block under it. Blocks the box already overlaps are ignored too (`move_and_collide` doc), so it can always move out, except on the horizontal axes, where an overlap of up to `STEP_REACH` (0.25) still stops it and pushes it back out.
- `collision.rs:move_and_step` takes the stepped result (`climb`) only if `landed.y` and it gets further along the motion than the flat move, which can be backwards if the box is in a ledge's face. It never steps when `step <= 0.0` or `delta.y > 0.0`. The step comes when a box thinner by `STEP_REACH` a side is blocked, so the box walks up to `STEP_REACH` into a ledge it can climb first, and a wall it cannot climb stops it at contact.
- `player.rs:BODY` is 0.6 x 2.0 x 0.6, and `EYE_HEIGHT` is below `BODY.max.y`. `NEAR` (0.05) is chosen so that at any pitch up to `MAX_PITCH` the near plane stays out of a two-block ceiling; `tests::test_the_near_plane_stays_out_of_the_ceiling_of_a_two_block_gap` enforces this.
- `player.rs:walk` treats ungenerated chunks as solid (`world.is_chunk_generated`) so the player waits for ground instead of falling through. Frames are clamped to `MAX_STEP` (0.1 s). Collision coordinates are offset by `BLOCK_CORNER` (0.5) because blocks are drawn centred on their coordinates.
- `view.rs:update_underwater_effects` snapshots the dry `DistanceFog`, `DepthOfField`, `ColorGrading` and `ClearColor` on entering water and restores that snapshot on exit; it does not hardcode dry values. Water deeper than `WATER_LEVEL + 1` at the eye gets the murk sphere; shallower fresh water does not (`fresh`). The cold grade is `lift_cold_grade` before it and `apply_cold_grade` after it (`ColdGrade` keeps the grade it saved), so the snapshot never holds the cold's shift and warm country is untouched.
- `debug.rs` guards `FrameTimeDiagnosticsPlugin` and `RenderDiagnosticsPlugin` the same way.
- `flythrough.rs:sample_keys` never overshoots a key (`tests::test_a_flight_passes_through_its_keys_and_never_overshoots_between`). `smooth_lift` never drops below the per-frame need (`tests::test_lift_clears_every_need_and_changes_gently`).

## Side Effects
- `player.rs`: `player_move` logs "Flying"/"Walking" via `info!` on the F toggle (`FLY_TOGGLE_KEY`) and zeroes velocity. `grab_cursor` mutates `CursorOptions` (`Locked` + hidden on click, released on Escape or focus loss). `player_look` drains `MouseMotion`. `view.rs:update_underwater_effects` mutates the camera's fog, depth of field and grading, the `ClearColor` resource, and the `MurkMaterial` alpha.
- `debug.rs`: inserts schedules `Mark(0..=5)` into `MainScheduleOrder` to time main-world stages, installs `count_draws` in the render sub-app's `RenderSystems::Prepare`, and adds the two diagnostics plugins if absent. Visible at start if `OCHRE_STATS` is set; in that case `update_overlay` also prints `STATS ...` to stdout.
- `flythrough.rs`: with a `FlightConfig` (`OCHRE_FLIGHT=<file>`) it flies the keys in a text file (`parse_keys`, `sample_keys`, `Flight`): one key a line, `seconds x z height yaw pitch day`, the height over the ground (`ground_height` and `smooth_lift`), yaw a compass bearing, and the `TimeOfDay` set to `day` each frame. It waits `OCHRE_FLIGHT_WARMUP` seconds (25) holding the first key, logs `flythrough: flying`, then flies in real time, or with `OCHRE_FLIGHT_FRAMES=<dir>` on the fixed 60 fps clock saving a frame each and running on `DRAIN_FRAMES` after the last so its screenshots are written. Without a `FlightConfig` the plugin adds nothing.

## Verification
- Run: `cargo test --release -p ochre-gameplay` (or `cargo test --release --workspace`). Tests are unit tests inside `player.rs`, `view.rs`, `collision.rs` and `flythrough.rs`.
