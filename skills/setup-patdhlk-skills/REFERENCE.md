# Templates for /setup-patdhlk-skills

Adapt names/paths to the repo. `<spec>` = the chosen spec dir (default `spec`).

## §1 Spec skeleton (when no sphinx-needs project exists)

```
ubproject.toml          # repo root — single source of truth
pyproject.toml          # uv-managed, package = false
<spec>/conf.py
<spec>/schemas.json
<spec>/index.rst
<spec>/requirements/    # req directives (role: requirement)
<spec>/architecture/    # decision directives (role: decision)
<spec>/glossary.rst     # term directives (role: term)
<spec>/issues/          # only with the sphinx-needs issue backend (§3)
```

Toolchain: `uv init` (then set `[tool.uv] package = false`), `uv add sphinx
"sphinx-needs>=8,<9" furo sphinx-autobuild`, commit `uv.lock`.

`ubproject.toml` (minimal — extend, never replace, an existing one):

```toml
"$schema" = "https://ubcode.useblocks.com/ubproject.schema.json"

[project]
name = "<repo-name>"
srcdir = "<spec>"

[needs]
id_required = true

[[needs.types]]
directive = "req"
title = "Requirement"
prefix = "REQ_"
color = "#BFD8D2"
style = "node"

[[needs.types]]
directive = "arch-decision"
title = "Architecture Decision"
prefix = "ADR_"
color = "#A6C8E0"
style = "node"

[[needs.types]]
directive = "term"
title = "Glossary Term"
prefix = "GLOSS_"
color = "#D8D8D8"
style = "node"

[needs.links.satisfies]
incoming = "is satisfied by"
outgoing = "satisfies"

[needs.links.refines]
incoming = "is refined by"
outgoing = "refines"

[needs.links.implements]
incoming = "is implemented by"
outgoing = "implements"

[needs.links.verifies]
incoming = "is verified by"
outgoing = "verifies"
```

`<spec>/conf.py`:

```python
import json
from pathlib import Path

project = "<repo-name> — Specification"
extensions = ["sphinx_needs"]
exclude_patterns = ["_build", ".venv"]
source_suffix = {".rst": "restructuredtext"}

needs_from_toml = "../ubproject.toml"

needs_schema_validation_enabled = True
with (Path(__file__).parent / "schemas.json").open("r", encoding="utf-8") as _fh:
    needs_schema_definitions = json.load(_fh)

html_theme = "furo"
```

`<spec>/schemas.json` starts as `{"schemas": []}`; add ID-pattern rules per
type the repo uses (see patdhlk/skills `spec/schemas.json` for the shape).

## §2 The config block

Append to `ubproject.toml` (update keys in place on re-run):

```toml
[tool.patdhlk-skills]
issue_backend = "sphinx-needs"   # or "github"
spec_dir = "<spec>"
builder = "ubc"                  # or "sphinx-build"
issue_doc = "<spec>/issues/index.rst"   # sphinx-needs backend only
# features_dir = "<spec>/features"      # written by /to-prd on first use
# decisions_doc = "<spec>/architecture/index.rst"  # written by /decide on first use
# terms_doc = "<spec>/glossary.rst"        # written by /grill-with-docs on first use

[tool.patdhlk-skills.roles]
# only roles that resolved to a directive — never invent entries
issue = "issue"
feature = "feat"
requirement = "req"
decision = "arch-decision"
term = "term"
test = "test"
```

## §3 The issue type (sphinx-needs backend, on consent)

Type + fields for `ubproject.toml`:

```toml
[needs.fields.kind]
description = "Issue kind: bug | feature | improvement | chore | question"
nullable = true
schema = { type = "string", enum = ["bug", "feature", "improvement", "chore", "question"] }

[needs.fields.github]
description = "GitHub issue number this need is tracked by (github backend only)"
nullable = true
schema = { type = "string" }

[[needs.types]]
directive = "issue"
title = "Issue"
prefix = "ISSUE_"
color = "#E8C8A0"
style = "node"
```

Status state machine (ADR_0005) for `schemas.json` — append to `"schemas"`:

```json
{
  "id": "issue-status",
  "message": "issue :status: must be a triage state: needs-triage | needs-info | ready-for-agent | ready-for-human | in-progress | done | wontfix",
  "select": { "properties": { "type": { "const": "issue" } } },
  "validate": {
    "local": {
      "properties": {
        "status": {
          "enum": ["needs-triage", "needs-info", "ready-for-agent",
                   "ready-for-human", "in-progress", "done", "wontfix"]
        }
      },
      "required": ["status"]
    }
  }
}
```

`<spec>/issues/index.rst`:

```rst
Issues
======

.. needtable::
   :types: issue
   :columns: id;title;status;kind
   :style: table
```

Add `issues/index` to the root toctree.

## §4 ubc install (Linux)

```bash
curl -fsSL "https://download.useblocks.com/ubc/0.29.3/ubc-linux-$(uname -m | sed 's/x86_64/x64/;s/aarch64/arm64/')-0.29.3" \
  -o /usr/local/bin/ubc && chmod +x /usr/local/bin/ubc
```

(Or copy `install-ubc.sh` from github.com/patdhlk/sphinx-needs-starter —
the devcontainer in §5 runs it automatically.) macOS/Windows hosts: persist
`builder = "sphinx-build"`.

ubc usage note: `ubc build needs --outpath <file>` — the flag is `--outpath`
and the target directory must already exist.

## §5 Devcontainer

Copy from github.com/patdhlk/sphinx-needs-starter (`.devcontainer/`):
`devcontainer.json` (image `ghcr.io/patdhlk/sphinx-needs-starter:latest`,
`onCreateCommand` runs `install-ubc.sh`, `postCreateCommand` runs `uv sync`),
`Dockerfile`, `install-ubc.sh`. Adjust `esbonio.sphinx.confDir` to `<spec>`.

## §6 CI workflow

`.github/workflows/spec.yml`:

```yaml
name: spec
on:
  push:
    branches: [main]
  pull_request:
jobs:
  strict-build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: astral-sh/setup-uv@v5
      - run: uv sync
      - run: uv run sphinx-build -W -b html <spec> <spec>/_build/html
      - run: uv run sphinx-build -b needs <spec> <spec>/_build/needs
```

## §7 CLAUDE.md agent block

```markdown
## Agent skills (patdhlk-skills)

- Issue backend: **<backend>**. Config + role map: `ubproject.toml` →
  `[tool.patdhlk-skills]`. Resolve directives ONLY through the role map.
- Query the needs corpus via needs.json, never by grepping RST:
  `ubc build needs --outpath <spec>/_build/needs/needs.json` (or
  `uv run sphinx-build -b needs <spec> <spec>/_build/needs`), then `jq`.
  Rebuild before every query.
- New need IDs: dense max+1 per prefix, from a fresh needs.json.
- Every spec mutation must end with the strict gate:
  `uv run sphinx-build -W -b html <spec> <spec>/_build/html`.
- Issues (sphinx-needs backend) live in `<issue_doc>`; `:status:` carries
  the triage state machine: needs-triage → needs-info | ready-for-agent |
  ready-for-human → in-progress → done | wontfix. Edit status in place —
  git history is the audit trail.
- Issues (github backend): `gh` CLI; triage states are labels; issue bodies
  carry `Implements: <NEED-IDs>`; needs carry `:github:` back-references.
```

## §8 Makefile

```make
SOURCEDIR = <spec>
BUILDDIR  = <spec>/_build

.PHONY: html strict needs serve clean

html:  ## Build the HTML spec
	uv run sphinx-build -b html "$(SOURCEDIR)" "$(BUILDDIR)/html"

strict:  ## Strict build gate — every spec mutation must pass this
	uv run sphinx-build -W -b html "$(SOURCEDIR)" "$(BUILDDIR)/html"

needs:  ## Build needs.json for querying (ubc preferred)
	@mkdir -p "$(BUILDDIR)/needs"
	@if command -v ubc >/dev/null 2>&1; then \
		ubc build needs --outpath "$(BUILDDIR)/needs/needs.json"; \
	else \
		uv run sphinx-build -b needs "$(SOURCEDIR)" "$(BUILDDIR)/needs"; \
	fi

serve:  ## Live preview with auto-rebuild (port 8000)
	uv run sphinx-autobuild "$(SOURCEDIR)" "$(BUILDDIR)/html"

clean:  ## Remove build artifacts
	rm -rf "$(BUILDDIR)"
```

When appending to an existing Makefile, rename colliding targets with a
`spec-` prefix (`spec-html`, `spec-strict`, ...).
