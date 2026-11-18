#!/usr/bin/env bash
# Prints (and with RUN=1 starts the game at) a place from the scorer's baseline CSV.
# usage: scripts/meso_spawn.sh <biome> [rank] [seed]      env: CSV=docs/meso-baseline.csv RUN=1
# With no seed it takes the best-scoring place of that biome over all seeds in the file (rank 1 is the best, 2 the next ...).
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
csv=${CSV:-docs/meso-baseline.csv}; biome=$1; rank=${2:-1}; seed=${3:-}
row=$(grep -v '^#\|^seed,' "$csv" | awk -F, -v b="$biome" -v s="$seed" '$2==b && (s=="" || $1==s)' | sort -t, -k7,7gr -k4,4g | sed -n "${rank}p")
[ -n "$row" ] || { echo "no $biome place of that rank in $csv" >&2; exit 1; }
IFS=, read -r s _ r x z kind score landmark mass range steep water above weakest <<<"$row"
echo "seed $s, $biome rank $r in its seed: $kind, score $score, relief $landmark / $mass / $range blocks, steepness $steep, water $water, $above blocks above the sea"
cmd="OCHRE_SEED=$s OCHRE_SPAWN=$x,$z,60 OCHRE_CLEAR_SKY=1 OCHRE_TIME=0.45 cargo run --release --example sandbox"
echo "$cmd"
[ "${RUN:-}" = 1 ] && eval "$cmd"
true
