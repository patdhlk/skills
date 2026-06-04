Glossary
========

Domain language for patdhlk-skills, as ``term`` directives — the
sphinx-needs replacement for a markdown CONTEXT.md (:need:`ADR_0002`).

.. term:: Issue backend
   :id: GLOSS_0001

   The store a repo's issues live in: ``github`` (GitHub Issues via ``gh``)
   or ``sphinx-needs`` (``issue`` directives in the spec). Declared in
   ``[tool.patdhlk-skills]`` in ``ubproject.toml`` (:need:`ADR_0003`).
   Requirements, ADRs, and glossary terms are always sphinx-needs,
   regardless of the issue backend.

.. term:: Role map
   :id: GLOSS_0002

   The persisted mapping from abstract skill roles (issue, feature,
   requirement, decision, term, test) to a repo's actual directive names,
   under ``[tool.patdhlk-skills.roles]``. Skills resolve directives only
   through the role map (:need:`ADR_0004`).

.. term:: Strict build gate
   :id: GLOSS_0003

   The ``sphinx-build -W`` run that ends every skill mutation of the spec.
   Schema violations, broken links, and duplicate IDs fail the gate; a
   mutation is not done until the gate passes (:need:`ADR_0007`).

.. term:: Triage state
   :id: GLOSS_0004

   The value of an issue's ``:status:`` field, drawn from the state machine
   ``needs-triage → needs-info | ready-for-agent | ready-for-human →
   in-progress → done | wontfix`` (:need:`ADR_0005`). On the github backend
   the same names appear as labels.

.. term:: Needs corpus
   :id: GLOSS_0005

   The full set of needs in a repo's spec, materialized as ``needs.json``
   by ``ubc build needs`` (preferred) or ``sphinx-build -b needs``
   (fallback) and queried with jq (:need:`ADR_0006`). Rebuilt before every
   query; never cached.

.. term:: Durable artifact
   :id: GLOSS_0006

   Any engineering artifact meant to outlive a conversation: issues,
   requirements, features, architecture decisions, glossary terms, PRDs.
   Durable artifacts are sphinx-needs directives in RST, never markdown
   (:need:`ADR_0002`).
