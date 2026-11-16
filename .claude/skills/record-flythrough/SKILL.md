---
name: record-flythrough
description: Record Ochre as a 1920x1080 60 fps mp4 with the game's own sound and music, flying a path through the world with the time of day changing along it. Use when asked for a trailer, a flythrough, a showcase video or a screen recording of the game, or to show a place at sunrise or night.
---

# Record flythrough

`record.sh` turns a flight file into an mp4 in about twelve minutes. The picture is rendered frame by frame, so it is a perfect 60 fps whatever the machine is doing. The sound is the game's real mix, recorded from a private audio sink.

```
.claude/skills/record-flythrough/record.sh .claude/skills/record-flythrough/tundra.flight out.mp4 [seed]
python3 .claude/skills/record-flythrough/check.py out.mp4
```

`WARMUP` (default 45) is the seconds the world loads before each run, `CRF` (default 18) the video quality, `PRESET` (default medium) the x264 preset, `WORK` where the frames go, `KEEP_WORK=1` keeps them. `SOUND=<wav>` reuses the sound pass of an earlier run (`KEEP_WORK=1` keeps `sound.wav`) and skips about five minutes, but the wind and ambience follow the camera, so redo it when the path or the audio code changes. Leave about 20 GB free on `/tmp` for two minutes, since a frame is a 2.5 MB png. A game window is open on the desktop for most of it, and closing it ends the take. Nothing plays out loud. `check.py` needs numpy.

## The flight file

One key a line, `seconds x z height yaw pitch day`, `#` for comments. The camera follows the ground, so `height` is blocks over it, and a key's `x z` can be anywhere without looking up the ground. It also clears the trees: the terrain's own trees near each point of the path are looked up before the flight and the camera stays 6 blocks over the highest crown, so a low leg rides up over a forest in a long curve. Everything is eased through the keys, and the first and last are still.

- `yaw` is a compass bearing in degrees, 0 north (-z) and 90 east (+x). It can run past 360 so a turn keeps going the same way (498 is 138).
- `pitch` is degrees up.
- `day` is the time of day, 0 midnight, 0.25 sunrise, 0.5 noon. A game day is six real hours, so a flight fakes time passing by running `day` quickly, 0.14 in a minute is a night turning to dawn. The sun rises in the south-east (yaw 138), the stars turn with `day`, and the aurora needs the dark, so it fades out as day nears 0.25.
- The weather runs on game time, so a flight that winds `day` on quickly sweeps through whatever storms the seed has, rain and snow and all. `record.sh` sets `OCHRE_CLEAR_SKY=1`, which holds the sky clear (no storms, rain, snow, lightning or rainbow, only fair-weather cloud). Set it yourself if you run a flight by hand.
- The first key's `x z` is where the game spawns, so the world around it loads during the warmup. Keys far from it load as the camera arrives, which a path faster than about 60 blocks a second outruns.

To plan a route use the `worldview` skill and sample `info` heights. Seed 4's tundra is a plateau near y 100 with foothills up to 190 at (10500, -21000) and a glacier range of 700 to 1100 behind them to the north-west. `tundra.flight` flies it for two minutes: low under the aurora, a look round, up the foothills, the sunrise from the top, then down and low over the plateau in daylight. `tundra-day.flight` is a three minute day on the same seed: a short high look at the glacier in the morning, a low run east across the plateau and the strait while the day runs on, the highest point of the far ridge, then a slow sweep back over the tundra as the sun sets and the aurora comes up. A leg over about 90 blocks a second is too fast to read as flying, and the world has to stream in as the camera goes. The aurora only shows over cold ground, in the north (-z).

`steppe.flight` is the steppe's on seed 1, 248 seconds long to hold `steppe4.ogg` whole: the music starts about 5 s in, not the 3 that `record.sh` asks for, since its timer runs from the game's start and the flight's from when the player exists, so the flight runs a few seconds past the track's end (a first take ended at 240 s, with the track's last 3 s cut off): slow and low through the calm intro and outro, and fast, up to about 80 blocks a second, through the loud middle, high and low in turn, with the speed following the track's drive. `steppe_flight.py steppe4.wav steppe.flight` writes it, from the route, heights, look and time of day at the top of the script (in seconds of the track) and the track's loudness over time, and `steppe-path.csv` beside it for checking the route: the whole path should stay over steppe, or the music crossfades to another biome's track. Record it with `OCHRE_MUSIC_TRACK=steppe4 record.sh steppe.flight out.mp4 1`: `OCHRE_MUSIC_TRACK` plays the track whose file name has that in it, since the first pick is random otherwise. `steppe4.ogg` is not committed, so only that machine can record it.

## How it works

1. The flight runs in real time (`OCHRE_FLIGHT=<file>`) with `PULSE_SINK` pointing the game at a null sink, and ffmpeg records the sink's monitor from the moment the game logs `flythrough: flying`. `OCHRE_MUSIC_DELAY` starts the music three seconds into the flight, so it is in the take (it otherwise starts after a random 10 to 40 s of silence).
2. It runs again with `OCHRE_FLIGHT_FRAMES=<dir>`, which switches the clock to a fixed 60 fps and saves `frame_NNNNN.png` for every frame, however long each takes to draw.
3. ffmpeg joins them, with the sound brought to -16 LUFS (the game mixes it at about -37).

The code is in `crates/ochre-gameplay/src/flythrough.rs` (`Flight`).

## What went wrong, so it does not again

- **Do not screen-record the desktop.** `x11grab` of a game window gave 3% distinct frames in one run and 64% at best, because the compositor does not hand it each frame. Xephyr caps the frame rate and crashed the game with a segfault. Rendering frames in the game avoids both. `check.py` reports the share of distinct frames, and a clean render is 100%.
- **Do not close the game window.** The picture pass draws in a 1080p window on the desktop, and closing it logs `No windows are open` and ends the game with the take half done.
- **Run with `BEVY_ASSET_ROOT` set** to the repo root, or a binary run from anywhere else finds no assets and draws a black window. `record.sh` sets it.
- **Never `pkill -f` or `pgrep -f` a pattern the command itself contains**, since it matches your own shell and kills it (exit 144). Other sessions run their own games too, so kill only the pids you started. `record.sh` does.
- **The game must run on after the last frame.** It saves screenshots on other threads, and exiting at once left the last four of 721 unwritten. `DRAIN_FRAMES` holds it for two seconds.
- **The sound can silently be missing.** If `PULSE_SINK` does not take, the script moves the game's stream onto the sink with `pactl move-sink-input`, and warns if it finds none. `check.py` correlates the audio with `cold.ogg` to prove the music is in it.
- **Publish a recording only with music that may be published.** `cold.ogg` was made on a paid Suno plan and is committed. The other tracks are free-tier, non-commercial and local only, so a flight that plays them is for private use.
