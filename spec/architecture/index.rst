Architecture Decisions
======================

The design of patdhlk-skills, decided in the founding grilling session
(2026-06-04) and subsequent grilling sessions, recorded here as
dogfooded artifacts.

.. arch-decision:: Fresh repo as drop-in replacement for mattpocock/skills
   :id: ADR_0001
   :status: accepted

   **Context.** mattpocock/skills is a proven set of agent workflow skills,
   but its durable artifacts (issues, ADRs, CONTEXT.md, PRDs) are markdown
   and its local issue backend is ``.scratch/`` markdown files.

   **Decision.** patdhlk-skills is a fresh repo that ports concepts
   selectively — not a fork, not an overlay. Skill names are kept
   (``/to-issues``, ``/to-prd``, ``/triage``, ``/qa``, ``/tdd``,
   ``/diagnose``; ``/grill-me`` ships unchanged) so the set is a drop-in
   replacement: uninstall mattpocock/skills where patdhlk-skills is
   installed.

   **Consequences.** ✅ Familiar vocabulary, 1:1 workflow mapping.
   ✅ No upstream coupling. ❌ Installing both plugins collides — the README
   must say so.

.. arch-decision:: Durable artifacts are sphinx-needs RST, never markdown
   :id: ADR_0002
   :status: accepted

   **Context.** Markdown artifacts (CONTEXT.md, ``docs/adr/*.md``) are easy
   to write but carry no IDs, no links, no schema, no traceability.

   **Decision.** All durable engineering artifacts in consumer repos —
   issues, requirements, features, architecture decisions, glossary terms,
   PRDs — are sphinx-needs directives in RST. Tool-mandated files stay
   markdown: SKILL.md (Claude Code requires it), CLAUDE.md, README, and
   GitHub issue bodies (platform format).

   **Consequences.** ✅ Everything durable is in the needs graph: linked,
   validated, queryable. ❌ Reading the corpus requires a needs.json build
   (:need:`ADR_0006`).

.. arch-decision:: Issue backend configurable via ubproject.toml
   :id: ADR_0003
   :status: accepted
   :refines: FEAT_0001

   **Context.** Issues may live on GitHub (public collaboration) or locally
   as sphinx-needs directives (solo/spec-driven repos). Skills need a
   deterministic way to discover which.

   **Decision.** A ``[tool.patdhlk-skills]`` table in the consumer repo's
   ``ubproject.toml`` declares ``issue_backend = "github" | "sphinx-needs"``,
   plus ``spec_dir``, ``builder``, and ``issue_doc``. Machine-readable, no
   markdown, next to the sphinx-needs config it parameterizes.

   **Consequences.** ✅ One lookup, no inference, no ambiguity. ❌ Requires a
   one-time setup step per repo (``/setup-patdhlk-skills``).

.. arch-decision:: Explicit role map instead of hardcoded directive names
   :id: ADR_0004
   :status: accepted
   :refines: FEAT_0004

   **Context.** Need-type catalogs are heavily user-dependent — taktora has
   17 types, the starter has 3. Skills cannot assume ``.. req::`` or
   ``.. arch-decision::`` exist.

   **Decision.** Setup reads the existing ``ubproject.toml``, proposes a
   mapping interactively, and persists it under
   ``[tool.patdhlk-skills.roles]`` (issue / feature / requirement /
   decision / term / test → directive name). Skills only ever look up
   roles. Setup never imposes a catalog; it only adds an issue type if the
   backend is local and none is mapped.

   **Consequences.** ✅ Deterministic at runtime, adapts to any catalog.
   ❌ A missing role needs one interactive question, then is persisted.

.. arch-decision:: Issue status carries the triage state machine
   :id: ADR_0005
   :status: accepted
   :refines: FEAT_0001

   **Context.** Matt's triage flow uses five GitHub labels; sphinx-needs has
   a first-class ``status`` field and schema validation.

   **Decision.** The ``:status:`` field carries the triage state directly:
   ``needs-triage → needs-info | ready-for-agent | ready-for-human →
   in-progress → done | wontfix``, enforced via schemas.json. ``:kind:``
   classifies (bug / feature / improvement / chore / question). Status edits
   happen in place; git history is the audit trail — no separate changelog
   field. On the github backend the same state names are labels.

   **Consequences.** ✅ One field, one source of truth, filterable in
   needtables, schema-enforced transitions at build time. ❌ Local and
   GitHub state vocabulary must be kept in sync by the skills.

.. arch-decision:: needs.json is the read path, ubc the preferred builder
   :id: ADR_0006
   :status: accepted

   **Context.** Skills must query the corpus ("all needs-triage issues",
   "max ID per prefix"). Grepping RST reimplements sphinx-needs parsing and
   misses needextend/includes; a full Sphinx build is slow.

   **Decision.** Skills build needs.json and query it with jq. The
   useblocks ``ubc`` CLI (``ubc build needs``) is preferred — it is faster
   than ``sphinx-build``; ``uv run sphinx-build -b needs`` is the fallback.
   The choice is persisted as ``builder`` in ``[tool.patdhlk-skills]``.
   Skills rebuild before every query rather than caching — stale reads are
   worse than slow reads.

   **Consequences.** ✅ Canonical data: links resolved, no parsing
   fragility. ✅ ubc keeps queries fast. ❌ ubc is a proprietary binary,
   fetched per-machine (devcontainer installs it on create).

.. arch-decision:: Strict build gate after every mutation
   :id: ADR_0007
   :status: accepted

   **Context.** Schema rules (:need:`ADR_0005`), link integrity, and
   duplicate IDs are only checked at build time.

   **Decision.** Every skill mutation of the spec ends with a strict build
   (``uv run sphinx-build -W -b html``). A failed gate means the mutation is
   fixed before the skill reports success. CI runs the same gate on PRs.

   **Consequences.** ✅ The corpus is never left invalid. ✅ Duplicate IDs
   from parallel branches surface at merge. ❌ Seconds of build time per
   mutation.

.. arch-decision:: Dense max+1 ID allocation
   :id: ADR_0008
   :status: accepted

   **Context.** New needs require unique IDs; allocation must be derivable
   from the corpus without a coordination service.

   **Decision.** Take the highest numeric suffix for the prefix from a
   fresh needs.json build and add one. No gaps, no hashing. Collisions from
   parallel branches are caught by the strict gate (:need:`ADR_0007`) as
   duplicate-ID build errors and renumbered at merge time.

   **Consequences.** ✅ Simplest possible scheme, small IDs. ❌ No room for
   manual inserts between siblings; ordering carries no meaning.

.. arch-decision:: Two-way GitHub traceability by convention
   :id: ADR_0009
   :status: accepted
   :refines: FEAT_0001

   **Context.** With ``issue_backend = "github"``, requirements and ADRs
   still live as sphinx-needs directives — traceability must cross the
   GitHub/spec seam.

   **Decision.** GitHub issue bodies carry the need IDs they implement
   (greppable ``Implements: REQ_0500, FEAT_0051`` line); the need side
   carries a ``:github:`` extra field with the issue number. Skills maintain
   both ends on create/close. No build-time enforcement across the seam.

   **Consequences.** ✅ Both directions queryable (jq on needs.json, gh
   search on bodies). ❌ Convention, not enforced — drift is possible and
   only skill discipline prevents it.

.. arch-decision:: A PRD is a feat directive plus child reqs
   :id: ADR_0010
   :status: accepted
   :refines: FEAT_0001

   **Context.** Matt's ``/to-prd`` publishes a frozen markdown memo to the
   issue tracker. In a needs graph a PRD decomposes naturally.

   **Decision.** ``/to-prd`` produces one RST document per PRD: a single
   ``feat`` directive (motivation, scope, non-goals as prose) plus child
   ``req`` directives linked ``:satisfies:`` the feature. ``/to-issues``
   then slices reqs into issues linked back to the req IDs.

   **Consequences.** ✅ The PRD is a living spec inside the graph, not a
   frozen memo. ❌ No single "PRD artifact" with its own lifecycle.

.. arch-decision:: Standalone from pharaoh — shared conventions only
   :id: ADR_0011
   :status: accepted

   **Context.** The pharaoh plugin already drafts, reviews, and ID-allocates
   sphinx-needs artefacts, with heavy ASPICE/ISO-26262 process assumptions
   (``.pharaoh/`` project dir, workflows.yaml).

   **Decision.** patdhlk-skills implements its own lightweight RST writing
   and needs.json reading inline. No dependency in either direction.
   Conventions are kept compatible — same link types (satisfies / refines /
   implements / verifies) and ID style — so both suites can coexist on one
   repo.

   **Consequences.** ✅ Installable by anyone without pharaoh; lean repos
   stay lean. ❌ Some duplicated drafting logic between the two suites.

.. arch-decision:: Full dogfood — own spec, starter devcontainer
   :id: ADR_0012
   :status: accepted

   **Context.** The flagship repo should exercise its own local backend.

   **Decision.** This repo carries its own ``spec/`` (issues, ADRs, reqs as
   directives), its own ``[tool.patdhlk-skills]`` with
   ``issue_backend = "sphinx-needs"``, and the sphinx-needs-starter
   devcontainer (Python 3.12 + uv + graphviz/plantuml image, ubc fetched on
   create, ``uv sync`` post-create). The founding interview is recorded as
   these ADRs; the v1 skill ports are the first issues.

   **Consequences.** ✅ The repo is its own integration test and living
   example. ❌ Spec maintenance overhead on every design change — which is
   the point.

.. arch-decision:: Distribution via plugin manifest and skills.sh layout
   :id: ADR_0013
   :status: accepted

   **Context.** Two install channels exist with near-identical repo
   requirements: the Claude Code plugin marketplace
   (``.claude-plugin/plugin.json``) and the skills.sh CLI
   (``npx skills add patdhlk/skills``).

   **Decision.** Ship both from one structure: ``.claude-plugin/plugin.json``
   (plugin name ``patdhlk-skills``) and a flat ``skills/<name>/SKILL.md``
   layout. Categories live in the README only.

   **Consequences.** ✅ Two channels, near-zero extra cost. ❌ Layout is
   constrained by both conventions at once.

.. arch-decision:: pds — a Rust gate-and-query CLI
   :id: ADR_0014
   :status: accepted

   **Context.** The mechanized checks beyond the build-time schema —
   body lint, verdict checking, next-action queries, duplicate detection
   (:need:`ISSUE_0013`–:need:`ISSUE_0018`) — were first sketched as make
   targets plus ad-hoc jq. Skills and CI need them deterministic, fast,
   and identical across consumer repos. Decided in the grilling session
   of 2026-06-06.

   **Decision.** A Rust CLI, binary ``pds``, crate ``pds-cli``, living
   in this repo at ``cli/`` and versioned in lockstep with the skills.
   Scope: build orchestration (shells out to the configured builder),
   checks, and queries — verbs ``build``, ``check``, ``lint``,
   ``verdict-check``, ``next``, ``status``, ``search``, ``dedup``.
   Hard invariant: **pds never mutates the spec** — skills author, pds
   judges. Output is JSON-only on stdout (logs to stderr); exit codes
   are uniform: 0 = clean, 1 = violations found (read the JSON, fix the
   corpus), 2 = tool/config error (stop and escalate). Configuration
   lives in ``[tool.patdhlk-skills.*]`` sub-tables; an absent table
   means the check is off; config referencing undeclared types or
   rubrics is a hard error. v1 speaks the local backend only: ``next``
   and ``status`` on a github-backend repo exit 2 with the equivalent
   ``gh`` command, and their JSON shape is versioned so a github driver
   can land without skill-text changes. Retrieval for ``search`` and
   ``dedup`` is BM25 (in-memory, deterministic, offline) by default;
   neural embeddings come later behind a cargo feature and
   ``--engine embed`` with lazy model download. ``dedup`` exits 1 on
   hits at/above threshold (a pre-filing gate); ``search`` always
   exits 0. Distribution: prebuilt static binaries from GitHub Releases
   (``aarch64/x86_64-apple-darwin``, ``aarch64/x86_64-linux-musl``) via
   an ``install.sh``, with ``cargo install pds-cli`` as fallback;
   ``/setup-patdhlk-skills`` owns detection and install, as it does for
   ``ubc``.

   **Consequences.** ✅ One deterministic entry point for gates and
   queries; agents branch on exit codes instead of parsing prose.
   ✅ ``make strict`` becomes a thin alias for ``pds check``.
   ❌ The repo grows a Rust toolchain, CI matrix, and release pipeline.
   ❌ macOS Gatekeeper: distributed binaries need Developer ID signing
   and notarization (or a documented quarantine story) in the release
   pipeline.

.. arch-decision:: Verdicts are needs directives with derived IDs
   :id: ADR_0015
   :status: accepted

   **Context.** Review judgments (triage verdicts, req-quality reviews)
   must be machine-checkable so a gate can require them
   (:need:`ISSUE_0014`). JSON receipt files would need a second storage
   location, a second read path, and a carve-out from :need:`ADR_0002`.

   **Decision.** A verdict is a sphinx-needs directive (new ``verdict``
   type, mapped in the role map). One verdict per judged need, linked to
   it, with a derived stable ID ``VERDICT_<judged-id>`` — a documented
   exception to :need:`ADR_0008`; re-review edits it in place and git
   history is the audit trail (:need:`ADR_0005` philosophy). Verdicts
   are status-less: staleness is computed, never declared. Fields:
   ``:rubric:`` (which rubric was judged), ``:axes_failed:`` (failed
   axis names; pass is *derived* — true iff empty — so "pass with a
   failing axis" is unrepresentable), ``:fingerprint:``
   (``sha256:<first-16-hex>`` over the judged need's title + normalized
   body; option-field edits like status flips do not invalidate).
   Findings are body prose. Verdicts live in ``spec/verdicts/``, are
   excluded from default needtables, and are themselves exempt from
   lint and from requiring verdicts. ``pds verdict-check`` reports four
   buckets: ``missing``, ``failing``, ``stale`` (fingerprint mismatch),
   ``malformed`` (schema-invalid or unknown axis names).

   **Consequences.** ✅ One read path (needs.json) for everything;
   verdict coverage is a graph query, visible in the docs. ✅ The
   malformed-pass class is impossible by construction. ❌ The corpus
   roughly doubles where verdicts are required. ❌ Two ID schemes
   coexist (dense max+1 and derived).

.. arch-decision:: Rubrics declared in config, semantics in skills
   :id: ADR_0016
   :status: accepted

   **Context.** ``pds`` must reject unknown axis names on verdicts
   (:need:`ADR_0015`), but what an axis *means* is judged by the AI
   review skills, and consumer repos tailor their type catalogs
   (:need:`ADR_0004`) — the binary cannot hardcode this repo's axes.

   **Decision.** Rubrics are config:
   ``[tool.patdhlk-skills.rubrics.<name>] axes = [...]`` declares the
   axis set; ``[tool.patdhlk-skills.verdicts] require = { <type> =
   "<rubric>" }`` maps need types to required rubrics. ``pds``
   validates names and structure only; axis semantics live in the
   review skills' prose, where the judge reads them. Default rubric
   tables are scaffolded by ``/setup-patdhlk-skills`` — defaults live
   in the plugin, not the binary. A ``require`` entry naming an
   undeclared rubric is a config hard error (exit 2).

   **Consequences.** ✅ The binary stays generic; rubric changes need
   no Rust release. ❌ Axis meaning exists only in skill prose —
   rubric/skill naming discipline is convention, not enforced.

.. arch-decision:: The gate builds needs, not html
   :id: ADR_0017
   :status: accepted

   **Context.** The strict gate (:need:`ADR_0007`) was
   ``sphinx-build -W -b html`` plus a separate needs.json build — two
   builds per mutation, and the gating diagnostics (unknown directives,
   broken refs, schema violations, duplicate IDs) come from
   parsing/resolution, not HTML rendering.

   **Decision.** ``pds check`` runs a per-builder adapter with two
   obligations: produce a fresh needs.json and run strict fail-closed
   diagnostics. For ``builder = "ubc"``: ``ubc check`` + ``ubc build
   needs``. For ``builder = "sphinx-build"``: ``uv run sphinx-build -W
   -b needs`` (one build serves both obligations), with a config escape
   hatch for projects without ``uv``. The html build is demoted to a
   docs-publishing step in CI and is no longer the per-mutation gate.
   A divergence (needs-build green, html red) is a ``pds`` bug to fix,
   not a reason to gate on html again.

   **Consequences.** ✅ One fast build per mutation instead of two.
   ❌ Rendering-only warnings (toctree gaps, malformed inline markup in
   prose) surface only at publish time.
