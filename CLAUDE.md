# patdhlk-skills

Agent skills with sphinx-needs-backed issues, requirements, and ADRs. The
repo dogfoods its own system: backlog, decisions, and glossary live in
`spec/` as sphinx-needs directives.

## Agent skills

- Issue backend: **sphinx-needs** (local). Config and role map:
  `ubproject.toml` → `[tool.patdhlk-skills]`.
- Issues live in `spec/issues/index.rst` as `.. issue::` directives.
  `:status:` carries the triage state machine: `needs-triage` →
  `needs-info` | `ready-for-agent` | `ready-for-human` → `in-progress` →
  `done` | `wontfix`. Edit status in place; git history is the audit trail.
- ADRs: `spec/architecture/index.rst` (`.. arch-decision::`). Glossary:
  `spec/glossary.rst` (`.. term::`). Durable artifacts are RST, never
  markdown (ADR_0002); SKILL.md/README/CLAUDE.md are exempt.
- Query the corpus via needs.json, never by grepping RST: `make needs`,
  then `jq` on `spec/_build/needs/needs.json` (ADR_0006).
- New IDs: dense max+1 per prefix, from a fresh needs.json (ADR_0008).
- **Every spec mutation must end with the strict gate: `make strict`**
  (= `uv run sphinx-build -W -b html spec spec/_build/html`). Schema
  violations, broken links, duplicate IDs fail the gate (ADR_0007).
- Toolchain: `uv sync` once; everything runs through `uv run`. `ubc` is
  the preferred needs builder when on PATH.

## Layout

- `skills/<name>/SKILL.md` — the skills (flat; categories in README only).
- `spec/` — the dogfooded specification (Sphinx + sphinx-needs, furo).
- `ubproject.toml` — single source of truth: need types, links, role map.
- `.claude-plugin/plugin.json` — plugin manifest (`patdhlk-skills`).
