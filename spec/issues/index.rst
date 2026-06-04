Issues
======

The dogfooded backlog (:need:`ADR_0012`): every v1 skill port is an
``issue`` directive, status-driven through the triage state machine
(:need:`ADR_0005`). IDs are allocated dense, max+1 (:need:`ADR_0008`).

.. issue:: Implement /setup-patdhlk-skills
   :id: ISSUE_0001
   :status: done
   :kind: feature
   :implements: FEAT_0004

   Build the setup skill. It must: read an existing ``ubproject.toml`` and
   propose a role map interactively (persisting it under
   ``[tool.patdhlk-skills.roles]``); scaffold a spec skeleton via
   ``uv init`` / ``uv add`` when no sphinx project exists; detect ``ubc`` on
   PATH and offer the install script when missing, persisting ``builder``;
   offer the sphinx-needs-starter devcontainer (opt-in); append an
   ``## Agent skills`` block to CLAUDE.md/AGENTS.md and offer a strict-build
   CI workflow.

.. issue:: Implement /to-prd
   :id: ISSUE_0002
   :status: done
   :kind: feature
   :implements: FEAT_0001

   Turn conversation context into one RST document per PRD: a single
   ``feat`` directive with motivation/scope/non-goals prose plus child
   ``req`` directives linked ``:satisfies:`` the feature (:need:`ADR_0010`).
   IDs allocated from a fresh needs.json build (:need:`ADR_0006`,
   :need:`ADR_0008`); mutation ends with the strict gate (:need:`ADR_0007`).

.. issue:: Implement /to-issues
   :id: ISSUE_0003
   :status: done
   :kind: feature
   :implements: FEAT_0001

   Slice a PRD's ``req`` directives into independently-grabbable issues.
   Backend sphinx-needs: write ``issue`` directives linked to the req IDs.
   Backend github: file via ``gh``, with ``Implements: REQ_xxxx`` in the
   body and ``:github:`` back-references on the needs (:need:`ADR_0009`).

.. issue:: Implement /triage
   :id: ISSUE_0004
   :status: done
   :kind: feature
   :implements: FEAT_0001

   Drive issues through the state machine
   ``needs-triage → needs-info | ready-for-agent | ready-for-human →
   in-progress → done | wontfix`` (:need:`ADR_0005`). Local backend: query
   needs.json for ``status == needs-triage``, edit ``:status:`` in place —
   git history is the audit trail. GitHub backend: the same states as
   labels.

.. issue:: Implement /qa
   :id: ISSUE_0005
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0001

   Conversational QA session that files issues from bug reports. Local
   backend: append ``issue`` directives (status ``needs-triage``) with
   duplicate detection against a fresh needs.json build.

.. issue:: Port /grill-me
   :id: ISSUE_0006
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0002

   Port unchanged — the skill is backend-agnostic and keeps its name and
   behavior (:need:`ADR_0001`).

.. issue:: Implement /grill-with-docs
   :id: ISSUE_0007
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0002

   Grilling session that challenges a plan against the domain model and
   updates documentation inline — but against the sphinx-needs spec:
   glossary ``term`` directives instead of CONTEXT.md, ``arch-decision``
   directives instead of markdown ADRs (:need:`ADR_0002`).

.. issue:: Implement /decide
   :id: ISSUE_0008
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0002

   Record a design decision as an ``arch-decision`` directive with Context /
   Decision / Consequences structure and links to affected needs. Standalone
   implementation — no pharaoh dependency (:need:`ADR_0011`).

.. issue:: Port /tdd
   :id: ISSUE_0009
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0003

   Red-green-refactor loop with vertical slices. Touchpoint: resolve issue
   references through the backend config when a task cites an ``ISSUE_`` ID
   or a GitHub issue number.

.. issue:: Port /diagnose
   :id: ISSUE_0010
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0003

   Disciplined diagnosis loop (reproduce → minimise → hypothesise →
   instrument → fix → regression-test). Backend touchpoint as in
   :need:`ISSUE_0009`.

.. issue:: Port /review
   :id: ISSUE_0011
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0003

   Review changes since a fixed point along Standards and Spec axes — the
   Spec axis reads the originating issue/req from the configured backend
   and its linked needs.

.. issue:: Port /prototype
   :id: ISSUE_0012
   :status: ready-for-agent
   :kind: feature
   :implements: FEAT_0003

   Throwaway prototypes to flesh out a design before committing. Largely
   unchanged; findings may feed ``/decide`` (:need:`ISSUE_0008`).
