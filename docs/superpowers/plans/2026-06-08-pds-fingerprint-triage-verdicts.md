# pds fingerprint + /triage verdict authoring — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Implement ISSUE_0022 — a read-only `pds fingerprint <id>` verb (so a skill can compute the ADR_0015 fingerprint without re-implementing the normalization), then wire /triage to author/update a `triage` verdict whenever it routes an issue into a verdict-required status, fail-closed.

**Architecture:** `pds fingerprint` is one `run_fingerprint` orchestrator in `verdicts.rs` (reusing `load_fresh_corpus` + the existing `fingerprint()`), a clap variant, and e2e tests — the `status`/`next` query-verb shape. The /triage wiring is SKILL.md prose only.

**Tech Stack:** Rust (edition 2024), clap, serde_json; assert_cmd e2e. The skill change is Markdown.

**Authoritative spec:** the Agent Brief on ISSUE_0022 in `spec/issues/index.rst` (read it first), ADR_0015 (fingerprint/verdict shape), ADR_0016 (rubric semantics in skills).

**Pinned (from the grill):**
- `pds fingerprint <id>` prints `{schema, verb:"fingerprint", id, fingerprint}` using `pds_core::fingerprint` — never re-implement normalization. Unknown id → exit 2 (Error::Config naming the id). github backend → exit 2 with a `gh` hint. Absent need / build failure follow the query-verb conventions.
- The fingerprint MUST equal what `pds verdict-check` computes (same `fingerprint()` call).
- /triage authors/updates `VERDICT_<id>` only on entry to `ready-for-agent` / `ready-for-human` / `in-progress`; order = status edit → `pds fingerprint <id>` → write verdict → `make strict`.
- Four axes (category/state/actionability/duplicate-check) defined in prose; fail-closed (uncertain axis → `:axes_failed:` + body finding); quick-override authors a fail-closed verdict.
- Pickup contract is prose only; the gate enforces it. No `pds next` change.

**Cargo from `cli/`.**

---

### Task 1: `pds fingerprint <id>` verb

**Files:**
- Modify: `cli/pds-core/src/verdicts.rs`
- Modify: `cli/pds-core/src/lib.rs`
- Modify: `cli/pds-cli/src/main.rs`
- Modify: `cli/pds-cli/tests/cli.rs`

- [ ] **Step 1: Write the failing e2e tests**

Append to `cli/pds-cli/tests/cli.rs`. Reuse `backlog_project` + `FAKE_SPHINX_BACKLOG` (ISSUE_0001 "first ready" with content; the fixture issues have titles and some content). Add:

```rust
// ---------------------------------------------------------------------------
// `pds fingerprint <id>` E2E — verdict fingerprint surface (ISSUE_0022).
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn fingerprint_prints_the_need_fingerprint() {
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("fingerprint")
        .arg("ISSUE_0001")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "fingerprint");
    assert_eq!(json["id"], "ISSUE_0001");
    let fp = json["fingerprint"].as_str().expect("fingerprint string");
    assert!(fp.starts_with("sha256:"), "got: {fp}");
    assert_eq!(fp.len(), "sha256:".len() + 16);
}

#[cfg(unix)]
#[test]
fn fingerprint_matches_pds_core_for_the_same_need() {
    // The verb must agree with the library fingerprint() — the single source
    // of truth that verdict-check also uses. FAKE_SPHINX_BACKLOG's ISSUE_0001
    // has title "first ready" and links but (per the fixture) content "".
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("fingerprint")
        .arg("ISSUE_0001")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().get_output().clone();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();

    let expected = pds_core::fingerprint("first ready", "");
    assert_eq!(json["fingerprint"], expected);
}

#[cfg(unix)]
#[test]
fn fingerprint_unknown_id_is_config_error_naming_it() {
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("fingerprint")
        .arg("ISSUE_9999")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "fingerprint");
    assert_eq!(json["error"]["kind"], "config");
    assert!(
        json["error"]["message"].as_str().unwrap().contains("ISSUE_9999"),
        "error must name the missing id, got: {}",
        json["error"]["message"]
    );
}

#[cfg(unix)]
#[test]
fn fingerprint_github_backend_is_tool_error() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "");

    let assert = pds()
        .arg("fingerprint")
        .arg("ISSUE_0001")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "fingerprint");
    assert_eq!(json["error"]["kind"], "tool");
}

#[cfg(unix)]
#[test]
fn fingerprint_build_failure_surfaces_findings_under_fingerprint_verb() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, FAKE_SPHINX_FAIL);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds()
        .arg("fingerprint")
        .arg("ISSUE_0001")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "fingerprint");
    assert_eq!(json["findings"].as_array().unwrap()[0]["check"], "build");
}
```

NOTE on the second test: confirm FAKE_SPHINX_BACKLOG's ISSUE_0001 fields before asserting — if it has a `content` value, pass that exact string to `pds_core::fingerprint`. Read the fixture; do not guess. (`pds-core` is already a dev-dependency of pds-cli from ISSUE_0014.)

- [ ] **Step 2: Run to verify red**

`cargo test -p pds-cli fingerprint` — clap rejects the unknown `fingerprint` subcommand.

- [ ] **Step 3: Implement**

In `cli/pds-core/src/verdicts.rs`, add after `run_verdict_check` (the imports `Map`, `load_fresh_corpus`, `CorpusResult`, `Config`, `Error`, `Outcome`, `Path` are already present from Task 4/ISSUE_0014; add `use crate::queries::CorpusResult;` and `load_fresh_corpus` to the queries import if not already imported — check the existing `use crate::queries::...` line):

```rust
/// `pds fingerprint <id>`: print the ADR_0015 content fingerprint of one need
/// from a fresh corpus, so a skill can author a verdict without re-deriving
/// the normalization. Read-only; uses the same [`fingerprint`] the verdict
/// gate uses. Unknown id → [`Error::Config`] naming it (exit 2).
pub fn run_fingerprint(
    config: &Config,
    project_root: &Path,
    id: &str,
) -> Result<Outcome, Error> {
    let gh_hint = "gh issue view <id> --json title,body";
    let corpus = match load_fresh_corpus(config, project_root, gh_hint)? {
        CorpusResult::Ready(c) => c,
        CorpusResult::BuildFailed(failed) => return Ok(failed),
    };
    let need = corpus.get(id).ok_or_else(|| Error::Config {
        message: format!("no need with id {id:?} in the corpus"),
    })?;
    let fp = fingerprint(&need.title, &need.content);

    let mut payload = Map::new();
    payload.insert("id".to_string(), Value::String(need.id.clone()));
    payload.insert("fingerprint".to_string(), Value::String(fp));
    Ok(Outcome::clean(payload))
}
```

(If the `use crate::queries::{...}` line in verdicts.rs doesn't yet bring in `CorpusResult` and `load_fresh_corpus`, extend it. They are `pub(crate)` in queries.rs.)

`lib.rs`: add `run_fingerprint` to the verdicts re-export:
```rust
pub use verdicts::{Bucket, VerdictFinding, fingerprint, run_fingerprint, run_verdict_check, verdict_check_corpus};
```

`cli/pds-cli/src/main.rs`:
1. Variant after `VerdictCheck`:
```rust
    /// Print the verdict fingerprint of one need (for authoring verdicts).
    Fingerprint {
        /// The need id to fingerprint.
        id: String,
    },
```
2. `verb()` arm: `Commands::Fingerprint { .. } => "fingerprint",`
3. Dispatch arm: `Commands::Fingerprint { id } => pds_core::run_fingerprint(&config, &project.root, id),`

- [ ] **Step 4: Run to verify green**

`cargo test -p pds-cli && cargo test -p pds-core`; workspace clippy `-D warnings`; fmt.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/verdicts.rs cli/pds-core/src/lib.rs cli/pds-cli/src/main.rs cli/pds-cli/tests/cli.rs
git commit -m "feat(pds): pds fingerprint <id> — verdict fingerprint surface (ISSUE_0022)"
```

---

### Task 2: Wire /triage to author triage verdicts

**Files:**
- Modify: `skills/triage/SKILL.md` (repo copy)

- [ ] **Step 1: RED — confirm the contract is absent**

```bash
grep -c "pds fingerprint" skills/triage/SKILL.md      # expect 0
grep -ci "axes_failed\|fail-closed\|VERDICT_" skills/triage/SKILL.md  # expect 0
```

- [ ] **Step 2: GREEN — add the verdict-authoring sub-step**

Edit `skills/triage/SKILL.md`. In the **Apply transitions** step (step 3), add an authoring sub-step that fires only when routing into an in-scope status (`ready-for-agent` / `ready-for-human` / `in-progress`), on the sphinx-needs backend. The prose must specify:

- **Ordering**: apply the `:status:` edit first; then `pds fingerprint <id>` on the rebuilt corpus; then write/update `VERDICT_<id>` in `spec/verdicts/` with `:rubric: triage`, the computed `:fingerprint:`, `:axes_failed:` per the fail-closed rule, and per-axis reasoning as body prose; then `make strict`.
- **Author vs. update**: derived ID `VERDICT_<id>` is one slot — edit in place if it exists (re-triage / needs-info round-trip), git history is the audit trail.
- **The four axes** as defined in the ISSUE_0022 brief: `category` (one correct `:kind:`), `state` (routing target justified — agent-finishable for ready-for-agent), `actionability` (body self-contained to the agent-brief bar), `duplicate-check` (`pds search`/`dedup` run, top non-self hits judged not duplicate / not already-shipped).
- **Fail-closed rule**: any axis not affirmatively confirmed goes in `:axes_failed:` with a body finding sentence; a passing (empty) verdict asserts all four were cleared.
- **Quick-override path** (the existing "move #42 to ready-for-agent" skip-grill section): still authors a verdict, but fail-closed — unevaluated axes fail with "routed by maintainer override, not triage-judged".

Show a worked RST example of an authored verdict (mirror the `spec/verdicts/index.rst` tracer shape).

Also extend step 4 (**Validate and report**): the report names each ready issue's verdict (ID + pass/fail), with a one-line note that a green `make strict` guarantees a passing fresh verdict on every in-scope issue — that *is* the pickup contract, no `pds next` change.

Add one **Hard rule**: never author a passing verdict for an axis you did not affirmatively check — uncertain is a failing axis with a finding.

Keep /triage's terse imperative voice; touch only step 3, step 4, and the hard-rules list. Do NOT restructure the routing table or the state machine.

- [ ] **Step 3: GREEN — verify the contract landed**

```bash
grep -n "pds fingerprint" skills/triage/SKILL.md          # >=1, in step 3
grep -in "axes_failed" skills/triage/SKILL.md              # present
grep -in "fail-closed\|override, not triage-judged" skills/triage/SKILL.md  # both present
grep -n "VERDICT_" skills/triage/SKILL.md                  # the worked example
```

- [ ] **Step 4: Dogfood the verb against this repo**

```bash
cd cli && cargo run -q -p pds-cli -- fingerprint ISSUE_0022 --config ../ubproject.toml
```
Confirm it prints `sha256:ce93a76416230227` (the value already on `VERDICT_ISSUE_0022` — proving the verb agrees with what the gate computed during triage). If it differs, STOP and report — the verb and the gate disagree, which is a Task 1 bug.

- [ ] **Step 5: Gate and commit**

```bash
cd .. && make strict
git add skills/triage/SKILL.md
git commit -m "feat(triage): author fail-closed triage verdicts on routing to in-scope statuses (ISSUE_0022)"
```

`make strict` must stay exit 0 (no spec mutation here, but the gate guards the dogfood).

---

### Task 3: Docs + close

**Files:**
- Modify: `CLAUDE.md`
- Modify: `spec/issues/index.rst`

- [ ] **Step 1: CLAUDE.md — add the verb**

In the backlog-verbs bullet, after the `pds dedup` clause, add: `pds fingerprint "<id>"` = the ADR_0015 content fingerprint of one need (for authoring verdicts). Keep the sentence's style.

- [ ] **Step 2: Close the issue**

`spec/issues/index.rst`: ISSUE_0022 `:status: ready-for-agent` → `done`. This moves it out of the verdict scope; `VERDICT_ISSUE_0022` stays valid corpus content (exempt).

- [ ] **Step 3: Gate**

```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && make strict
```
All green. (After ISSUE_0022 → done it's exempt, so VERDICT_ISSUE_0022 is no longer demanded but remains; gate stays green.)

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md spec/issues/index.rst
git commit -m "docs(pds): document pds fingerprint; ISSUE_0022 -> done"
```

---

## Self-review notes
- **Coverage:** the verb (Task 1: shape, source-of-truth equality, unknown-id exit 2, github exit 2, build-failure passthrough), /triage wiring (Task 2: ordering, four axes, fail-closed, quick-override, pickup-contract prose), docs + close (Task 3). Out-of-scope respected: no /review (ISSUE_0023), no `pds next` change, no verdict-shape change.
- **Type consistency:** `run_fingerprint(config, project_root, id)`, payload keys `id`/`fingerprint`, verb name `"fingerprint"`, clap `Fingerprint { id }` — consistent across tasks.
- **Sequencing:** Task 1 must land before Task 2's dogfood (the verb must exist). Task 2's dogfood cross-checks the verb against the gate-computed `sha256:ce93a76416230227` already on `VERDICT_ISSUE_0022` — a real end-to-end agreement check.
