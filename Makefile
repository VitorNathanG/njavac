# njavac — the single command surface. Everything that validates byte-identity
# runs through Docker: only the pinned GraalVM javac in the image reproduces the
# golden bytes (see CLAUDE.md §Testing). `check` is a LOCAL build for
# compiler-internal debugging only, never acceptance.
#
#   make verify [FILE=fixtures/x/F.java]  # fast Docker correctness gate
#   make record [FILE=..]                 # re-record goldens (after fixtures/JDK change), then verify
#   make bench  [FILE=..]                 # authoritative: full online correctness + deterministic timing
#   make probe  FILE=Probe.java           # disassemble a probe with the pinned javac (javap -v -p)
#   make diff   A=a.class B=b.class       # structural class-file diff, in-container
#   make image                            # build the pinned image
#   make check                            # LOCAL release build (debugging only; NOT a test)

IMAGE     := njavac-bench
VOLUME    := njavac-goldens
GOLDENS   := /goldens
# bench timing determinism: pin one core, fix memory, no swap. Override per host.
BENCH_CPU ?= 2
BENCH_MEM ?= 2g
FILE      ?=
A         ?=
B         ?=

.PHONY: help image probe verify record bench diff check

help:  ## show this help
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | sed -E 's/:.*## /\t/' | sort

image:  ## build the pinned Docker image
	docker build -t $(IMAGE) .

probe: image  ## disassemble a .java with the pinned javac: make probe FILE=Probe.java
	@test -n "$(FILE)" || { echo "usage: make probe FILE=path/to/Probe.java"; exit 2; }
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint sh $(IMAGE) -c \
	  'd=$$(mktemp -d); "$$JAVA_HOME/bin/javac" -d "$$d" "$(FILE)" && "$$JAVA_HOME/bin/javap" -v -p "$$d"/*.class'

verify: image  ## fast Docker correctness gate (whole suite, or one FILE=path)
	@docker run --rm -v $(VOLUME):$(GOLDENS) --entrypoint sh $(IMAGE) \
	    -c 'ls $(GOLDENS)/*.class >/dev/null 2>&1' \
	  || { echo ">> golden cache empty — recording from the pinned javac"; \
	       docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --record --golden-dir $(GOLDENS); }
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --offline --golden-dir $(GOLDENS) $(FILE)

record: image  ## re-record goldens from the pinned javac into the volume, then verify
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --record --golden-dir $(GOLDENS)
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --offline --golden-dir $(GOLDENS) $(FILE)

bench: image  ## authoritative Docker run: full online correctness + deterministic timing
	docker run --rm \
	  --cpuset-cpus=$(BENCH_CPU) --cpus=1 \
	  --memory=$(BENCH_MEM) --memory-swap=$(BENCH_MEM) --pids-limit=256 \
	  $(IMAGE) $(FILE)

diff: image  ## structural class-file diff in-container: make diff A=x.class B=y.class
	@test -n "$(A)" && test -n "$(B)" || { echo "usage: make diff A=a.class B=b.class"; exit 2; }
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint classdiff $(IMAGE) $(A) $(B)

check:  ## LOCAL release build only — compiler-internal debugging, NOT acceptance
	cargo build --release
