# njavac

njavac is an experimental Java 25 to JVM bytecode compiler written in Rust. It
supports a deliberately small language surface under a strict behavioral
compatibility contract and retains the repository-pinned `javac` class bytes
whenever practical.

It is a compiler research project, not a replacement for javac.

## Documentation

The [njavac Maintainer Guide](docs/src/index.md) is the authoritative source for
project behavior, architecture, tooling, workflows, language coverage, research,
and plans.

- [Quickstart](docs/src/start/quickstart.md)
- [Exact language support](docs/src/reference/language-support.md)
- [Compatibility contract](docs/src/reference/compatibility-contract.md)
- [Current architecture](docs/src/architecture/overview.md)
- [Developer tooling](docs/src/tooling/command-surface.md)
- [Maintainer workflow](docs/src/contributing/workflow.md)
- [Active work](docs/src/direction/active-work.md)

Serve the searchable browser version through Docker:

```bash
make docs
```

Then open <http://localhost:3000>. Run `make docs-check` before committing
documentation changes.

## License

[MIT](LICENSE)
