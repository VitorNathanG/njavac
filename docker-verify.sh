#!/usr/bin/env bash
# Fast, on-policy correctness gate (ROADMAP §0.5).
#
# Byte-compares njavac against goldens produced by the PINNED javac INSIDE the
# image and persisted in a Docker volume, so javac's cost is paid once (at record)
# rather than on every check. Everything runs in Docker — the volume is only cache
# storage on the host, populated by the container's pinned javac; it is never
# hand-edited and never committed. This makes the mandatory correctness gate fast
# without leaving Docker or trusting a host toolchain.
#
#   ./docker-verify.sh                       # fast correctness over all fixtures
#   ./docker-verify.sh fixtures/x/Foo.java   # fast correctness for one fixture
#   ./docker-verify.sh --record [<file>]     # (re)record goldens first, then verify
#
# The gate auto-records when the volume is empty. Re-record with --record after
# changing fixtures (or rebuilding the image on a new JDK), otherwise the cache
# goes stale. For the authoritative from-scratch check plus timing, use
# docker-bench.sh (a full online run against freshly-invoked pinned javac).
set -euo pipefail

IMAGE=njavac-bench
VOL=njavac-goldens
G=/goldens

echo ">> building pinned image"
docker build -t "$IMAGE" .

force_record=0
if [[ "${1:-}" == "--record" ]]; then force_record=1; shift; fi

# Record when forced, or when the volume holds no goldens yet.
if [[ $force_record -eq 1 ]] || ! docker run --rm -v "$VOL:$G" --entrypoint sh "$IMAGE" \
        -c "ls $G/*.class >/dev/null 2>&1"; then
    echo ">> recording goldens from the pinned javac into volume '$VOL'"
    docker run --rm -v "$VOL:$G" "$IMAGE" --record --golden-dir "$G"
fi

echo ">> offline correctness (njavac vs cached pinned goldens; no javac spawns)"
exec docker run --rm -v "$VOL:$G" "$IMAGE" --offline --golden-dir "$G" "$@"
