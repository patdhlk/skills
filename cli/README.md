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

### From GitHub Releases (shell installer)

> **(first release pending)** — the install one-liner below works once the
> first `v*` tag has been released; until then the download URL 404s.

Each [GitHub Release](https://github.com/patdhlk/skills/releases) ships pre-built
binaries for macOS (aarch64 and x86_64) and static-musl Linux (aarch64 and
x86_64), with SHA-256 checksums. Install the latest with the generated shell
installer:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/patdhlk/skills/releases/latest/download/pds-cli-installer.sh | sh
```

To pin a specific version, swap `latest` for the tag, e.g.
`releases/download/v0.1.0/pds-cli-installer.sh`. The installer places the `pds`
binary in your Cargo bin directory (`$CARGO_HOME/bin`, default `~/.cargo/bin`).

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
