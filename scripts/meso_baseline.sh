#!/usr/bin/env bash
# The scorer's top places for a set of seeds, from a running worldview, as one CSV with the seed first.
# usage: scripts/meso_baseline.sh [out.csv] [seed ...]      env: PORT=8777 TOP=10 NULL=1 (the null run) TAIL=0.95
# Needs `cargo run --release --example worldview -- 1 8777` running. Each new seed takes about 10 s to generate and score.
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
out=${1:-docs/meso-baseline.csv}; shift || true
seeds=("$@"); [ ${#seeds[@]} -eq 0 ] && seeds=(1 3 4 20 42 34 35 12 64 62 30)
port=${PORT:-8777}; top=${TOP:-10}; tail=${TAIL:-0.95}; null=${NULL:-0}
version=$(sed -n 's/^pub const GENERATOR_VERSION: u32 = \([0-9]*\);.*/\1/p' crates/ochre-world/src/worldmap/mod.rs)
{
  echo "# meso scorer baseline: git $(git rev-parse --short HEAD), GENERATOR_VERSION $version, top $top per biome, tail $tail, null $null, grid step 64 blocks, ground as far LOD draws it (cell 32)"
  echo "# provisional first-pass terms, see docs/designs/meso-scale-geography.md. Lines starting # are comments."
  echo "seed,biome,rank,x,z,kind,score,relief_landmark,relief_mass,relief_range,steepness,weakest"
  for seed in "${seeds[@]}"; do
    curl -sf "http://127.0.0.1:$port/places?seed=$seed&top=$top&tail=$tail&null=$null" | tail -n +2 | sed "s/^/$seed,/"
  done
} > "$out"
echo "wrote $out: $(grep -vc '^#\|^seed,' "$out") places over ${#seeds[@]} seeds"
