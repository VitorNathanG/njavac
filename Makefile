# njavac — the single command surface. Everything that validates byte-identity
# runs through Docker: only the pinned GraalVM javac in the image reproduces the
# golden bytes (see CLAUDE.md §Testing). `check` is a LOCAL build for
# compiler-internal debugging only, never acceptance.
#
#   make verify      [FILE=fixtures/x/F.java]  # fast gate: njavac vs cached goldens (may be stale)
#   make correctness [FILE=..]                 # fresh authoritative online check, no timing
#   make record      [FILE=..]                 # re-record goldens (after fixtures/JDK change), then verify
#   make bench       [FILE=..]                 # authoritative: full online correctness + deterministic timing
#   make probe       FILE=Probe.java           # disassemble a probe with the pinned javac (javap -v -p)
#   make src-diff    FILE=Probe.java           # diff BOTH compilers on one source (byte + classdiff + javap)
#   make diff        A=a.class B=b.class       # structural class-file diff, in-container
#   make fuzz        [SEED=n] [COUNT=n]        # differential fuzz vs pinned javac (random seed unless SEED=n)
#   make fuzz-selftest                         # prove the finding->minimize->report machinery
#   make image                                 # build the pinned image
#   make check                                 # LOCAL release build (debugging only; NOT a test)

IMAGE     := njavac-bench
VOLUME    := njavac-goldens
GOLDENS   := /goldens
# bench timing determinism: pin one core, fix memory, no swap. Override per host.
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

.PHONY: help image probe src-diff verify correctness record bench diff fuzz fuzz-selftest check

help:  ## show this help
	@grep -E '^[a-z-]+:.*##' $(MAKEFILE_LIST) | sed -E 's/:.*## /\t/' | sort

image:  ## build the pinned Docker image
	docker build -t $(IMAGE) .

probe: image  ## disassemble a .java with the pinned javac: make probe FILE=Probe.java
	@test -n "$(FILE)" || { echo "usage: make probe FILE=path/to/Probe.java"; exit 2; }
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint sh $(IMAGE) -c \
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
	  || { echo ">> golden cache empty — recording from the pinned javac"; \
	       docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --record --golden-dir $(GOLDENS); }
	docker run --rm -v $(VOLUME):$(GOLDENS) $(IMAGE) --offline --golden-dir $(GOLDENS) $(FILE)

correctness: image  ## fresh authoritative online byte-identity check (no timing)
	docker run --rm $(IMAGE) --no-timing $(FILE)

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

fuzz: image  ## differential fuzz random in-scope Java (random seed unless SEED=n): make fuzz [SEED=n] [COUNT=n] [BATCH=n]
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint fuzz $(IMAGE) \
	  $(if $(SEED),--seed $(SEED),) --count $(COUNT) $(if $(BATCH),--batch $(BATCH),) $(FUZZFLAGS)

fuzz-selftest: image  ## prove the finding->minimize->report machinery (no real bug needed)
	docker run --rm -v "$(CURDIR):/w" -w /w --entrypoint fuzz $(IMAGE) --selftest

check:  ## LOCAL release build only — compiler-internal debugging, NOT acceptance
	cargo build --release
