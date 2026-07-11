#!/usr/bin/env bash
# Build the pinned image and run the benchmark under a deterministic CPU/memory
# envelope. Timing repeatability comes from the `docker run` flags here, not the
# image: one pinned core, fixed memory, no swap, no other tenants.
set -euo pipefail

IMAGE=njavac-bench
# Pin work to a single core so scheduler migration and turbo/contention noise
# don't dominate. CPU 0 is often busiest; use a mid core. Adjust to your host.
CPU="${BENCH_CPU:-2}"
MEM="${BENCH_MEM:-2g}"

echo ">> building image (pinned rust + graalce 25.0.2)"
docker build -t "$IMAGE" .

echo ">> running benchmark pinned to cpu $CPU, mem $MEM, swap disabled"
exec docker run --rm \
    --cpuset-cpus="$CPU" \
    --cpus=1 \
    --memory="$MEM" \
    --memory-swap="$MEM" \
    --pids-limit=256 \
    "$IMAGE" "$@"
