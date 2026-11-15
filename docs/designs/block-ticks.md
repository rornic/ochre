# Block ticks, lazy simulation, hearths and decay

*   **Author:** Rory Nickolls
*   **Status:** Draft. Nothing here is built.
*   **Date:** 2026-11-14

## Overview

The survival game needs a world that changes (plants grow, fires spread, bases decay) and remembers. The world is 64 km square and shared by many players, so the server cannot tick every block, and it cannot hold an edit forever.

The design rests on three rules:

1.  **Derive, don't tick.** Most state is a function of a stored timestamp and the time now. A bush stores `planted_at`; its stage is computed when read. Nothing runs between reads.
2.  **Schedule, don't poll.** What does change over time in an eventful way (fire, flowing water, a hearth running dry) schedules its next event on a server queue. Cost follows activity, never world size.
3.  **Catch up on load, path-independently.** A chunk nobody is near is not simulated. When it loads, it is fast-forwarded from its last state over the elapsed ticks, to the same result as if it had been simulated all along.

On top of those: a hearth (with the campfire as its first tier) is both the claim and the thing that keeps a base from decaying, and a chunk with no maintained claim eventually returns to its generated state, so stored edits stay bounded by the number of active players.

## Scope

In: world time, environment history, the three simulation categories, the event queue, catch-up, hearths and claims, decay, respawn at a hearth, the effect on LODs, tests.

Out, each its own design: networking and the command layer (the server split), the body model (temperature, hunger, thirst, injury), inventory and items, fire chemistry in detail (spread rules per material), combat and raiding.

## Current state

*   `World` (`ochre-terrain/src/world.rs`) holds generated chunks. `set_block_at` is the one mutation point and records changed blocks for `take_edits`, which meshing consumes. Chunks are 16 blocks a side (`CHUNK_SIZE`), stored as runs in `ChunkData`.
*   Edits are not saved. A restart returns the world to its generated state.
*   Input systems edit `World` directly (`breaking.rs`), with no command layer or authority.
*   The game clock is already a `u64` tick count, `TimeOfDay(u64)` at `TICKS_PER_SECOND = 20`, with `DAY_SECONDS = 21_600` (a 6-hour day) in `ochre-world/src/sky.rs`. Only the day fraction is `f32`, taken by modulo before converting, so precision does not run out. What is missing is authority: `move_sky` advances the clock per frame on the client, wound by debug keys, so it is local, not the server's. The plan doc's "f32 days, 600 s per day" is out of date.
*   Weather is a pure function of seed, place and game tick (`ochre-world/src/storms.rs`, `docs/designs/weather.md`), and the clouds follow it. What catch-up needs from it, `integrate` below, is not built yet.
*   Water flow exists (`flow_at`) and is not designed to be replayed either.

## World time

The existing `TimeOfDay(u64)` ticks are the world time. The change is who owns them: the server advances the count at a fixed rate and clients only read it (and interpolate between ticks for rendering, as `move_sky` already does with its carry). All timestamps, schedules and catch-up spans are in these ticks. The debug wind keys become a local view offset, not a change to the world's time.

Whether the world ages while the server is down is an open decision. Either answer works with the design; it only changes what `now` means at startup.

## Environment history

Catch-up needs to know what the weather and season did during a gap. Weather becomes a pure function of `(seed, region, tick)`, with a regional grid coarse enough to be cheap and drawn by integer hash, in the style of `worldmap/character.rs`.

Two operations follow:

*   `weather_at(seed, region, tick)`: what a client or system sees now.
*   `integrate(seed, region, from, to)`: totals over a span (rain, dry days, cold hours, wind), cheap enough to run on every catch-up. Closed form or coarse steps, never per tick.

Seasons are a pure function of tick. This replaces the global unseeded storm state, and it is what lets fire read dryness and the body model read cold, from the same source.

## Three kinds of simulation

### 1. Derived state (the bulk of the world)

Plants and regrowth, crops, moss and weathering, footpath wear, spoilage, rust, and structure decay. Each such block (or a sparse side entry for it) stores the tick it began or last changed. Its current stage is `f(now - stamp, integrate(...))`, computed when it is read, meshed, or acted on.

Most blocks are inert and carry nothing. The tickable ones live in a sparse per-chunk side table keyed by local position, not in the dense `ChunkData` runs, so a forest does not fragment the run encoding.

### 2. Event-driven state

Fire, flowing water, collapses, a hearth running dry. Each active item schedules its next event on a server priority queue keyed by tick:

*   A burning block schedules its next spread attempt, drawn from the material's flammability, wind and dryness at that time.
*   A hearth schedules one event at the tick its fuel runs out.

Work is proportional to the number of active items. When an item goes idle (a fire burns out, water settles) it leaves the queue and goes back to being derived state.

### 3. Cross-chunk coupling

An event that reaches an unloaded chunk (a fire at a border, a flow into a neighbour) first catches that chunk up to now, then applies. Unloaded chunks are never ticked on someone else's behalf.

What a fire does in a chunk nobody has near it is a design question in itself. Freezing it lets a player escape one by leaving. Letting it burn out coarsely, by resolving the whole event ahead of time on load, is the more honest choice and is the default here. Flagged as an open decision.

## Catch-up

When a chunk loads, the server compares its `last_sim` to now and runs one pass over its tickable entries and any scheduled events that fell due.

**The invariant.** Catch-up depends only on the state at `last_sim`, the elapsed span, `integrate(...)` over that span, and a seed from `(chunk, last_sim, now)`. It does not depend on when the chunk loaded, how often, or who loaded it. If it did, players could farm it: hover at a chunk edge so decay never runs, or pick the moment a raid triggers it.

For stochastic systems, "same result" means the same distribution, checked by property tests (below), and the seeded draw makes it deterministic for a given span.

**Cost.** Catch-up output goes through `set_block_at` and shows up in `take_edits` like any player edit. A heavily decayed chunk can queue a burst of remeshing as someone approaches, so catch-up is budgeted per frame and spread across chunks, never done in one pass.

## Hearths and claims

One mechanic, in tiers.

| Tier | Claim | Fuel | Decay protection | Use |
| --- | --- | --- | --- | --- |
| Campfire | a few blocks | burns out in hours | none worth the name | first night, forward camps, warmth, cooking |
| Stone hearth | a base | prepaid, capped at about one to two weeks | full while stocked | warmth, cooking, smoking and drying food, boiling water, respawn |
| Later tiers | larger | higher | higher | stations such as kiln or forge, sharing the same fuel |

A hearth is a block entity holding `lit_at`, `fuel` and its tier. Its burn-out tick is known at lighting, so offline protection, the start of decay and the visibility of its smoke all derive from that one timestamp, and the queue holds a single event for it. Adding fuel recomputes the event.

*   The first-night shelter needs no upkeep. The campfire is how a new player learns the system without a penalty.
*   Upkeep scales with what is claimed. A hearth's cost rises with the volume and materials it protects.
*   A hearth is a fire. A campfire beside a thatched roof, or in a dry-summer forest, is a hazard under the fire system. Rain puts out an outdoor fire, so players roof it and give it a smoke hole.
*   Smoke is the cost of protection. A lit hearth is visible for miles, so full protection means being found.
*   Clans share a claim and its upkeep.

## Respawn

A player respawns near their hearth, not on it.

*   **Placement.** A random valid point within a radius of the hearth, a few hundred blocks to start (a server setting, default 300). Valid means walkable ground, above water, off steep slopes, and outside the claim radius of another player's lit hearth, so a respawn never lands inside someone's base.
*   **Why not on it.** A raider who camps or destroys a hearth cannot trap the player, and the walk back to the dropped gear is the cost of dying. A short walk at most, never a long one.
*   **Fallback.** If the hearth is gone (burnt out and decayed, or destroyed), the player uses a bedroll if they placed one, and otherwise a distant spawn chosen by the fair-start rule, far from established bases.
*   **Delay.** An optional short respawn delay for the fallback case only, to discourage suicide-hopping across the map.

## Decay

Decay is derived state. A structure that is no longer protected moves through stages over weeks, driven by its materials, the weather integrated over the gap, and time:

1.  **Weathering:** moss, rot and rust, cosmetic and slightly weakening.
2.  **Weakened:** blocks are easier to break, and wood starts to collapse.
3.  **Ruin:** the structure is partly gone. It can be looted or moved into.
4.  **Fade:** over months, a ruin sinks back into the terrain. Edits to the ground (tunnels, pits) backfill more slowly still.

Timescales are server settings, set long at first, because they can only safely be shortened. Stone outlasts wood by a large factor, or there is no reason to invest in it. An unprotected stash decays the same way and its contents return to the world's supply, which is what keeps scarce resources circulating.

**The invariant.** A chunk with no maintained claim eventually carries no stored edits. It is the generated chunk again, with nothing on disk and nothing to catch up. This bounds the edit store by the number of active players, and it makes the distant view (which draws the generated terrain) agree with the world again.

## LODs and landmarks

LODs are heightfields from the terrain generator and show no edits. A maintained base, a woodpile, a cleared forest, or a hearth's smoke will not appear from a distance without help. Maintained claims register a sparse landmark layer (position, kind, extent) that the LOD pass can draw: a smoke plume, a building mass, a clearing. This is much cheaper than meshing edits at every LOD level. Abandoned land needs none of it, because decay returns it to what the LODs already draw.

## Persistence

The server stores per chunk: its sparse side table of derived entries, its block edits, its `last_sim`, and the scheduled events that belong to it. A chunk with none of these is not stored. The save format gets a version so decay rules can change without invalidating old worlds.

## Tests

*   **Path independence.** Simulate a span tick by tick, and by one catch-up, and compare (exact for deterministic systems, distribution for stochastic ones). Repeat with the span split at random points.
*   **Determinism.** A seed, a chunk and a span give the same catch-up on every machine, like `generate/generator/determinism.rs` and the `libm` rule for the maths.
*   **Environment.** `integrate` over a span equals the sum of `weather_at` over its parts, within a stated tolerance.
*   **Queue cost.** A world with a million derived entries and a hundred active fires costs the same per tick as one with no derived entries.
*   **Bounded storage.** Build, abandon, advance time past decay, and assert the chunk's stored size returns to zero.
*   **Respawn.** Respawn points are valid ground, inside the radius, outside other claims, over many seeds.
*   **Remesh budget.** Loading a fully decayed chunk stays under the frame budget, with catch-up spread across frames.

## Open decisions

| Decision | Lean | Why it matters |
| --- | --- | --- |
| Does the world age while the server is down? | Yes, up to a cap | A long outage should not wipe bases, and should not freeze fires or seasons either. |
| Tick rate | To measure | The event queue and fire spread set the floor, and the cost sets the ceiling. |
| Day length | Keep `DAY_SECONDS = 21_600` (the plan doc's 600 s is stale), revisit after playtests | Seasons are set in real weeks, and the day length sets how fast the body model runs. |
| What a fire does in an unobserved chunk | Resolved coarsely on load | Freezing is an exploit, and full simulation is unaffordable. |
| Decay timescales | Long, as server settings | Cannot be tuned without playtest data. |
| Respawn radius | 300 blocks | Too small is camped, too large is a long walk. |
| Block state storage | Sparse side table per chunk | Keeps `ChunkData` runs unfragmented. |

## Phases

1.  Server-owned `TimeOfDay` ticks. Weather as a pure function of `(seed, region, tick)`, with clouds following it (built, see `docs/designs/weather.md`), and `integrate` over a span.
2.  The sparse side table and one derived system (plant growth) end to end, with the path-independence test.
3.  The event queue and one event-driven system (the hearth burn-out), then fire on top of it.
4.  Persistence of edits, side tables and events, with versioning.
5.  Claims, decay stages, and the bounded-storage invariant.
6.  Landmark layer for LODs.
7.  Respawn at a hearth.

Phases 1 to 3 do not need the server split. Phases 4 to 7 should land on the server crate, once it exists.
