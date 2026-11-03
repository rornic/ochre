#!/bin/sh
# Usage: scripts/census.sh [first] [last] [port] > census.csv. Needs a running worldview.
first=${1:-1}; last=${2:-64}; port=${3:-8777}
header=1
for seed in $(seq "$first" "$last"); do
  body=$(curl -s "http://127.0.0.1:$port/summary?seed=$seed") || exit 1
  if [ "$header" = 1 ]; then
    echo "$body" | cut -d: -f1 | paste -sd, -; header=0
  fi
  echo "$body" | cut -d: -f2- | tr -d ' ' | paste -sd, -
done
