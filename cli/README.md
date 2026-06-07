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
`releases/download/v0.2.0/pds-cli-installer.sh`. The installer places the `pds`
binary in your Cargo bin directory (`$CARGO_HOME/bin`, default `~/.cargo/bin`).

### macOS Gatekeeper

The recommended `curl … | sh` install path above is **quarantine-free by
construction**: the installer fetches the tarball with `curl`, and `curl` does
not set the `com.apple.quarantine` attribute. Gatekeeper only quarantines files
downloaded by a *browser* (Safari, Chrome, …), so the shell installer sidesteps
it entirely.

The macOS binaries are **ad-hoc signed**. On arm64 this happens automatically:
Apple's linker ad-hoc signs every arm64 Mach-O (`codesign -dv` reports
`flags=…(adhoc,linker-signed)`, `Signature=adhoc`), and that signature survives
the tarball round-trip. arm64 macOS *requires* a valid (even ad-hoc) signature
to run; x86_64 macOS does not require a signature at all. An unsigned,
un-quarantined x86_64 binary runs fine.

If you download the tarball (or a loose binary) with a **browser**, macOS
attaches a quarantine flag and Gatekeeper will block the first run. Clear it,
with your consent, before extracting:

```sh
# You downloaded this yourself and trust it; drop the browser quarantine flag.
xattr -d com.apple.quarantine ~/Downloads/pds-cli-aarch64-apple-darwin.tar.xz
```

(Equivalently, right-click the extracted `pds` in Finder and choose **Open**
once to approve it.) The ad-hoc signature is enough for Gatekeeper once the
quarantine flag is gone.

If you would rather not think about any of this, build locally — **locally
compiled binaries are never quarantined**:

```sh
cargo install pds-cli
```

> Developer ID code signing and Apple notarization are not yet configured (no
> certificates exist). They would let browser-downloaded binaries run without
> the `xattr` step. Where they plug in is documented in the
> [Releasing](#releasing) section below.

## Usage

```sh
# Run the full strict gate (build corpus + enforce rules).
pds check

# Build the corpus only (produces needs.json, exits 0/1/2).
pds build

# Lint need bodies for substance (required sections, weasel words, …).
# Absent [tool.patdhlk-skills.lint] table => clean exit 0 without building.
pds lint

# Per-status counts over the issue backlog (rebuilds first; exits 0/1/2).
pds status

# The next actionable (ready-for-agent) issue (rebuilds first; exits 0/1/2).
pds next

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

[tool.patdhlk-skills.gate]
# exempt_statuses = ["done", "wontfix"]  # default; skipped by lint and future checks

[tool.patdhlk-skills.lint.required_sections]
# Directive name must appear in [[needs.types]]; both **Section.** and **Section** accepted.
arch-decision = ["Context", "Decision", "Consequences"]

[tool.patdhlk-skills.lint.nontrivial_body]
# Minimum body length in characters (> 0); pair with max_body_length for a ceiling.
issue = 200
```

All lint rule keys (`required_sections`, `nontrivial_body`, `max_body_length`,
`weasel_words`, `unenumerated_quantifiers`) are optional — an absent key means
that rule is off. `pds check` runs lint automatically when the table is present.

## Releasing

Releases are tag-driven. The full runbook is one commit plus one tag push:

1. **Bump the version in lockstep.** In a single commit, set
   `workspace.package.version` in [`cli/Cargo.toml`](Cargo.toml) and
   `.version` in `.claude-plugin/plugin.json` to the same new value. These two
   must always match — the `version_lockstep` test
   ([`cli/pds-cli/tests/version_lockstep.rs`](pds-cli/tests/version_lockstep.rs))
   fails CI if they drift, and `release.yml` re-checks the tag against
   `plugin.json` before any build runs.
2. **Push the bump commit, then tag and push the tag:**

   ```sh
   git push origin main        # the version-bump commit
   # wait for the rust + spec CI checks on that commit to go green —
   # release.yml re-checks versions but does NOT re-run clippy/tests
   git tag v<version>          # e.g. git tag v0.2.0
   git push origin v<version>  # this push triggers the release
   ```

   The `v[0-9]+.[0-9]+.[0-9]+*` tag push triggers
   [`release.yml`](../.github/workflows/release.yml), which builds all four
   targets with `dist`, creates the GitHub Release (binaries + SHA-256 sums +
   `pds-cli-installer.sh`), then publishes to crates.io **core → cli**
   ([`publish-crates.yml`](../.github/workflows/publish-crates.yml) publishes
   `pds-core` first, waits for it to appear on the sparse index, then publishes
   `pds-cli`).

> **First release is `v0.2.0`.** The existing `v0.1.0` tag is historical — it
> points at the skills-v1 milestone commit and predates this release pipeline;
> do not reuse it.

**Required secret:** `CARGO_REGISTRY_TOKEN` must exist in the repository's
Actions secrets (Settings → Secrets and variables → Actions). `release.yml`
forwards it via `secrets: inherit`; `cargo publish` reads it. Without it the
crates.io publish job fails.

### Developer ID signing / notarization (future)

When Apple Developer ID certificates are available, macOS signing plugs into
`dist` via the `macos-sign` key in the `[dist]` table of `dist-workspace.toml`:

```toml
[dist]
macos-sign = true
```

With that enabled, `dist` (v0.32.0) runs `/usr/bin/codesign` against the
binaries in CI, importing the cert into a temporary keychain via
`/usr/bin/security`. It reads three repository secrets:
`CODESIGN_CERTIFICATE` (base64-encoded `.p12`), `CODESIGN_CERTIFICATE_PASSWORD`,
and `CODESIGN_IDENTITY` (the Developer ID identity string). That gives real
Developer ID signing (a Team Identifier instead of `adhoc`). **Notarization**
(stapling via Apple's `notarytool`) is *not* covered by `dist` v0.32.0 — it
would need a separate post-build CI step (`xcrun notarytool submit … --wait`
then `xcrun stapler staple`). Until then, the quarantine story above is the
supported path.

## Contributing

The project dogfoods its own spec: issues, requirements, and architectural
decisions live in `spec/` as sphinx-needs directives. See the repo root
[CLAUDE.md](../CLAUDE.md) for the workflow.
