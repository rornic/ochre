#!/usr/bin/env bash
# Records a flight (OCHRE_FLIGHT, see crates/ochre-gameplay/src/flythrough.rs) as a 1920x1080, 60 fps mp4 with the game's own sound.
# usage: record.sh <flight file> <out.mp4> [seed]
# env:   WARMUP=45 seconds the world loads before each run, CRF=18, PRESET=medium (x264), SOUND=<wav from an earlier run of the same flight, skips step 1>, WORK=<dir> for the frames (about 2.5 MB each, kept if KEEP_WORK=1)
#
# Three steps, because a screen capture of the desktop got a third of the frames it should and the game cannot render in step with
# the sound at the same time:
#   1. the flight in real time into a sound sink of its own, recording what the game plays (so nothing plays out loud)
#   2. the flight again, frame by frame on a fixed 60 fps clock, a frame saved for each, however slowly it draws
#   3. ffmpeg puts the two together, with the sound brought to a normal video loudness
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
flight=$(realpath "$1")
out=$(realpath -m "$2")
seed=${3:-4}
warmup=${WARMUP:-45}
crf=${CRF:-18}
preset=${PRESET:-medium}
work=${WORK:-$(mktemp -d)}
mkdir -p "$work" "$(dirname "$out")"

# the keys, comments stripped: seconds x z height yaw pitch day
keys=$(sed 's/#.*//' "$flight" | awk 'NF')
read -r _ first_x first_z _ _ _ first_day <<<"$(head -1 <<<"$keys")"
read -r last _ <<<"$(tail -1 <<<"$keys")"
frames=$(awk -v t="$last" 'BEGIN { print int(t * 60) + 1 }')

game="" ffmpeg="" sink=""
cleanup() {
  # only the processes this script started, by pid: other sessions run their own games
  for pid in $ffmpeg $game; do kill "$pid" 2>/dev/null || true; done
  [ -n "$sink" ] && pactl unload-module "$sink" 2>/dev/null || true
  if [ -z "${KEEP_WORK:-}" ]; then rm -rf "$work"; else echo "kept $work"; fi
}
trap cleanup EXIT
die() { echo "$1, see $work" >&2; KEEP_WORK=1; exit 1; }

cargo build --release --example sandbox

# BEVY_ASSET_ROOT: run from a copied binary and the game looks for assets beside it, finds none and draws nothing
# OCHRE_CLEAR_SKY: the weather runs on game time, so a flight that winds the clock on would sweep through the seed's storms
game_env=(BEVY_ASSET_ROOT="$PWD" OCHRE_SEED="$seed" OCHRE_SPAWN="$first_x,$first_z,5" OCHRE_TIME="$first_day"
  OCHRE_FLIGHT="$flight" OCHRE_FLIGHT_WARMUP="$warmup" OCHRE_CLEAR_SKY=1)

if [ -n "${SOUND:-}" ]; then
  echo "== 1/3 the sound: reusing $SOUND"
  cp "$SOUND" "$work/sound.wav"
else
  echo "== 1/3 the sound, in real time: $warmup s of warmup, then the flight"
  sink=$(pactl load-module module-null-sink sink_name=ochre_rec sink_properties=device.description=ochre_rec)
  # the music's first track starts three seconds into the flight, which `OCHRE_MUSIC_DELAY` counts from the game's start
  env "${game_env[@]}" PULSE_SINK=ochre_rec OCHRE_NOVSYNC=1 OCHRE_MUSIC_DELAY=$((warmup + 3)) \
    target/release/examples/sandbox >"$work/sound.log" 2>&1 &
  game=$!
  # move the game's sound stream onto the sink if `PULSE_SINK` did not take it
  for _ in $(seq 240); do
    kill -0 "$game" 2>/dev/null || die "the game died"
    ids=$(pactl -f json list sink-inputs | python3 -c '
import json, sys
pid = sys.argv[1]
print(" ".join(str(i["index"]) for i in json.load(sys.stdin) if i.get("properties", {}).get("application.process.id") == pid))' "$game")
    if [ -n "$ids" ]; then
      for id in $ids; do pactl move-sink-input "$id" ochre_rec; done
      break
    fi
    sleep 0.5
  done
  until grep -q "flythrough: flying" "$work/sound.log"; do
    kill -0 "$game" 2>/dev/null || die "the game died"
    sleep 0.1
  done
  length=$(awk -v t="$last" 'BEGIN { print t + 1 }')
  ffmpeg -hide_banner -loglevel warning -y -f pulse -i ochre_rec.monitor -t "$length" -c:a pcm_s16le "$work/sound.wav" 2>"$work/sound-ffmpeg.log" &
  ffmpeg=$!
  wait "$ffmpeg" || die "recording the sound failed"
  ffmpeg=""
  wait "$game" 2>/dev/null || true
  game=""
  pactl unload-module "$sink"
  sink=""
fi

echo "== 2/3 the picture, $frames frames on a fixed clock"
env "${game_env[@]}" OCHRE_FLIGHT_FRAMES="$work/frames" target/release/examples/sandbox >"$work/frames.log" 2>&1 &
game=$!
while kill -0 "$game" 2>/dev/null; do
  sleep 20
  echo "   $(find "$work/frames" -name 'frame_*.png' 2>/dev/null | wc -l) / $frames frames"
done
wait "$game" 2>/dev/null || true
game=""
have=$(find "$work/frames" -name 'frame_*.png' | wc -l)
[ "$have" -ge "$frames" ] || die "only $have of $frames frames were saved"

echo "== 3/3 putting them together"
# loudnorm: the game mixes its sound low under the ambience, a video wants -16 LUFS
ffmpeg -hide_banner -loglevel warning -y -framerate 60 -thread_queue_size 128 -i "$work/frames/frame_%05d.png" -i "$work/sound.wav" \
  -af "loudnorm=I=-16:TP=-1.5:LRA=11" -c:v libx264 -preset "$preset" -crf "$crf" -pix_fmt yuv420p \
  -c:a aac -b:a 192k -shortest -movflags +faststart "$out"
echo "recorded $out: $(ffprobe -v error -show_entries format=duration -of csv=p=0 "$out") s, $(du -h "$out" | cut -f1)"
level=$(ffmpeg -hide_banner -nostats -i "$out" -af volumedetect -f null - 2>&1 | sed -n 's/.*mean_volume: \(.*\) dB/\1/p')
echo "mean volume ${level:-none} dB"
if [ -z "$level" ] || awk -v l="$level" 'BEGIN { exit !(l < -40) }'; then
  echo "warning: the recording's sound is silent or missing" >&2
fi
