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
#   * cargo registry + target/ -> cache mounts, so dependency and incremental
#     compilation are reused; only changed sources recompile.
#
# Determinism:
#   * GraalVM 25.0.2 archives are selected by architecture and SHA-256 verified.
#   * Base images are digest-pinned; Rust dependencies use `cargo build --locked`.
# Timing repeatability (CPU pinning, memory caps) lives in the Makefile `bench`
# target's `docker run` flags.

# ---- Stage 1: fetch and verify the exact reference JDK ----------------------
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS jdk-fetch
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && case "$TARGETARCH" in \
         amd64) archive=graalvm-community-jdk-25.0.2_linux-x64_bin.tar.gz; \
                sha=e0be791c8fda4d03b6b0a0cb824fef3149736170057b3a515252b44419606af0 ;; \
         arm64) archive=graalvm-community-jdk-25.0.2_linux-aarch64_bin.tar.gz; \
                sha=b4580d9f223d0a4b3a1757e58b18ff4c1db950e67e105fc5cb741457d2384a71 ;; \
         *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
       esac \
    && curl -fsSL "https://github.com/graalvm/graalvm-ce-builds/releases/download/jdk-25.0.2/$archive" -o /tmp/graalvm.tar.gz \
    && echo "$sha  /tmp/graalvm.tar.gz" | sha256sum -c - \
    && mkdir -p /opt/graalvm \
    && tar -xzf /tmp/graalvm.tar.gz --strip-components=1 -C /opt/graalvm

# ---- Stage 2: exact runtime JDK on a content-addressed base -----------------
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS jdk
COPY --from=jdk-fetch /opt/graalvm /opt/graalvm
ENV JAVA_HOME=/opt/graalvm
ENV JAVAC=$JAVA_HOME/bin/javac

# ---- Stage 3: build njavac with pinned toolchain + cached compilation -------
FROM rust:1.95-slim-bookworm@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082 AS build
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

# ---- Stage 4: runtime — JDK base + the small, frequently-changing layers ----
FROM jdk AS bench
WORKDIR /work
# Marks this as the controlled harness so `bench` will produce timings.
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
