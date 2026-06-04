# Recipe for the published dev container base image:
#   ghcr.io/patdhlk/sphinx-needs-starter
#
# This image is referenced by devcontainer.json ("image": ...). It is NOT built
# on each customer's machine — they pull it. To rebuild and publish a new
# version, see the "Publishing the base image" section in the README.
#
# Note: the ubc CLI is deliberately NOT baked in (it is useblocks' proprietary
# binary). The dev container fetches it on create via install-ubc.sh.
FROM mcr.microsoft.com/devcontainers/python:3.12-bookworm

LABEL org.opencontainers.image.source="https://github.com/patdhlk/sphinx-needs-starter"
LABEL org.opencontainers.image.description="Sphinx + Sphinx-Needs build toolchain (uv, Graphviz, PlantUML, Java) — dev container base image"
LABEL org.opencontainers.image.licenses="MIT"

# Diagram toolchain used during the Sphinx build:
#   - graphviz provides `dot` (sphinx.ext.graphviz, and PlantUML's layout engine)
#   - plantuml provides the `plantuml` wrapper and pulls in a headless JRE
# (Mermaid renders client-side in the browser, so it needs no system package.)
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        graphviz \
        plantuml \
    && rm -rf /var/lib/apt/lists/*

# Bring in uv/uvx from Astral's first-party distroless image (no third-party
# devcontainer Feature). Pinned for reproducibility.
COPY --from=ghcr.io/astral-sh/uv:0.8.19 /uv /uvx /bin/
