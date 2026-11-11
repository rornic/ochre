# ochre-gameplay

## Overview
Player movement, collision, wildlife and the debug overlay for the Ochre voxel engine (`Cargo.toml: description`). The crate root holds only `Cargo.toml`; all code is in `src/`. It depends on `ochre-base`, `ochre-terrain`, `ochre-world`, `bevy` and `rand` (`Cargo.toml: [dependencies]`), and is the top of the crate stack (`ochre-engine` re-exports it, root `Cargo.toml`). It is `publish = false`.

## Subdirectories & Boundaries
- `src`: [./src/README.md](./src/README.md) - plugins `PlayerPlugin`, `BreakingPlugin`, `BirdPlugin`, `CritterPlugin`, `DebugOverlayPlugin`, `FlythroughPlugin`; the pure collision functions `move_and_collide` and `move_and_step`; `spawn_player`, `LookedAt`, `block_looked_at`, `spawn_winged`.

## Verification
- Run: `cargo test --release -p ochre-gameplay`. The dev-dependency `ochre-world` enables `test-support` so tests can build worlds on real maps (`Cargo.toml: [dev-dependencies]`). Always `--release`: generation in debug is unusably slow (repo `AGENTS.md`).
