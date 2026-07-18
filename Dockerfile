# syntax=docker/dockerfile:1
#
# Deterministic build and execution environments for njavac.
#
# Shared stages isolate the pinned reference JDK from the Rust build. Final
# targets then add only the capabilities needed by reference probes, fixture
# acceptance, benchmarking, or fuzzing.
#
# Caching:
#   * cargo registry + target/ -> cache mounts, so dependency and incremental
#     compilation are reused; only changed sources recompile.
#
# Determinism:
#   * GraalVM 25.0.2 archives are selected by architecture and SHA-256 verified.
#   * Base images are digest-pinned; Rust dependencies use `cargo build --locked`.
# Timing repeatability (CPU pinning, memory caps) lives in the Makefile
# `benchmark` target's `docker run` flags.

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

# ---- Stage 2: runnable pinned reference environment -------------------------
FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818 AS reference
COPY --from=jdk-fetch /opt/graalvm /opt/graalvm
ENV JAVA_HOME=/opt/graalvm
ENV JAVAC=$JAVA_HOME/bin/javac

# ---- Stage 3: build njavac with pinned toolchain + cached compilation -------
FROM rust:1.95-slim-bookworm@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082 AS rust-build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
# target/ is a cache mount (not in the layer), so copy the binaries out to a
# real path for downstream capability targets.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked \
    && mkdir -p /out \
    && cp target/release/njavac target/release/benchmark \
          target/release/benchmark_alloc target/release/classdiff \
          target/release/fuzz /out/

# ---- Stage 4: fixture acceptance, benchmarking, and paired diagnostics ------
FROM reference AS acceptance
WORKDIR /work
# Marks this as the controlled harness so `bench` will produce timings.
ENV NJAVAC_IN_CONTAINER=1
COPY fixtures ./fixtures
COPY --from=rust-build /out/njavac    /usr/local/bin/njavac
COPY --from=rust-build /out/benchmark /usr/local/bin/benchmark
COPY --from=rust-build /out/benchmark_alloc /usr/local/bin/benchmark_alloc
# The structural class-file differ, reachable for debugging via `make diff`; it
# also backs the diff `benchmark` prints on a mismatch.
COPY --from=rust-build /out/classdiff /usr/local/bin/classdiff
ENTRYPOINT ["benchmark", "--njavac", "/usr/local/bin/njavac", "--alloc-helper", "/usr/local/bin/benchmark_alloc"]

# ---- Stage 5: self-contained differential fuzzing ---------------------------
FROM reference AS fuzz
WORKDIR /work
COPY --from=rust-build /out/fuzz /usr/local/bin/fuzz
COPY tools/FuzzJavac.java tools/FuzzObserve.java /opt/njavac/tools/
ENV FUZZ_WORKER=/opt/njavac/tools/FuzzJavac.java
ENV FUZZ_OBSERVER=/opt/njavac/tools/FuzzObserve.java
ENTRYPOINT ["fuzz"]
