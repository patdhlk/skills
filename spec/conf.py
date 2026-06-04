"""Sphinx configuration for the patdhlk-skills specification site."""

import json
from pathlib import Path

# -- Project information -------------------------------------------------------

project = "patdhlk-skills — Specification"
author = "Patrick Dahlke"
copyright = "2026, Patrick Dahlke"
release = "0.1.0"

# -- General configuration -----------------------------------------------------

extensions = [
    "sphinx_needs",
]

exclude_patterns = [
    "_build",
    "Thumbs.db",
    ".DS_Store",
    ".venv",
]

# Durable artifacts are RST only (ADR_0002) — no markdown source suffix.
source_suffix = {
    ".rst": "restructuredtext",
}

# -- sphinx-needs configuration -----------------------------------------------

# Read need types, fields, and link types from the repo-root ubproject.toml so
# tooling (ubc, the skills themselves) consumes them as data without parsing
# Python.
needs_from_toml = "../ubproject.toml"

# Schema validation — rules live in spec/schemas.json so they're editable as
# data. Violations are caught by `sphinx-build -W`, the strict gate every
# skill mutation must pass (ADR_0007).
needs_schema_validation_enabled = True
with (Path(__file__).parent / "schemas.json").open("r", encoding="utf-8") as _fh:
    needs_schema_definitions = json.load(_fh)

# -- HTML output ----------------------------------------------------------------

html_theme = "furo"
html_title = project
