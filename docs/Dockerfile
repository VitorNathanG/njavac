# syntax=docker/dockerfile:1.7

FROM alpine:3.23.3@sha256:25109184c71bdad752c8312a8623239686a9a2071e8825f20acb8f2198c3f659 AS fetch

RUN apk add --no-cache ca-certificates curl

ARG TARGETARCH

RUN set -eux; \
    case "${TARGETARCH}" in \
        amd64) \
            target="x86_64-unknown-linux-musl"; \
            mdbook_sha="5222beabd3e37dc5be0d18ff99b79058469354db5c220153a1b92db5ba12be89"; \
            mermaid_sha="c3a924da1c1895d694f0544fb8ce71ae01508138d8e9b02374e9bf608ae765d7" \
            ;; \
        arm64) \
            target="aarch64-unknown-linux-musl"; \
            mdbook_sha="753e5c5c363ee8a56972344dcf91466f005a51db84a7aeffe427ae3ef83d6d44"; \
            mermaid_sha="372c2e8ef1f59f65e40887517ecf353587c3c30fb7f4911e2ec250e0ca61d806" \
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
    curl -fsSL \
      "https://github.com/badboy/mdbook-mermaid/releases/download/v0.17.0/mdbook-mermaid-v0.17.0-${target}.tar.gz" \
      -o /tmp/mdbook-mermaid.tar.gz; \
    echo "${mermaid_sha}  /tmp/mdbook-mermaid.tar.gz" | sha256sum -c -; \
    tar -xzf /tmp/mdbook-mermaid.tar.gz -C /out mdbook-mermaid; \
    chmod 0755 /out/mdbook /out/mdbook-mermaid

FROM alpine:3.23.3@sha256:25109184c71bdad752c8312a8623239686a9a2071e8825f20acb8f2198c3f659

COPY --from=fetch /out/mdbook /out/mdbook-mermaid /usr/local/bin/

WORKDIR /work
EXPOSE 3000

CMD ["mdbook", "--version"]
