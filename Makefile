# Makefile for the patdhlk-skills spec.
# Commands run through `uv run` so the project's pinned tools are used.
#
# The gate (strict/needs) runs through `pds`, the repo's own CLI. When `pds`
# is on PATH it is used directly; otherwise we fall back to `cargo run` against
# the in-tree crate (cli/), so the gate works for any contributor with Rust.

SOURCEDIR = spec
BUILDDIR  = spec/_build

# `pds` from PATH if present, else a quiet `cargo run` against the in-tree crate.
PDS = $(shell command -v pds 2>/dev/null || echo "cargo run -q --manifest-path cli/Cargo.toml -p pds-cli --")

.PHONY: help html strict needs serve clean

.DEFAULT_GOAL := help

help:  ## List available targets
	@echo "Available targets:"
	@echo "  help    Show this help (default)"
	@echo "  html    Build the HTML docs into $(BUILDDIR)/html (NOT the gate)"
	@echo "  strict  Strict gate via 'pds check': needs build + strict diagnostics (ADR_0007, ADR_0017)"
	@echo "  needs   Build a fresh needs.json via 'pds build' (ADR_0006)"
	@echo "  serve   Live preview with auto-rebuild on http://localhost:8000"
	@echo "  clean   Remove $(BUILDDIR)"

html:  ## Build the HTML docs (NOT the gate — ADR_0017 demotes html to docs-only)
	uv run sphinx-build -b html "$(SOURCEDIR)" "$(BUILDDIR)/html"

strict:  ## Strict gate — every mutation must pass this (ADR_0007, ADR_0017)
	# ADR_0017: the gate builds needs, not html. `pds check` runs the
	# builder's strict diagnostics and emits a fresh needs.json; html is
	# no longer gating (see the `html` target / the `docs` CI job).
	$(PDS) check

needs:  ## Build a fresh needs.json for querying (ADR_0006)
	# `pds build` owns the per-builder adapter (ubc preferred, sphinx
	# fallback) — no hand-rolled branching here anymore.
	$(PDS) build

serve:  ## Live preview with auto-rebuild (port 8000)
	uv run sphinx-autobuild "$(SOURCEDIR)" "$(BUILDDIR)/html"

clean:  ## Remove build artifacts
	rm -rf "$(BUILDDIR)"
