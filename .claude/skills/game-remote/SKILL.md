---
name: game-remote
description: Drive a running Ochre game over the Bevy Remote Protocol, to teleport the camera, set the time of day, read the game's state and take screenshots of what it renders. Use to see in the game a spot found with worldview or worldprobe, or to check a rendering change at a coordinate, instead of asking the user to fly there.
---

# Game remote

`OCHRE_REMOTE` starts `RemoteControlPlugin` (`crates/ochre-gameplay/src/remote.rs`): a JSON-RPC server on `127.0.0.1`, port 15702 unless `OCHRE_REMOTE` is a port number. It opens a game window, so ask the user before launching one on their desktop, and stop it when done.

## Start it

```
cargo build --release --example sandbox
(OCHRE_REMOTE=1 OCHRE_SEED=<seed> nohup cargo run --release --example sandbox > /tmp/game.log 2>&1 &)
until curl -s -o /dev/null -X POST http://127.0.0.1:15702; do sleep 3; done
```

Stop it by pid (`pgrep -f '^target/release/examples/sandbox'`), and check `/tmp/game.log` for a panic before trusting what it shows. Seeds aren't changed live: restart with another `OCHRE_SEED`.

## Call it

```
brp() { curl -s -X POST http://127.0.0.1:15702 -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":${2:-null}}"; }
brp ochre.teleport '{"x":1200,"z":-340,"height":40,"yaw":90,"pitch":-20}'
brp ochre.time '{"day":0.3}'
brp ochre.screenshot                       # {"result":{"path":"target/remote/shot-<ms>.png"}}
until brp ochre.state | grep -q '"last_shot":"target/remote/shot-<ms>.png"'; do sleep 1; done
```

Then read the PNG with the Read tool.

- `ochre.teleport {x, z, y?, height?, yaw?, pitch?}`: the eyes at `y`, else `height` blocks (default 3) over the highest ground or water round the spot. Yaw is a compass bearing in degrees (0 north, -z; 90 east, +x), pitch is degrees up. Leaving either out keeps the current view. Puts the player in fly mode.
- `ochre.time {day}`: 0 midnight, 0.25 sunrise, 0.5 noon. The weather follows the game's clock and position, and can't be set; launch with `OCHRE_CLEAR_SKY=1` for a sky without storms.
- `ochre.state`: seed, eye position, yaw, pitch, ground height and biome under the eye, time of day, weather, fps, chunk and LOD node counts, `settled`, `shots_queued`, `last_shot`.
- `ochre.screenshot {path?}`: queued until loading settles (no chunk or LOD work for 30 frames and 2 s since the last teleport, or 90 s at most), then saved. Returns the path at once; `last_shot` shows it once written.

Bevy's own methods (`world.query`, `world.get_resources`, `rpc.discover`, ...) work too, for reflected components and resources.

## Pairing with worldview

Find the spot with the `worldview` skill (`/info` and `/export` on the same seed), then teleport there and screenshot. The two hold separate copies of the world; both read the map cache in `worlds/`.
