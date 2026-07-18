# Troubleshooting

Start with the exact command output and classify the failure as environment,
cache, compiler, worker, observer, or documentation. Do not switch to host javac
or a local benchmark to bypass a Docker failure; that changes the reference and
invalidates the result.

## Docker image does not build

Check that Docker is running, BuildKit is available, the host can download the
pinned toolchains on the first build, and enough disk space remains for the JDK
and Cargo layers. Re-run the failing Make target rather than reproducing selected
Dockerfile commands by hand; the target preserves the expected tag and context.

An SDKMAN, Cargo registry, or target cache miss makes a build slower but should not
change output. A checksum failure in the documentation image is a hard supply or
version mismatch; do not bypass it.

## Benchmark rejects the CPU selection

`make bench` pins one CPU by host index. If Docker reports an invalid CPU set,
choose an index visible to Docker with `BENCH_CPU`. On constrained desktop Docker
VMs, the host's logical CPU numbers and Docker's available set may differ.

Keep `BENCH_MEM` fixed between compared runs. Changing CPU or memory settings can
make the command run, but the resulting timing is not comparable to a baseline
made under different controls.

## `make verify` fails unexpectedly

The golden volume can be stale. `make verify` auto-records only when the volume has
no class files; it does not detect fixture or JDK changes in a nonempty cache.

Run:

```sh
make record
make correctness
```

`make record` refreshes the complete suite from pinned javac. `make correctness`
then removes the cache from the diagnosis by comparing against a fresh invocation.
If fresh correctness still fails, treat it as a real divergence and follow
[Differential Debugging](../tooling/differential-debugging.md).

If every fixture fails only in offline mode, stale, incomplete, or incompatible
goldens are especially likely. Never repair a golden by copying or editing a class
file manually.

## `record FILE=...` updates more than one case

This is intentional. The `record` target does not pass `FILE` to its recording
step, so pinned javac records the whole suite. `FILE` filters only the offline
verification that follows. See [Fixtures and Goldens](../tooling/fixtures-and-goldens.md).

## A focused fixture command cannot see an ad hoc source

`verify`, `correctness`, and `bench` use sources copied into the main image and do
not bind-mount the repository. Their `FILE` mode is intended for fixtures present
in that image. Use `make src-diff FILE=...` for an ad hoc source under the
repository; that target mounts the working tree.

All main targets rebuild or re-evaluate the image dependency first, so a newly
added fixture should become visible. If it does not, inspect Docker build context
and ignore rules before changing the harness.

## `src-diff` succeeded but printed a mismatch

This is expected behavior. `src-diff` is a diagnostic command: when both compilers
accept, it ignores the nonzero statuses from `classdiff` and textual `diff` so all
diagnostics print. It therefore returns success even for byte divergence.

Read `IDENTICAL` versus `bytes differ` in its output. Use
`make correctness FILE=fixtures/.../Case.java` or `make correctness` when the exit
status must enforce byte identity.

## `javap` agrees but bytes differ

`javap` is not a complete byte renderer. Trust the byte comparison and inspect the
structural `classdiff` path and offset. Constant-pool ordering, attribute bytes, or
other representational differences can remain invisible in disassembly. Use
`make diff` for two retained classes or the report printed by the fixture harness.

## Fuzzer workers cannot start

Errors naming `tools/FuzzJavac.java` or `tools/FuzzObserve.java` usually mean the
fuzz binary was run in the image without the repository mount or from the wrong
working directory. The main image deliberately does not contain those Java source
files.

Run the corresponding `make fuzz...` target from the repository root. It mounts
the repository at `/w` and sets `/w` as the container workdir. If worker paths were
explicitly overridden, restore paths visible inside that mount.

## `make fuzz` exits zero with byte divergences

The fuzzer's operational hard-fail condition is behavioral difference, invalid
njavac syntax rejection, or panic. A byte divergence whose observer output matches
is telemetry, so the process can exit zero.

This does not make the drift acceptable: exact bytes are the product contract.
Record the seed and structural signature, reduce the case, and add a fixture once
the mismatch is understood. Normal byte-only telemetry does not write a finding
bundle, so preserve the printed example before starting another run.

## Fuzzer output is missing

Hard findings normally persist under ignored `fuzz-out/` because Make bind-mounts
the repository. Byte-only behavior matches print telemetry but do not write a
normal artifact bundle. Unsupported and worker mismatch dumps are capped, so the
summary count may exceed the number of saved examples.

With the default stop-on-finding mode, the run exits after its first hard finding.
Use the binary's documented keep-going mode through `FUZZFLAGS` only when a census
is useful; always retain the printed seed.

## Observer times out or restarts

Timeout is a modeled observation, not automatically a worker crash. The observer
allows two seconds per class, marks the unrun peer when necessary, and the Rust
driver restarts the JVM before continuing or reversing the pair. Run
`make fuzz-observe-verify` if timeout recovery itself is suspect.

The observer is valid only for the current generated subset. Do not enable fuzz
generation of input reads, `System.exit`, threads, or persistent global mutation
without redesigning the isolation boundary.

## Worker verification diverges

Treat a `make fuzz-verify` failure as an invalid fuzzer oracle, not an njavac
compiler finding. Inspect artifacts under `fuzz-out/worker-mismatch/` and compare
CLI acceptance and bytes with worker output. Changes to the JDK, virtual source
name, compiler options, batching, in-memory file manager, or worker protocol can
all invalidate the worker.

Do not rely on normal fuzz results until the worker agrees with the pinned CLI.

## Local build or profile is green

`make check` proves only that a host release build completed. `make profile`
measures local in-process performance. Neither invokes pinned javac or compares
bytes, so neither is acceptance evidence. Run `make correctness` separately.

If profile numbers move sharply on macOS, verify that both runs used the same
power mode and comparable thermal/background conditions. Low Power Mode and
regular performance mode are not comparable baselines.

## Documentation page is absent from the book

mdBook renders pages listed by `docs/src/SUMMARY.md`. A source file can exist and
still be omitted from `docs/book/` and from the rendered-link check. Ensure the
navigation owner includes the page, then run `make docs-check`.

If `make docs` cannot bind its port, override `DOCS_PORT`. The server publishes on
loopback only. If generated files have wrong ownership, use the Make target rather
than a root-run direct container; the target supplies the host UID/GID.

## No CI check appeared

The repository currently has no hosted workflow under `.github/workflows`.
Correctness, fuzzing, and documentation gates must be run explicitly. See
[Docker and CI](../tooling/docker-and-ci.md) for the current automation boundary
and [Command Surface](../tooling/command-surface.md) for gate selection.
