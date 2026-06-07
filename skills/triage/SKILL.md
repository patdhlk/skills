---
name: triage
description: Drive issues through the triage state machine - route incoming issues to needs-info, ready-for-agent, ready-for-human, or wontfix, and keep states legal on both backends (sphinx-needs status edits or GitHub labels). Use when the user wants to triage issues, review the backlog, prepare issues for an agent, or says "triage" / "go through the open issues".
---

# Triage

One state machine, schema-enforced on the local backend (ADR_0005):

```
needs-triage ──► needs-info | ready-for-agent | ready-for-human | wontfix
needs-info ──► (answered) ──► needs-triage
ready-for-* ──► in-progress ──► done | wontfix
```

`/triage` owns the routing decisions; `in-progress`/`done` belong to whoever
does the work. Never jump states (e.g. `needs-triage → done`) — the enum
won't catch it, you must.

## Workflow

### 1. Resolve configuration and load the queue

Read `[tool.patdhlk-skills]` from `ubproject.toml` (missing → point to
`/setup-patdhlk-skills`).

- **sphinx-needs**: build a fresh needs.json (ADR_0006), queue = issues with
  `status == "needs-triage"`, plus `needs-info` ones whose question got
  answered since.
- **github**: `gh issue list --label needs-triage` **plus** open issues with
  no state label at all (treat unlabeled as needs-triage; offer to label
  them). Ensure the five state labels exist (`gh label create`, idempotent).

Empty queue → say so and stop.

### 2. Route each issue

Apply in order — first match wins:

1. **Duplicate / invalid / out of scope** → `wontfix`, with a one-line
   reason (and the duplicate's ID if any).
2. **Cannot be acted on as written** — missing repro, ambiguous scope, no
   way to verify → `needs-info`, writing the concrete question.
3. **An agent can finish it unattended** — clear acceptance criteria, no
   unresolved design decisions, no credentials or human taste required →
   `ready-for-agent`.
4. **Otherwise** → `ready-for-human` (say what the human must decide).

Propose the routing for the whole queue in one table (ID, title, verdict,
reason) and get the user's go-ahead — they may override any verdict —
**before** applying.

### 3. Apply transitions

- **sphinx-needs**: edit each `:status:` in place in the issue's RST —
  git history is the audit trail, no changelog field. For `needs-info`,
  append the question to the directive body under a ``**Needs info.**``
  lead-in. For `wontfix`, append the one-line reason the same way.
- **github**: swap the state label (remove old, add new). For `needs-info`
  and `wontfix`, post the question/reason as a comment; close `wontfix`
  issues as not-planned (`gh issue close --reason "not planned"`).

### 4. Validate and report

Local mutations end with the strict gate (ADR_0007, ADR_0017):

```bash
pds check
# no pds: uv run sphinx-build -W -b html <spec_dir> <spec_dir>/_build/html
```

Exit 1 means fix the corpus and re-run; exit 2 means stop and escalate.
Report counts per verdict and the `ready-for-agent` list — that's the
pickup queue for the next work session.

## Hard rules

- Only the transitions drawn above; `/triage` never sets `in-progress` or
  `done`.
- Every `needs-info` carries a concrete, answerable question; every
  `wontfix` carries a reason. No silent state changes.
- The user confirms the routing table before anything is applied.
- A failed strict gate is YOUR bug to fix before reporting success.
