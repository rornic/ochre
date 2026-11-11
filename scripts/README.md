# scripts

## Overview
Two shell clients for the `worldview` example's HTTP server (`examples/worldview.rs`, see [../examples/README.md](../examples/README.md)). Both need that server already running. Neither builds or starts it.

## Interface
- `capture-worldview.sh URL OUTPUT_DIR before|after [seeds...]`: for each seed (default `1 42 1337`) calls `URL/export` and `URL/summary`. Writes `OUTPUT_DIR/seed-<seed>-<name>.png` and `seed-<seed>-summary.txt`.
  - `before` renders `landscape`, `biomes`, `moisture`, `temperature`. `after` adds `plates`, `uplift`, `precipitation`, `surface-temperature`, `rivers`.
  - Every view is `x=-32768 z=-32768 w=1024 h=1024 scale=64`, so the same region is captured each time.
  - Exits 2 if the phase is neither `before` nor `after`.
- `census.sh [first] [last] [port]`: calls `http://127.0.0.1:<port>/summary?seed=N` for seeds `first..last` (defaults 1, 64, 8777) and prints CSV on stdout, with a header row built from the key names of the first response.

## Invariants
- Layer names in `capture-worldview.sh` must exist in `examples/worldview.rs:layer_pixel` (or be `hillshade`, `rivers`, `motion`, handled in `render`). An unknown name draws nothing.
- The default port 8777 in `census.sh` matches `examples/worldview.rs:main`.
- The capture origin `-32768` and 1024x1024 at scale 64 (65536 blocks) assume the default `[world] size = 65536` in `assets/settings.toml`.
- `census.sh` relies on `/summary` returning `key: value` lines (`examples/worldview.rs:handle`, `/summary`). It strips spaces and splits on the first colon.
- `capture-worldview.sh` runs under `set -euo pipefail` with `curl --fail`, so one failed request aborts the capture. `census.sh` exits 1 if `curl` fails.

## Side Effects
- HTTP GETs to the viewer. The server generates a world map per new seed and holds one in memory (`examples/worldview.rs:Server::world`).
- `/export` also writes PNGs to `target/worldview/` on the server side (`examples/worldview.rs:handle`).
- `capture-worldview.sh` creates `OUTPUT_DIR` with `mkdir -p` and writes files into it. `census.sh` writes only to stdout.

## Verification
- No tests. Start the server with `cargo run --release --example worldview -- 1 8777`, then run `scripts/census.sh 1 2 8777`.
