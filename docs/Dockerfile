# syntax=docker/dockerfile:1.7

FROM alpine:3.23.3@sha256:25109184c71bdad752c8312a8623239686a9a2071e8825f20acb8f2198c3f659 AS fetch

RUN apk add --no-cache ca-certificates curl

ARG TARGETARCH

RUN set -eux; \
    case "${TARGETARCH}" in \
        amd64) \
            target="x86_64-unknown-linux-musl"; \
            mdbook_sha="5222beabd3e37dc5be0d18ff99b79058469354db5c220153a1b92db5ba12be89" \
            ;; \
        arm64) \
            target="aarch64-unknown-linux-musl"; \
            mdbook_sha="753e5c5c363ee8a56972344dcf91466f005a51db84a7aeffe427ae3ef83d6d44" \
            ;; \
        *) \
            echo "unsupported architecture: ${TARGETARCH}" >&2; exit 1 \
            ;; \
    esac; \
    mkdir /out; \
    curl -fsSL \
      "https://github.com/rust-lang/mdBook/releases/download/v0.5.4/mdbook-v0.5.4-${target}.tar.gz" \
      -o /tmp/mdbook.tar.gz; \
    echo "${mdbook_sha}  /tmp/mdbook.tar.gz" | sha256sum -c -; \
    tar -xzf /tmp/mdbook.tar.gz -C /out mdbook; \
    chmod 0755 /out/mdbook

FROM rust:1.95-slim-bookworm@sha256:d7482085ff5b415f84dba5647ae71606650bdef00db7aeb69f4b3d170c3e4082 AS mermaid-build

RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN curl -fsSL \
      "https://static.crates.io/crates/mdbook-mermaid/mdbook-mermaid-0.17.0.crate" \
      -o /tmp/mdbook-mermaid.crate \
    && echo "7676e3d7e2ca3218b7c67ca43c0b5d01173ad8e29fd9bcaad2fece19129d6a3d  /tmp/mdbook-mermaid.crate" | sha256sum -c - \
    && tar -xzf /tmp/mdbook-mermaid.crate --strip-components=1 -C /src
COPY docs/mdbook-mermaid.Cargo.lock /src/Cargo.lock
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked --bin mdbook-mermaid \
    && cp target/release/mdbook-mermaid /out-mdbook-mermaid

FROM debian:bookworm-slim@sha256:7b140f374b289a7c2befc338f42ebe6441b7ea838a042bbd5acbfca6ec875818

COPY --from=fetch /out/mdbook /usr/local/bin/mdbook
COPY --from=mermaid-build /out-mdbook-mermaid /usr/local/bin/mdbook-mermaid

WORKDIR /work
EXPOSE 3000

CMD ["mdbook", "--version"]
