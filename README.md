# patdhlk-skills

Agent workflow skills for Claude Code, with **sphinx-needs** as the backbone:
issues, requirements, features, architecture decisions, and glossary terms are
traceable `sphinx-needs` directives in RST — never markdown. Issues can
alternatively live on GitHub; everything else always lives in the spec.

A drop-in replacement for [mattpocock/skills](https://github.com/mattpocock/skills)
(whose concepts these skills port — thanks, Matt). **Do not install both** —
the skill names collide on purpose.

## Install

```bash
npx skills@latest add patdhlk/skills
```

or via the Claude Code plugin marketplace (`patdhlk-skills`).

## The model

- A consumer repo declares its setup in `ubproject.toml`:

  ```toml
  [tool.patdhlk-skills]
  issue_backend = "sphinx-needs"   # or "github"
  spec_dir = "spec"
  builder = "ubc"                  # or "sphinx-build"

  [tool.patdhlk-skills.roles]
  issue = "issue"
  requirement = "req"
  decision = "arch-decision"
  # ... maps abstract roles to YOUR directive names
  ```

- Skills **read** the corpus by building `needs.json` (`pds build`, which
  runs the configured builder — ubc preferred, sphinx-build fallback) and
  querying it with `jq`.
- Skills **write** by editing RST and finishing with the strict gate
  (`pds check`; raw `sphinx-build -W` when `pds` is missing) — schema
  violations, broken links, and duplicate IDs fail immediately.
- Run `/setup-patdhlk-skills` once per repo: it adapts to your existing
  `ubproject.toml` (never imposes a catalog), persists the role map, and can
  scaffold a spec skeleton (via [uv](https://docs.astral.sh/uv/)), the
  [sphinx-needs-starter](https://github.com/patdhlk/sphinx-needs-starter)
  devcontainer, and a strict-build CI workflow.

## Skills (v1)

| Group | Skills |
|---|---|
| Issue flow | `/to-prd`, `/to-issues`, `/triage`, `/qa` |
| Docs & decisions | `/grill-me`, `/grill-with-docs`, `/decide` |
| Dev loop | `/tdd`, `/diagnose`, `/review`, `/prototype` |
| Setup | `/setup-patdhlk-skills` |

Implementation status is tracked — of course — as `.. issue::` directives in
[`spec/issues/`](spec/issues/index.rst).

## This repo dogfoods itself

The design lives in [`spec/`](spec/): the founding decisions as
`arch-decision` directives, the backlog as `issue` directives, the domain
language as `term` directives. See `spec/architecture/index.rst` for the
ADRs that explain everything above.

```bash
uv sync          # set up the toolchain
make html        # build the spec
make strict      # the strict gate CI runs (= pds check)
make needs       # build needs.json (= pds build)
make serve       # live preview on :8000
```

## License

MIT
