# njavac — the single command surface. Exact-byte and behavioral reference checks
# run through Docker: only the configured GraalVM javac in the built image is the
# reference, and every compiler build or execution uses an explicit target from
# that Dockerfile (see
# `docs/src/tooling/command-surface.md`).
#
#   make verify      [FILE=fixtures/x/F.java]  # fast gate: njavac vs cached goldens (may be stale)
#   make correctness [FILE=..]                 # fresh authoritative exact fixture check, no timing
#   make record      [FILE=..]                 # re-record goldens (after fixtures/JDK change), then verify
#   make bench       [FILE=..]                 # exact fixture check + controlled same-host timing
#   make probe       FILE=Probe.java           # disassemble a probe with the configured javac (javap -v -p)
#   make src-diff    FILE=Probe.java           # diff BOTH compilers on one source (byte + classdiff + javap)
#   make diff        A=a.class B=b.class       # structural class-file diff, in-container
#   make fuzz        [SEED=n] [COUNT=n]        # exact + behavioral differential fuzz (random seed unless pinned)
#   make fuzz-verify [COUNT=n]                 # sample worker output against the configured javac CLI
#   make fuzz-selftest                         # exercise narrow synthetic outcome/minimizer plumbing
#   make fuzz-observe-verify                   # exercise the persistent execution observer
#   make image                                 # build the fixture-acceptance image
#   make profile [ROUNDS=n] [TRIALS=n] [PHASE=all|lex|parse|sema|codegen|full]  # controlled hot profile
#   make docs                                  # serve the maintainer guide at localhost:3000
#   make docs-build                            # build the maintainer guide through Docker
#   make docs-check                            # build and check internal links through Docker

IMAGE           ?= njavac-acceptance
REFERENCE_IMAGE ?= njavac-reference
FUZZ_IMAGE      ?= njavac-fuzz
PROFILE_IMAGE   ?= njavac-profile
VOLUME          ?= njavac-goldens
GOLDENS         ?= /goldens
# Performance controls reduce same-host noise: pin one core, fix memory, no swap.
BENCH_CPU ?= 2
BENCH_MEM ?= 2g
FILE      ?=
A         ?=
B         ?=
# fuzz knobs (the fuzzer is NOT a timing benchmark, so it is not CPU-pinned).
# SEED is unset by default -> a fresh RANDOM seed each run (printed so a finding
# reproduces with `make fuzz SEED=<n>`); set SEED=n to pin it.
SEED      ?=
COUNT     ?= 5000
BATCH     ?=
FUZZFLAGS ?=
ROUNDS    ?= 1000
TRIALS    ?= 5
PHASE     ?= all
DOCS_IMAGE ?= njavac-docs:mdbook-0.5.4
DOCS_PORT  ?= 3000
DOCS_UID   := $(shell id -u)
DOCS_GID   := $(shell id -g)

.PHONY: help image reference-image fuzz-image profile-image probe src-diff verify correctness record bench diff fuzz fuzz-verify fuzz-selftest fuzz-observe-verify profile docs-image docs docs-build docs-check

help:  ## show this help
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | sed -E 's/:.*## /\t/' | sort

image:  ## build the fixture-acceptance Docker image
	docker build --target acceptance -t $(IMAGE) .

reference-image:
	docker build --target reference -t $(REFERENCE_IMAGE) .

fuzz-image:
	docker build --target fuzz -t $(FUZZ_IMAGE) .

profile-image:
	docker build --target profile -t $(PROFILE_IMAGE) .

probe: reference-image  ## disassemble a .java with the configured javac: make probe FILE=Probe.java
	@test -n "$(FILE)" || { echo "usage: make probe FILE=path/to/Probe.java"; exit 2; }
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint sh $(REFERENCE_IMAGE) -c \
	  'd=$$(mktemp -d); "$$JAVA_HOME/bin/javac" -d "$$d" "$(FILE)" && "$$JAVA_HOME/bin/javap" -v -p "$$d"/*.class'

src-diff: image  ## diff both compilers on ONE source: make src-diff FILE=Probe.java
	@test -n "$(FILE)" || { echo "usage: make src-diff FILE=path/to/Probe.java"; exit 2; }
	@docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint sh $(IMAGE) -c \
	  'jd=$$(mktemp -d); nd=$$(mktemp -d); n=$$(basename "$(FILE)" .java); \
	   "$$JAVA_HOME/bin/javac" -d "$$jd" "$(FILE)" || { echo "javac rejected"; exit 3; }; \
	   njavac -d "$$nd" "$(FILE)" || { echo "njavac rejected"; exit 4; }; \
	   if cmp -s "$$jd/$$n.class" "$$nd/$$n.class"; then echo "IDENTICAL: $$n"; else \
	     echo ">> bytes differ"; classdiff "$$jd/$$n.class" "$$nd/$$n.class" || true; \
	     "$$JAVA_HOME/bin/javap" -c -p "$$jd/$$n.class" > "$$jd/v"; \
	     "$$JAVA_HOME/bin/javap" -c -p "$$nd/$$n.class" > "$$nd/v"; \
	     echo "=== javap -c diff (< javac / > njavac) ==="; diff "$$jd/v" "$$nd/v" || true; \
	   fi'

verify: image  ## fast gate: njavac vs cached goldens (whole suite, or one FILE=path)
	@docker run --rm -v $(VOLUME):$(GOLDENS) --entrypoint sh $(IMAGE) \
	    -c 'ls $(GOLDENS)/*.class >/dev/null 2>&1' \
	  || { echo ">> golden cache empty — recording from the configured javac"; \
	       docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --record --golden-dir $(GOLDENS); }
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --offline --golden-dir $(GOLDENS) $(FILE)

correctness: image  ## fresh authoritative exact-byte fixture check (no timing)
	docker run --rm $(IMAGE) --no-timing $(FILE)

record: image  ## re-record goldens from the configured javac, then verify
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --record --golden-dir $(GOLDENS)
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --offline --golden-dir $(GOLDENS) $(FILE)

bench: image  ## exact-byte fixture check + controlled same-host Docker timing
	docker run --rm \
	  --cpuset-cpus=$(BENCH_CPU) --cpus=1 \
	  --memory=$(BENCH_MEM) --memory-swap=$(BENCH_MEM) --pids-limit=256 \
	  $(IMAGE) $(FILE)

diff: image  ## structural class-file diff in-container: make diff A=x.class B=y.class
	@test -n "$(A)" && test -n "$(B)" || { echo "usage: make diff A=a.class B=b.class"; exit 2; }
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint classdiff $(IMAGE) $(A) $(B)

fuzz: fuzz-image  ## exact + behavioral fuzz of random in-scope Java: make fuzz [SEED=n] [COUNT=n] [BATCH=n]
	docker run --rm -v "$(CURDIR)/fuzz-out:/work/fuzz-out" $(FUZZ_IMAGE) \
	  $(if $(SEED),--seed $(SEED),) --count $(COUNT) $(if $(BATCH),--batch $(BATCH),) $(FUZZFLAGS)

fuzz-verify: fuzz-image  ## sample the javac worker against the configured CLI: make fuzz-verify [COUNT=n]
	docker run --rm -v "$(CURDIR)/fuzz-out:/work/fuzz-out" $(FUZZ_IMAGE) \
	  $(if $(SEED),--seed $(SEED),) --count $(COUNT) $(if $(BATCH),--batch $(BATCH),) --verify-worker

fuzz-selftest: fuzz-image  ## exercise narrow synthetic outcome/minimizer plumbing
	docker run --rm -v "$(CURDIR)/fuzz-out:/work/fuzz-out" $(FUZZ_IMAGE) --selftest

fuzz-observe-verify: fuzz-image  ## exercise the persistent JVM observer and its timeout restart
	docker run --rm -v "$(CURDIR)/fuzz-out:/work/fuzz-out" $(FUZZ_IMAGE) --verify-observer

profile: profile-image  ## controlled in-process profile: make profile [ROUNDS=1000] [TRIALS=5] [PHASE=all|lex|parse|sema|codegen|full]
	docker run --rm \
	  --cpuset-cpus=$(BENCH_CPU) --cpus=1 \
	  --memory=$(BENCH_MEM) --memory-swap=$(BENCH_MEM) --pids-limit=256 \
	  $(PROFILE_IMAGE) $(ROUNDS) $(TRIALS) $(PHASE)

docs-image:  ## build the pinned mdBook + Mermaid documentation image
	docker build -f docs/Dockerfile -t $(DOCS_IMAGE) .

docs: docs-image  ## serve the maintainer guide at http://localhost:3000 (override DOCS_PORT)
	docker run --rm --init \
	  --user "$(DOCS_UID):$(DOCS_GID)" \
	  --mount type=bind,source="$(CURDIR)",target=/work \
	  --workdir /work \
	  --publish "127.0.0.1:$(DOCS_PORT):3000" \
	  $(DOCS_IMAGE) \
	  mdbook serve docs --hostname 0.0.0.0 --port 3000

docs-build: docs-image  ## build the maintainer guide through Docker
	docker run --rm \
	  --user "$(DOCS_UID):$(DOCS_GID)" \
	  --mount type=bind,source="$(CURDIR)",target=/work \
	  --workdir /work \
	  $(DOCS_IMAGE) \
	  mdbook build docs

docs-check: docs-build  ## inventory sources, then check rendered internal links through Docker
	docker run --rm \
	  --user "$(DOCS_UID):$(DOCS_GID)" \
	  --mount type=bind,source="$(CURDIR)",target=/work,readonly \
	  --workdir /work \
	  $(DOCS_IMAGE) \
	  sh docs/check-summary.sh
	docker run --rm \
	  --mount type=bind,source="$(CURDIR)/docs/book",target=/input,readonly \
	  lycheeverse/lychee:0.24.2@sha256:e2d19e57cf6ab037026f20b8e449a1f30d9d7f81eef4194763aab2eab20bd28d \
	  --offline --no-progress --include-fragments=anchor-only --root-dir /input /input
