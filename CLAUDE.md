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
- Query the corpus via needs.json, never by grepping RST: `make needs`
  (runs `pds build` under the hood), then `jq` on
  `spec/_build/needs/needs.json` (ADR_0006).
- Backlog queries have dedicated verbs (each rebuilds needs.json first):
  `pds status` = per-status issue counts; `pds next` = the lowest-ID
  `ready-for-agent` issue (`{"issue": null, "reason": "none-ready"}` when
  the backlog is clean). Ad-hoc reads stay `jq`.
- New IDs: dense max+1 per prefix, from a fresh needs.json (ADR_0008).
- **Every spec mutation must end with the strict gate: `make strict`**
  (= `pds check`, ADR_0017 — the per-builder gate that emits a fresh
  needs.json plus strict fail-closed diagnostics). Schema violations,
  broken links, duplicate IDs fail the gate (ADR_0007).
- Exit contract (ADR_0014/0019): `pds` prints one JSON object on stdout
  (`{"schema":1,"verb":...}`), builder/log noise on stderr; exit 0 =
  clean, 1 = violations (read the JSON findings, fix the corpus), 2 =
  tool/config error (stop and escalate). Branch on the exit code.
- Toolchain: `uv sync` once; spec build/query/gate run through `pds`
  (`ubc` is the preferred builder when on PATH, sphinx-build the
  fallback). Everything else runs through `uv run`.

## Layout

- `skills/<name>/SKILL.md` — the skills (flat; categories in README only).
- `spec/` — the dogfooded specification (Sphinx + sphinx-needs, furo).
- `ubproject.toml` — single source of truth: need types, links, role map.
- `.claude-plugin/plugin.json` — plugin manifest (`patdhlk-skills`).
