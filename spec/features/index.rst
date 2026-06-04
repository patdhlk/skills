Features
========

The four v1 skill groups. Every skill port in :doc:`../issues/index`
implements one of these features.

.. feat:: Issue-flow core
   :id: FEAT_0001
   :status: implemented

   The skills that read and write the issue store through the configurable
   backend (:need:`ADR_0003`): ``/to-prd``, ``/to-issues``, ``/triage``,
   ``/qa``. These are the reason the GitHub-vs-sphinx-needs backend
   abstraction exists.

.. feat:: Docs and decision flow
   :id: FEAT_0002
   :status: implemented

   The skills that write durable artifacts — ``arch-decision``, ``req``, and
   ``term`` directives — into a consumer repo's sphinx-needs spec instead of
   markdown ADRs and CONTEXT.md: ``/grill-me``, ``/grill-with-docs``,
   ``/decide``.

.. feat:: Dev-loop skills
   :id: FEAT_0003
   :status: open

   Largely backend-agnostic engineering workflow skills: ``/tdd``,
   ``/diagnose``, ``/review``, ``/prototype``. They reference the issue store
   for context but do not own it.

.. feat:: Setup and scaffolding
   :id: FEAT_0004
   :status: open

   ``/setup-patdhlk-skills`` configures a consumer repo: reads an existing
   ``ubproject.toml`` and adapts to it (never imposes a catalog), persists
   the role map (:need:`ADR_0004`), scaffolds a spec skeleton via uv where
   none exists, detects/installs ubc, offers the sphinx-needs-starter
   devcontainer, and appends the agent block to CLAUDE.md plus a strict-build
   CI workflow.
