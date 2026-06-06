# pds

`pds` is a gate-and-query CLI for [sphinx-needs](https://sphinx-needs.readthedocs.io/)-backed
repositories. It drives a sphinx-needs corpus build and enforces quality gates, emitting
structured JSON to stdout and exiting with a well-defined code:

| Exit code | Meaning |
|-----------|---------|
| 0 | All checks passed. |
| 1 | One or more needs-corpus findings (violations). |
| 2 | Tool or configuration error (pds itself could not run the check). |

All diagnostic output is a single JSON object on stdout. Human-readable detail
goes to stderr. This makes pds suitable as a step in CI pipelines and agent
workflows that consume machine-readable results.

## Workspace crates

| Crate | Purpose |
|-------|---------|
| [`pds-core`](pds-core/) | Library — sphinx-needs corpus gate-and-query primitives |
| [`pds-cli`](pds-cli/) | Binary (`pds`) — CLI driver built on pds-core |

## Installation

### From crates.io (stable)

```sh
cargo install pds-cli
```

The installed binary is named `pds`.

### From GitHub Releases

Pre-built binaries for Linux x86_64, macOS x86_64, and macOS aarch64 are
attached to each [GitHub Release](https://github.com/patdhlk/skills/releases).
A one-line installer will be added in a forthcoming release — check the release
notes for the `install.sh` link.

## Usage

```sh
# Run the full strict gate (build corpus + enforce rules).
pds check

# Build the corpus only (produces needs.json, exits 0/1/2).
pds build

# Override the config path.
pds check --config path/to/ubproject.toml
```

## Configuration

`pds` reads `ubproject.toml` in the current directory (or the path given by
`--config`). See the [spec repo](https://github.com/patdhlk/skills/tree/main/spec)
for the full schema reference.

```toml
[tool.patdhlk-skills]
builder  = "ubc"        # "ubc" or "sphinx-build"
spec_dir = "spec"
```

## Contributing

The project dogfoods its own spec: issues, requirements, and architectural
decisions live in `spec/` as sphinx-needs directives. See the repo root
[CLAUDE.md](../CLAUDE.md) for the workflow.
