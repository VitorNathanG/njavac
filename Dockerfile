# syntax=docker/dockerfile:1
#
# Deterministic benchmark environment for njavac.
#
# Ordering rationale: the JDK is set up FIRST and is the base of the final
# image. It's the slowest, most stable layer (a ~350 MB GraalVM install that
# almost never changes), so keeping it at the bottom of the stack means every
# rebuild after a code change reuses it from cache and only re-lays the tiny
# binary/reference layers on top.
#
# Caching:
#   * SDKMAN archives  -> cache mount, so the GraalVM zip is downloaded once and
#     reused across rebuilds (and never bloats an image layer).
#   * cargo registry + target/ -> cache mounts, so dependency and incremental
#     compilation are reused; only changed sources recompile.
#
# Determinism:
#   * 25.0.2-graalce is the live reference used to produce fresh or cached goldens.
#   * pinned rust:1.95 toolchain + `cargo build --locked`.
# Timing repeatability (CPU pinning, memory caps) lives in the Makefile `bench`
# target's `docker run` flags.

# ---- Stage 1: JDK — set up first, forms the base of the runtime image -------
FROM debian:bookworm-slim AS jdk
ENV SDKMAN_DIR=/opt/sdkman
ENV JAVA_HOME=/opt/sdkman/candidates/java/25.0.2-graalce
ENV JAVAC=$JAVA_HOME/bin/javac
RUN apt-get update && apt-get install -y --no-install-recommends \
        curl zip unzip bash ca-certificates \
    && rm -rf /var/lib/apt/lists/*
# Archives go to a cache mount: downloaded once, reused, absent from the layer.
RUN --mount=type=cache,target=/opt/sdkman/archives,sharing=locked \
    curl -s "https://get.sdkman.io" | bash \
    && bash -c 'source $SDKMAN_DIR/bin/sdkman-init.sh \
        && sdk install java 25.0.2-graalce'

# ---- Stage 2: build njavac with pinned toolchain + cached compilation -------
FROM rust:1.95-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# target/ is a cache mount (not in the layer), so copy the binaries out to a
# real path for the final stage to pull from.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && mkdir -p /out \
    && cp target/release/njavac target/release/bench target/release/classdiff target/release/fuzz /out/

# ---- Stage 3: runtime — JDK base + the small, frequently-changing layers ----
FROM jdk AS bench
WORKDIR /work
# Marks this as the deterministic harness so `bench` will produce timings.
ENV NJAVAC_IN_CONTAINER=1
# fixtures change rarely; binaries change most, so copy them last.
COPY fixtures ./fixtures
COPY --from=build /out/njavac    /usr/local/bin/njavac
COPY --from=build /out/bench     /usr/local/bin/bench
# The structural class-file differ, reachable for debugging via `make diff`; it
# also backs the diff `bench` prints on a mismatch.
COPY --from=build /out/classdiff /usr/local/bin/classdiff
# The two-layer differential fuzzer, documented in `docs/src/tooling/fuzzing.md`
# (entrypoint override). Its source-launched workers use the pinned JDK.
COPY --from=build /out/fuzz     /usr/local/bin/fuzz
ENTRYPOINT ["bench", "--njavac", "/usr/local/bin/njavac"]
CMD ["--njavac-runs", "1000", "--javac-runs", "5", "--warmup", "5"]
