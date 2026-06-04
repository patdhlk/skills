# Makefile for the patdhlk-skills spec.
# Commands run through `uv run` so the project's pinned tools are used.

SOURCEDIR = spec
BUILDDIR  = spec/_build

.PHONY: help html strict needs serve clean

.DEFAULT_GOAL := help

help:  ## List available targets
	@echo "Available targets:"
	@echo "  help    Show this help (default)"
	@echo "  html    Build the HTML spec into $(BUILDDIR)/html"
	@echo "  strict  Strict build gate: warnings are errors (ADR_0007)"
	@echo "  needs   Build needs.json (ubc preferred, sphinx-build fallback — ADR_0006)"
	@echo "  serve   Live preview with auto-rebuild on http://localhost:8000"
	@echo "  clean   Remove $(BUILDDIR)"

html:  ## Build the HTML spec
	uv run sphinx-build -b html "$(SOURCEDIR)" "$(BUILDDIR)/html"

strict:  ## Strict build gate — every mutation must pass this (ADR_0007)
	uv run sphinx-build -W -b html "$(SOURCEDIR)" "$(BUILDDIR)/html"

needs:  ## Build needs.json for querying (ADR_0006)
	@mkdir -p "$(BUILDDIR)/needs"
	@if command -v ubc >/dev/null 2>&1; then \
		ubc build needs --outpath "$(BUILDDIR)/needs/needs.json"; \
	else \
		uv run sphinx-build -b needs "$(SOURCEDIR)" "$(BUILDDIR)/needs"; \
	fi
	@echo "needs.json written under $(BUILDDIR)/needs/"

serve:  ## Live preview with auto-rebuild (port 8000)
	uv run sphinx-autobuild "$(SOURCEDIR)" "$(BUILDDIR)/html"

clean:  ## Remove build artifacts
	rm -rf "$(BUILDDIR)"
