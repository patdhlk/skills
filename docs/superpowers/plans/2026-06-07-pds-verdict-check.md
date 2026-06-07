# pds verdict-check Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement ISSUE_0014 — verdict directives as gate-checkable judgments: the `verdict` need type, rubric/verdicts config, a `pds verdict-check` verb reporting four gating buckets (missing / failing / stale / malformed), flagless `pds check` integration, and a dogfooded tracer verdict `VERDICT_ISSUE_0014`.

**Architecture:** A new `verdicts` module in pds-core mirrors `lint.rs`: pure, lib-testable corpus check (`verdict_check_corpus`) + orchestration (`run_verdict_check`) on the shared fresh-build preamble pattern, findings through the canonical `outcome::finding` envelope. Config follows the raw→validated→fail-on-drift pattern. One new dependency: `sha2` (RustCrypto, pure Rust) for fingerprints.

**Tech Stack:** Rust (edition 2024), clap derive, serde_json, sha2; assert_cmd e2e with fake-builder scripts.

**Authoritative spec:** the Agent Brief on ISSUE_0014 in `spec/issues/index.rst`, plus ADR_0015 (verdict shape) and ADR_0016 (rubric config) in `spec/architecture/index.rst`. Read all three before starting. The brief supersedes the issue's older prose (no `--with-verdicts` flag).

**Working directory for cargo commands:** `cli/`.

**Pinned semantics (from the triage grill — implementers must not re-litigate):**
- Fingerprint (AMENDED during Task 1 review): `"sha256:" + first 16 lowercase hex` of SHA-256 over UTF-8 of `normalize(title) + "\n" + normalize(content)`, where `normalize` collapses whitespace runs to one space and trims, applied to each field SEPARATELY — the `\n` separator survives normalization, so a word migrating across the title/body boundary invalidates. RST reflow must not invalidate; any word change must. (Task 1's original joint-normalization text is superseded; Tasks 3-6 inherit the amended form.)
- All four buckets gate (exit 1), `check` field = `verdict:missing|failing|stale|malformed`, severity `error`.
- `need` field: judged need's ID for missing/failing/stale (verdict ID in the message); the verdict's own ID for malformed.
- Per-verdict precedence: malformed beats stale beats failing — one finding per verdict, no cascades.
- Requirement satisfied only by a well-typed need with ID `VERDICT_<judged-id>` AND `type == <verdict directive>` AND `rubric ==` the required rubric; a rubric mismatch is a `missing` finding whose message names the mismatch.
- Scoping: a verdict is demanded only when the judged need's type is in `verdicts.require`, its status is in `verdicts.statuses` (absent key = all), and its status is not in `gate.exempt_statuses`. Exempt-status needs are skipped for ALL buckets. Verdict-typed needs are skipped by lint and never require verdicts.
- `stale` messages MUST include the recomputed fingerprint (makes re-fingerprinting mechanical).
- Statusless needs: when `verdicts.statuses` is present, a need with no status is NOT in scope; when absent, it is.

---

### Task 1: Fingerprint module (sha2 dependency)

**Files:**
- Modify: `cli/pds-core/Cargo.toml`
- Create: `cli/pds-core/src/verdicts.rs`
- Modify: `cli/pds-core/src/lib.rs`

- [ ] **Step 1: Add the dependency**

In `cli/pds-core/Cargo.toml` `[dependencies]`, after `serde_json = "1"`:

```toml
sha2 = "0.10"
```

- [ ] **Step 2: Write the failing tests**

Create `cli/pds-core/src/verdicts.rs`:

```rust
//! Verdict directives and the `pds verdict-check` corpus check (ISSUE_0014).
//!
//! The shape contract is ADR_0015: one verdict per judged need, derived ID
//! `VERDICT_<judged-id>`, status-less, pass derived from an empty
//! `axes_failed`, staleness computed from a content fingerprint. Rubric
//! config is ADR_0016: the binary validates names and structure only.
//!
//! Mirrors `lint.rs`: [`verdict_check_corpus`] is pure and lib-testable;
//! [`run_verdict_check`] orchestrates build → load → check.

use sha2::{Digest, Sha256};

/// Content fingerprint for staleness detection (ADR_0015, pinned in the
/// ISSUE_0014 brief): `"sha256:" + first 16 lowercase hex` of SHA-256 over
/// the UTF-8 of `title + "\n" + content`, after collapsing every whitespace
/// run to a single space and trimming. Whitespace-only edits (RST reflow)
/// never invalidate; any word change does. Directive options (status, links)
/// never enter the hash.
pub fn fingerprint(title: &str, content: &str) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_has_pinned_shape() {
        let fp = fingerprint("a title", "a body");
        assert!(fp.starts_with("sha256:"), "got: {fp}");
        assert_eq!(fp.len(), "sha256:".len() + 16, "16 hex chars, got: {fp}");
        assert!(
            fp["sha256:".len()..].chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "lowercase hex only, got: {fp}"
        );
    }

    #[test]
    fn fingerprint_is_deterministic() {
        assert_eq!(fingerprint("t", "b"), fingerprint("t", "b"));
    }

    #[test]
    fn reflow_does_not_change_fingerprint() {
        // The exact cosmetic-edit class: RST line re-wrapping.
        let a = fingerprint("title", "one two three four");
        let b = fingerprint("title", "one two\n   three\n\tfour");
        assert_eq!(a, b, "whitespace runs must collapse identically");
    }

    #[test]
    fn word_change_changes_fingerprint() {
        assert_ne!(
            fingerprint("title", "one two three"),
            fingerprint("title", "one two four")
        );
    }

    #[test]
    fn case_change_changes_fingerprint() {
        assert_ne!(fingerprint("title", "body"), fingerprint("title", "Body"));
    }

    #[test]
    fn title_body_boundary_is_unambiguous() {
        // "ab" + "c" must differ from "a" + "bc" (the "\n" separator, which
        // then collapses to a space — but only AFTER joining).
        assert_ne!(fingerprint("ab", "c"), fingerprint("a", "bc"));
    }

    #[test]
    fn leading_trailing_whitespace_is_trimmed() {
        assert_eq!(
            fingerprint("title", "body"),
            fingerprint("  title", "body  ")
        );
    }
}
```

In `cli/pds-core/src/lib.rs`: add `pub mod verdicts;` to the module list (alphabetical, after `retrieval`), and `pub use verdicts::fingerprint;` at the end of the pub-use block.

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p pds-core verdicts` — expect `not yet implemented` panics (7 tests).

- [ ] **Step 4: Implement**

```rust
pub fn fingerprint(title: &str, content: &str) -> String {
    let joined = format!("{title}\n{content}");
    let normalized = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = Sha256::digest(normalized.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{}", &hex[..16])
}
```

(`split_whitespace` collapses runs and trims ends in one move — exactly the pinned normalization. Note the join happens BEFORE collapsing, so the `\n` separator becomes a space but the boundary survives as a token gap.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p pds-core verdicts` — 7 passed. Then `cargo clippy -p pds-core --all-targets -- -D warnings` and `cargo fmt` + `--check`.

- [ ] **Step 6: Commit**

```bash
git add cli/pds-core/Cargo.toml cli/Cargo.lock cli/pds-core/src/verdicts.rs cli/pds-core/src/lib.rs
git commit -m "feat(pds-core): verdict fingerprint — whitespace-collapsed sha256 prefix (ADR_0015)"
```

---

### Task 2: Rubrics and verdicts config

**Files:**
- Modify: `cli/pds-core/src/config.rs`
- Modify: `cli/pds-core/src/builder.rs` (test Config literal)
- Modify: `cli/pds-core/src/checker.rs` (test Config literal)
- Modify: `cli/pds-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests` in `cli/pds-core/src/config.rs`:

```rust
    // ------------------------------------------------------------------
    // rubrics + verdicts tables (ISSUE_0014 / ADR_0016)
    // ------------------------------------------------------------------

    #[test]
    fn absent_rubrics_and_verdicts_yield_empty_and_none() {
        let (_tmp, project) = make_project("[project]\nname = \"x\"");
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.rubrics.is_empty());
        assert!(cfg.verdicts.is_none());
    }

    #[test]
    fn full_verdicts_config_parses() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category", "state", "actionability"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
statuses = ["ready-for-agent", "in-progress"]
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(
            cfg.rubrics.get("triage").unwrap(),
            &vec!["category".to_string(), "state".to_string(), "actionability".to_string()]
        );
        let v = cfg.verdicts.as_ref().unwrap();
        assert_eq!(v.require.get("issue").map(String::as_str), Some("triage"));
        assert_eq!(
            v.statuses.as_ref().unwrap(),
            &vec!["ready-for-agent".to_string(), "in-progress".to_string()]
        );
    }

    #[test]
    fn verdicts_statuses_absent_is_none() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert!(cfg.verdicts.unwrap().statuses.is_none());
    }

    #[test]
    fn verdicts_require_undeclared_rubric_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.verdicts]
require = { issue = "review" }
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("review"), "must name the undeclared rubric, got: {msg}");
    }

    #[test]
    fn verdicts_require_undeclared_type_is_config_error() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { story = "triage" }
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("story"));
    }

    #[test]
    fn verdicts_without_require_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.verdicts]
statuses = ["in-progress"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("require"));
    }

    #[test]
    fn rubric_with_empty_axes_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.rubrics.triage]
axes = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("triage"));
    }

    #[test]
    fn rubric_with_duplicate_axes_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.rubrics.triage]
axes = ["category", "category"]
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("category"));
    }

    #[test]
    fn rubric_with_empty_string_axis_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.rubrics.triage]
axes = ["category", ""]
"#;
        let (_tmp, project) = make_project(toml);
        assert!(matches!(Config::load(&project).unwrap_err(), Error::Config { .. }));
    }

    #[test]
    fn verdicts_empty_statuses_list_is_config_error() {
        // An empty scope list would demand verdicts of nothing — config noise.
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
statuses = []
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("statuses"));
    }

    #[test]
    fn unknown_verdicts_keys_are_ignored() {
        let toml = r#"
[[needs.types]]
directive = "issue"

[tool.patdhlk-skills.rubrics.triage]
axes = ["category"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
future_key = 7
"#;
        let (_tmp, project) = make_project(toml);
        assert!(Config::load(&project).is_ok());
    }
```

- [ ] **Step 2: Run to verify red**

Run: `cargo test -p pds-core config` — compile errors (no `rubrics`/`verdicts` fields) are the red state.

- [ ] **Step 3: Implement**

In `cli/pds-core/src/config.rs`:

1. Public validated type (near `LintConfig`):

```rust
/// Validated `[tool.patdhlk-skills.verdicts]` table (ADR_0016 + the
/// ISSUE_0014 `statuses` scoping amendment).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictsConfig {
    /// Need type → required rubric name. Non-empty; every key is a declared
    /// directive, every value a declared rubric.
    pub require: HashMap<String, String>,
    /// Statuses in which a verdict is demanded. `None` = all non-exempt
    /// statuses (including statusless needs). Non-empty when `Some`.
    pub statuses: Option<Vec<String>>,
}
```

2. `Config` gains (after `dedup_threshold`):

```rust
    /// Declared rubrics: name → axis list (`[tool.patdhlk-skills.rubrics.<name>]`).
    /// Empty when no rubrics are declared. Axes are non-empty, unique strings.
    pub rubrics: HashMap<String, Vec<String>>,
    /// Verdict requirements (`None` when the table is absent — verdict-check
    /// is then a clean no-op, the lint activation model).
    pub verdicts: Option<VerdictsConfig>,
```

3. Raw types: `RawPatdhlkSkills` gains `rubrics: Option<HashMap<String, RawRubric>>,` and `verdicts: Option<RawVerdicts>,` (after `dedup`); plus:

```rust
/// Raw `[tool.patdhlk-skills.rubrics.<name>]` — unknown keys ignored.
#[derive(Deserialize)]
struct RawRubric {
    axes: Option<Vec<String>>,
}

/// Raw `[tool.patdhlk-skills.verdicts]` — unknown keys ignored.
#[derive(Deserialize, Default)]
struct RawVerdicts {
    require: Option<HashMap<String, String>>,
    statuses: Option<Vec<String>>,
}
```

4. In `from_raw`, destructure the two new fields, then after the dedup block:

```rust
        // rubrics tables
        let rubrics = validate_rubrics(raw_rubrics)?;

        // verdicts table
        let verdicts = raw_verdicts
            .map(|raw| validate_verdicts(raw, &declared_directives, &rubrics))
            .transpose()?;
```

and add `rubrics,` + `verdicts,` to `Ok(Config { ... })`.

5. Validators (Helpers section), following the lint validators' style:

```rust
/// Validate `[tool.patdhlk-skills.rubrics.*]`: every rubric needs a
/// non-empty `axes` list of non-empty, unique strings.
fn validate_rubrics(
    raw: Option<HashMap<String, RawRubric>>,
) -> Result<HashMap<String, Vec<String>>, Error> {
    let Some(raw) = raw else {
        return Ok(HashMap::new());
    };
    let mut rubrics: HashMap<String, Vec<String>> = HashMap::new();
    for (name, rubric) in raw {
        let axes = rubric.axes.unwrap_or_default();
        if axes.is_empty() {
            return Err(Error::Config {
                message: format!("rubrics.{name}: axes must be a non-empty list"),
            });
        }
        if let Some(pos) = axes.iter().position(|a| a.is_empty()) {
            return Err(Error::Config {
                message: format!(
                    "rubrics.{name}: axis at index {pos} is an empty string"
                ),
            });
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for axis in &axes {
            if !seen.insert(axis.as_str()) {
                return Err(Error::Config {
                    message: format!("rubrics.{name}: duplicate axis {axis:?}"),
                });
            }
        }
        rubrics.insert(name, axes);
    }
    Ok(rubrics)
}

/// Validate `[tool.patdhlk-skills.verdicts]` against declared directives and
/// declared rubrics (ADR_0016: a require entry naming an undeclared rubric is
/// a config hard error).
fn validate_verdicts(
    raw: RawVerdicts,
    declared_directives: &HashSet<String>,
    rubrics: &HashMap<String, Vec<String>>,
) -> Result<VerdictsConfig, Error> {
    let require = raw.require.unwrap_or_default();
    if require.is_empty() {
        return Err(Error::Config {
            message: "verdicts table requires a non-empty `require` map \
                      ({ <type> = \"<rubric>\" })"
                .to_string(),
        });
    }
    let mut entries: Vec<(&String, &String)> = require.iter().collect();
    entries.sort();
    for (need_type, rubric) in entries {
        if !declared_directives.contains(need_type) {
            return Err(Error::Config {
                message: format!(
                    "verdicts.require references undeclared [[needs.types]] \
                     directive {need_type:?}"
                ),
            });
        }
        if !rubrics.contains_key(rubric) {
            return Err(Error::Config {
                message: format!(
                    "verdicts.require[{need_type:?}] names undeclared rubric \
                     {rubric:?}; declare [tool.patdhlk-skills.rubrics.{rubric}]"
                ),
            });
        }
    }
    if let Some(ref statuses) = raw.statuses {
        if statuses.is_empty() {
            return Err(Error::Config {
                message: "verdicts.statuses must not be an empty list; \
                          remove the key to mean all non-exempt statuses"
                    .to_string(),
            });
        }
        if let Some(pos) = statuses.iter().position(|s| s.is_empty()) {
            return Err(Error::Config {
                message: format!(
                    "verdicts.statuses element at index {pos} is an empty string"
                ),
            });
        }
    }
    Ok(VerdictsConfig {
        require,
        statuses: raw.statuses,
    })
}
```

6. The two test-only `Config` literals in `builder.rs` and `checker.rs` gain `rubrics: HashMap::new(), verdicts: None,`.

7. `lib.rs`: add `VerdictsConfig` to the `pub use config::{...}` list.

- [ ] **Step 4: Run to verify green**

`cargo test -p pds-core` all green; clippy `-D warnings`; fmt.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/config.rs cli/pds-core/src/builder.rs cli/pds-core/src/checker.rs cli/pds-core/src/lib.rs
git commit -m "feat(pds-core): rubrics + verdicts config with fail-on-drift validation (ADR_0016)"
```

---

### Task 3: Verdict parsing and the four-bucket corpus check (pure)

**Files:**
- Modify: `cli/pds-core/src/verdicts.rs`
- Modify: `cli/pds-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Extend `cli/pds-core/src/verdicts.rs` imports:

```rust
use std::collections::HashMap;

use serde_json::Value;

use crate::config::VerdictsConfig;
use crate::needs::{Need, NeedsCorpus};
use crate::outcome::finding;
use sha2::{Digest, Sha256};
```

Add above the tests module:

```rust
/// The four verdict-check buckets (ADR_0015). All gate: any finding ⇒ exit 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Bucket {
    /// A required verdict does not exist (or exists with the wrong rubric).
    Missing,
    /// The verdict exists, is well-formed and fresh, and has failed axes.
    Failing,
    /// The verdict's fingerprint no longer matches the judged need's text.
    Stale,
    /// The verdict itself is broken: missing/invalid fields, unknown axis,
    /// undeclared rubric, bad ID shape, or an orphaned judged need.
    Malformed,
}

impl Bucket {
    /// The finding `check` value: `"verdict:<bucket>"`.
    pub fn check(self) -> &'static str {
        match self {
            Bucket::Missing => "verdict:missing",
            Bucket::Failing => "verdict:failing",
            Bucket::Stale => "verdict:stale",
            Bucket::Malformed => "verdict:malformed",
        }
    }
}

/// One verdict-check finding. `need` follows the act-on-it rule: the judged
/// need's ID for missing/failing/stale, the verdict's own ID for malformed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictFinding {
    pub bucket: Bucket,
    pub need: String,
    pub message: String,
}

/// Serialize one finding into the canonical envelope (ADR_0019).
pub fn finding_json(f: &VerdictFinding) -> Value {
    finding(f.bucket.check(), "error", Some(&f.need), &f.message)
}

/// The derived verdict ID for a judged need (ADR_0015).
fn verdict_id(judged_id: &str) -> String {
    format!("VERDICT_{judged_id}")
}

/// True when `need`'s status puts it in the verdict-required scope.
fn in_status_scope(need: &Need, statuses: Option<&Vec<String>>) -> bool {
    match statuses {
        None => true,
        Some(list) => need
            .status
            .as_deref()
            .is_some_and(|s| list.iter().any(|x| x == s)),
    }
}

/// Run the four-bucket verdict check over a corpus (pure; lib-testable).
///
/// `verdict_directive` is the directive the `verdict` role maps to.
/// `rubrics` is the declared rubric map. Exempt-status needs are skipped for
/// every bucket; verdict-typed needs never require verdicts. Per-verdict
/// precedence: malformed > stale > failing — exactly one finding per broken
/// verdict. Findings are deterministically ordered (need, bucket, message).
pub fn verdict_check_corpus(
    corpus: &NeedsCorpus,
    verdicts: &VerdictsConfig,
    rubrics: &HashMap<String, Vec<String>>,
    verdict_directive: &str,
    exempt_statuses: &[String],
) -> Vec<VerdictFinding> {
    todo!()
}
```

Add tests inside `mod tests` (reuse the `corpus_from` helper pattern from `queries.rs`/`retrieval.rs` — copy it in, plus a config helper):

```rust
    use std::collections::HashMap;

    use crate::config::VerdictsConfig;
    use crate::needs::NeedsCorpus;

    fn corpus_from(json: &str) -> NeedsCorpus {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        NeedsCorpus::load(f.path()).unwrap()
    }

    fn cfg(statuses: Option<Vec<&str>>) -> VerdictsConfig {
        VerdictsConfig {
            require: HashMap::from([("issue".to_string(), "triage".to_string())]),
            statuses: statuses.map(|v| v.into_iter().map(String::from).collect()),
        }
    }

    fn rubrics() -> HashMap<String, Vec<String>> {
        HashMap::from([(
            "triage".to_string(),
            vec!["category".to_string(), "state".to_string()],
        )])
    }

    const EXEMPT: &[String] = &[];

    fn exempt_done() -> Vec<String> {
        vec!["done".to_string(), "wontfix".to_string()]
    }

    /// A corpus with one in-scope issue and a configurable verdict object.
    /// `verdict_fields` is spliced into the verdict's JSON (or the verdict is
    /// omitted entirely when None).
    fn one_issue_corpus(issue_status: &str, verdict_fields: Option<&str>) -> NeedsCorpus {
        let verdict = match verdict_fields {
            Some(fields) => format!(
                r#","VERDICT_ISSUE_0001": {{"id":"VERDICT_ISSUE_0001","type":"verdict",
                     "title":"verdict for ISSUE_0001",{fields}}}"#
            ),
            None => String::new(),
        };
        let json = format!(
            r#"{{"current_version":"","project":"t","versions":{{"":{{"needs":{{
                "ISSUE_0001": {{"id":"ISSUE_0001","type":"issue","title":"the title",
                                "status":"{issue_status}","content":"the body"}}{verdict}
            }}}}}}}}"#
        );
        corpus_from(&json)
    }

    /// The correct fingerprint for the fixture issue ("the title" / "the body").
    fn fixture_fp() -> String {
        fingerprint("the title", "the body")
    }

    #[test]
    fn missing_verdict_is_flagged_on_the_judged_need() {
        let corpus = one_issue_corpus("ready-for-agent", None);
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Missing);
        assert_eq!(findings[0].need, "ISSUE_0001");
    }

    #[test]
    fn passing_fresh_verdict_yields_no_findings() {
        let fields = format!(
            r#""rubric":"triage","axes_failed":"","fingerprint":"{}""#,
            fixture_fp()
        );
        let corpus = one_issue_corpus("ready-for-agent", Some(&fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert!(findings.is_empty(), "got: {findings:?}");
    }

    #[test]
    fn failing_axes_are_flagged_on_the_judged_need() {
        let fields = format!(
            r#""rubric":"triage","axes_failed":"state","fingerprint":"{}""#,
            fixture_fp()
        );
        let corpus = one_issue_corpus("ready-for-agent", Some(&fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Failing);
        assert_eq!(findings[0].need, "ISSUE_0001");
        assert!(findings[0].message.contains("state"));
    }

    #[test]
    fn stale_fingerprint_is_flagged_and_message_names_the_recomputed_value() {
        let fields = r#""rubric":"triage","axes_failed":"","fingerprint":"sha256:0000000000000000""#;
        let corpus = one_issue_corpus("ready-for-agent", Some(fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Stale);
        assert_eq!(findings[0].need, "ISSUE_0001");
        assert!(
            findings[0].message.contains(&fixture_fp()),
            "stale message must carry the recomputed fingerprint, got: {}",
            findings[0].message
        );
    }

    #[test]
    fn stale_beats_failing_one_finding_only() {
        // Stale AND axes_failed: the axes refer to old text — report stale only.
        let fields = r#""rubric":"triage","axes_failed":"state","fingerprint":"sha256:0000000000000000""#;
        let corpus = one_issue_corpus("ready-for-agent", Some(fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Stale);
    }

    #[test]
    fn unknown_axis_is_malformed_on_the_verdict() {
        let fields = format!(
            r#""rubric":"triage","axes_failed":"vibes","fingerprint":"{}""#,
            fixture_fp()
        );
        let corpus = one_issue_corpus("ready-for-agent", Some(&fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Malformed);
        assert_eq!(findings[0].need, "VERDICT_ISSUE_0001");
        assert!(findings[0].message.contains("vibes"));
    }

    #[test]
    fn undeclared_rubric_is_malformed() {
        let fields = format!(
            r#""rubric":"nonsense","axes_failed":"","fingerprint":"{}""#,
            fixture_fp()
        );
        let corpus = one_issue_corpus("ready-for-agent", Some(&fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        // The verdict is malformed AND the requirement is unsatisfied (wrong
        // rubric ⇒ missing). Two findings, deterministic order by need id.
        assert_eq!(findings.len(), 2, "got: {findings:?}");
        assert!(findings.iter().any(|f| f.bucket == Bucket::Malformed
            && f.need == "VERDICT_ISSUE_0001"));
        assert!(findings.iter().any(|f| f.bucket == Bucket::Missing
            && f.need == "ISSUE_0001"
            && f.message.contains("nonsense")));
    }

    #[test]
    fn missing_fingerprint_field_is_malformed() {
        let fields = r#""rubric":"triage","axes_failed":"""#;
        let corpus = one_issue_corpus("ready-for-agent", Some(fields));
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert!(findings.iter().any(|f| f.bucket == Bucket::Malformed));
    }

    #[test]
    fn orphan_verdict_is_malformed() {
        let json = r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "VERDICT_ISSUE_0099": {"id":"VERDICT_ISSUE_0099","type":"verdict",
                "title":"v","rubric":"triage","axes_failed":"",
                "fingerprint":"sha256:0000000000000000"}
        }}}}"#;
        let corpus = corpus_from(json);
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Malformed);
        assert_eq!(findings[0].need, "VERDICT_ISSUE_0099");
        assert!(findings[0].message.contains("ISSUE_0099"));
    }

    #[test]
    fn bad_id_shape_is_malformed() {
        let json = r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "VERD_0001": {"id":"VERD_0001","type":"verdict","title":"v",
                "rubric":"triage","axes_failed":"",
                "fingerprint":"sha256:0000000000000000"}
        }}}}"#;
        let corpus = corpus_from(json);
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Malformed);
        assert_eq!(findings[0].need, "VERD_0001");
    }

    #[test]
    fn exempt_status_skips_all_buckets() {
        let corpus = one_issue_corpus("done", None);
        let findings = verdict_check_corpus(
            &corpus,
            &cfg(None),
            &rubrics(),
            "verdict",
            &exempt_done(),
        );
        assert!(findings.is_empty(), "done need must not be missing");
    }

    #[test]
    fn statuses_scope_excludes_out_of_scope_needs() {
        let corpus = one_issue_corpus("needs-triage", None);
        let findings = verdict_check_corpus(
            &corpus,
            &cfg(Some(vec!["ready-for-agent"])),
            &rubrics(),
            "verdict",
            EXEMPT,
        );
        assert!(findings.is_empty(), "needs-triage is out of scope");
    }

    #[test]
    fn statuses_scope_includes_in_scope_needs() {
        let corpus = one_issue_corpus("ready-for-agent", None);
        let findings = verdict_check_corpus(
            &corpus,
            &cfg(Some(vec!["ready-for-agent"])),
            &rubrics(),
            "verdict",
            EXEMPT,
        );
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Missing);
    }

    #[test]
    fn statusless_need_out_of_scope_when_statuses_present_in_scope_when_absent() {
        let json = r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"t","content":"b"}
        }}}}"#;
        let corpus = corpus_from(json);
        let scoped = verdict_check_corpus(
            &corpus,
            &cfg(Some(vec!["ready-for-agent"])),
            &rubrics(),
            "verdict",
            EXEMPT,
        );
        assert!(scoped.is_empty());
        let unscoped =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(unscoped.len(), 1);
        assert_eq!(unscoped[0].bucket, Bucket::Missing);
    }

    #[test]
    fn non_required_types_never_need_verdicts() {
        let json = r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "ADR_0001": {"id":"ADR_0001","type":"arch-decision","title":"t","content":"b"}
        }}}}"#;
        let corpus = corpus_from(json);
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert!(findings.is_empty());
    }

    #[test]
    fn finding_json_uses_the_canonical_envelope() {
        let f = VerdictFinding {
            bucket: Bucket::Missing,
            need: "ISSUE_0001".to_string(),
            message: "no verdict on file".to_string(),
        };
        let v = finding_json(&f);
        assert_eq!(v["check"], "verdict:missing");
        assert_eq!(v["severity"], "error");
        assert_eq!(v["need"], "ISSUE_0001");
        assert_eq!(v["message"], "no verdict on file");
    }
```

`lib.rs`: extend to `pub use verdicts::{Bucket, VerdictFinding, fingerprint, verdict_check_corpus};` (add `finding_json` only if it must be public — `run_verdict_check` in Task 4 and `checker.rs` in Task 5 need it crate-internally; mirror lint's choice, which exports `lint_corpus` and uses `finding_json` cross-module — make it `pub` like lint's).

- [ ] **Step 2: Run to verify red**

`cargo test -p pds-core verdicts` — the new tests fail on `todo!()`.

- [ ] **Step 3: Implement `verdict_check_corpus`**

```rust
pub fn verdict_check_corpus(
    corpus: &NeedsCorpus,
    verdicts: &VerdictsConfig,
    rubrics: &HashMap<String, Vec<String>>,
    verdict_directive: &str,
    exempt_statuses: &[String],
) -> Vec<VerdictFinding> {
    let mut findings: Vec<VerdictFinding> = Vec::new();

    let is_exempt = |need: &Need| {
        need.status
            .as_deref()
            .is_some_and(|s| exempt_statuses.iter().any(|x| x == s))
    };

    // Pass 1 — judge every verdict-typed need on its own merits:
    // malformed > stale > failing, one finding max per verdict.
    for vneed in corpus.iter().filter(|n| n.need_type == verdict_directive) {
        if is_exempt(vneed) {
            continue;
        }

        // ID shape + judged-need existence.
        let judged_id = vneed.id.strip_prefix("VERDICT_").map(str::to_string);
        let judged = judged_id
            .as_deref()
            .and_then(|id| corpus.iter().find(|n| n.id == id));

        let mut malformed: Vec<String> = Vec::new();
        if judged_id.is_none() {
            malformed.push(format!(
                "id {:?} does not match VERDICT_<judged-id>",
                vneed.id
            ));
        } else if judged.is_none() {
            malformed.push(format!(
                "judged need {:?} does not exist (orphan verdict)",
                judged_id.as_deref().unwrap_or_default()
            ));
        }

        // Fields from extras.
        let rubric = extra_str(vneed, "rubric");
        let fp = extra_str(vneed, "fingerprint");
        let axes_failed: Vec<String> = extra_str(vneed, "axes_failed")
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();

        let rubric_axes = match rubric.as_deref() {
            None => {
                malformed.push("missing :rubric: field".to_string());
                None
            }
            Some(name) => match rubrics.get(name) {
                None => {
                    malformed.push(format!("undeclared rubric {name:?}"));
                    None
                }
                Some(axes) => Some(axes),
            },
        };
        if let Some(axes) = rubric_axes {
            for axis in &axes_failed {
                if !axes.iter().any(|a| a == axis) {
                    malformed.push(format!(
                        "axis {axis:?} is not in rubric {:?}",
                        rubric.as_deref().unwrap_or_default()
                    ));
                }
            }
        }
        if fp.is_none() {
            malformed.push("missing :fingerprint: field".to_string());
        }

        if !malformed.is_empty() {
            findings.push(VerdictFinding {
                bucket: Bucket::Malformed,
                need: vneed.id.clone(),
                message: malformed.join("; "),
            });
            continue;
        }

        // Well-formed: staleness next (judged is Some here — orphans were
        // malformed above).
        let judged = judged.expect("checked above");
        let expected = fingerprint(&judged.title, &judged.content);
        if fp.as_deref() != Some(expected.as_str()) {
            findings.push(VerdictFinding {
                bucket: Bucket::Stale,
                need: judged.id.clone(),
                message: format!(
                    "verdict {} fingerprint {} does not match the judged \
                     need's current text (recomputed: {expected}); re-review \
                     and update the verdict",
                    vneed.id,
                    fp.as_deref().unwrap_or_default()
                ),
            });
            continue;
        }

        // Fresh: failing iff axes_failed non-empty.
        if !axes_failed.is_empty() {
            findings.push(VerdictFinding {
                bucket: Bucket::Failing,
                need: judged.id.clone(),
                message: format!(
                    "verdict {} fails axes: {}",
                    vneed.id,
                    axes_failed.join(", ")
                ),
            });
        }
    }

    // Pass 2 — missing: every required, in-scope, non-exempt need must have
    // a well-typed verdict with the required rubric.
    for need in corpus.iter() {
        if need.need_type == verdict_directive || is_exempt(need) {
            continue;
        }
        let Some(required_rubric) = verdicts.require.get(&need.need_type) else {
            continue;
        };
        if !in_status_scope(need, verdicts.statuses.as_ref()) {
            continue;
        }
        let vid = verdict_id(&need.id);
        let verdict = corpus
            .iter()
            .find(|n| n.id == vid && n.need_type == verdict_directive);
        match verdict {
            None => findings.push(VerdictFinding {
                bucket: Bucket::Missing,
                need: need.id.clone(),
                message: format!(
                    "no verdict on file: {} requires rubric {required_rubric:?} \
                     (expected {vid})",
                    need.need_type
                ),
            }),
            Some(v) => {
                let rubric = extra_str(v, "rubric").unwrap_or_default();
                if rubric != *required_rubric {
                    findings.push(VerdictFinding {
                        bucket: Bucket::Missing,
                        need: need.id.clone(),
                        message: format!(
                            "verdict {vid} has rubric {rubric:?} but \
                             {required_rubric:?} is required"
                        ),
                    });
                }
            }
        }
    }

    findings.sort_by(|a, b| {
        a.need
            .cmp(&b.need)
            .then_with(|| a.bucket.cmp(&b.bucket))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

/// A non-null string extra, `None` when absent or JSON null.
fn extra_str(need: &Need, key: &str) -> Option<String> {
    need.extras
        .get(key)
        .and_then(Value::as_str)
        .map(String::from)
}
```

- [ ] **Step 4: Run to verify green**

`cargo test -p pds-core verdicts` — all green (7 fingerprint + 16 new). Clippy, fmt.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/verdicts.rs cli/pds-core/src/lib.rs
git commit -m "feat(pds-core): four-bucket verdict corpus check with pinned precedence (ADR_0015)"
```

---

### Task 4: run_verdict_check orchestration + CLI verb

**Files:**
- Modify: `cli/pds-core/src/verdicts.rs`
- Modify: `cli/pds-core/src/lib.rs`
- Modify: `cli/pds-cli/src/main.rs`
- Modify: `cli/pds-cli/tests/cli.rs`

- [ ] **Step 1: Write the failing e2e tests**

Append to `cli/pds-cli/tests/cli.rs`. First a fixture + helper mirroring `lint_project` (note: `pds-cli` must gain `pds-core` as a dev-dependency so fixtures can compute correct fingerprints — add to `cli/pds-cli/Cargo.toml` `[dev-dependencies]`: `pds-core = { workspace = true }`):

```rust
// ---------------------------------------------------------------------------
// `pds verdict-check` E2E — verdict gate (unix-only: fake builders).
// ---------------------------------------------------------------------------

/// Write a fake sphinx-build whose needs.json contains one ready-for-agent
/// issue plus (optionally) a verdict need with the given fields JSON.
/// Returns (tempdir, config_path). The config declares issue+verdict types,
/// the verdict role, a triage rubric, and the require/statuses tables.
#[cfg(unix)]
fn verdict_project(verdict_fields: Option<&str>) -> (tempfile::TempDir, std::path::PathBuf) {
    let verdict = match verdict_fields {
        Some(fields) => format!(
            r#","VERDICT_ISSUE_0001": {{"id":"VERDICT_ISSUE_0001","type":"verdict","title":"v",{fields}}}"#
        ),
        None => String::new(),
    };
    let needs = format!(
        r#"{{"current_version":"","project":"t","versions":{{"":{{"needs":{{
            "ISSUE_0001": {{"id":"ISSUE_0001","type":"issue","title":"the title","status":"ready-for-agent","content":"the body"}}{verdict}
        }}}}}}}}"#
    );
    let script_body = format!(
        "#!/bin/sh\noutdir=\"$(eval echo \\${{$#}})\"\nmkdir -p \"$outdir\"\ncat > \"$outdir/needs.json\" <<'JSON'\n{needs}\nJSON\nexit 0\n"
    );

    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(&script, &script_body);
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[[needs.types]]\ndirective = \"issue\"\n\n\
         [[needs.types]]\ndirective = \"verdict\"\n\n\
         [tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n\n\
         [tool.patdhlk-skills.roles]\nissue = \"issue\"\nverdict = \"verdict\"\n\n\
         [tool.patdhlk-skills.rubrics.triage]\naxes = [\"category\", \"state\"]\n\n\
         [tool.patdhlk-skills.verdicts]\nrequire = {{ issue = \"triage\" }}\n\
         statuses = [\"ready-for-agent\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();
    (tmp, config)
}

#[cfg(unix)]
#[test]
fn verdict_check_missing_exits_one() {
    let (_tmp, config) = verdict_project(None);

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "verdict-check");
    let findings = json["findings"].as_array().expect("findings array");
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:missing");
    assert_eq!(findings[0]["need"], "ISSUE_0001");
}

#[cfg(unix)]
#[test]
fn verdict_check_passing_fresh_verdict_exits_zero() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "verdict-check");
    assert!(json["findings"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn verdict_check_stale_message_carries_recomputed_fingerprint() {
    let fields = r#""rubric":"triage","axes_failed":"","fingerprint":"sha256:0000000000000000""#;
    let (_tmp, config) = verdict_project(Some(fields));

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:stale");
    let expected = pds_core::fingerprint("the title", "the body");
    assert!(
        findings[0]["message"].as_str().unwrap().contains(&expected),
        "stale message must carry the recomputed fingerprint"
    );
}

#[cfg(unix)]
#[test]
fn verdict_check_malformed_names_the_verdict() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"vibes","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:malformed");
    assert_eq!(findings[0]["need"], "VERDICT_ISSUE_0001");
}

#[cfg(unix)]
#[test]
fn verdict_check_without_table_is_clean_and_does_not_build() {
    // No verdicts table: clean exit 0, and the builder must NOT run.
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let script = root.join("fake-sphinx.sh");
    write_script(
        &script,
        "#!/bin/sh\ntouch \"$(dirname \"$0\")/built.marker\"\nexit 0\n",
    );
    std::fs::create_dir_all(root.join("spec")).unwrap();
    let config = root.join("ubproject.toml");
    let toml = format!(
        "[tool.patdhlk-skills]\nbuilder = \"sphinx-build\"\nspec_dir = \"spec\"\n\n\
         [tool.patdhlk-skills.gate]\nsphinx_command = [\"{}\"]\n",
        script.display()
    );
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "verdict-check");
    assert!(json["findings"].as_array().unwrap().is_empty());
    assert!(
        !root.join("built.marker").exists(),
        "no verdicts table must mean no build"
    );
}

#[cfg(unix)]
#[test]
fn verdict_check_missing_verdict_role_is_config_error() {
    // verdicts table present but the role map has no `verdict` entry.
    let (_tmp, config) = verdict_project(None);
    let toml = std::fs::read_to_string(&config).unwrap();
    let toml = toml.replace("verdict = \"verdict\"\n", "");
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "verdict-check");
    assert_eq!(json["error"]["kind"], "config");
    assert!(json["error"]["message"].as_str().unwrap().contains("verdict"));
}

#[cfg(unix)]
#[test]
fn verdict_check_undeclared_rubric_in_require_is_config_error() {
    let (_tmp, config) = verdict_project(None);
    let toml = std::fs::read_to_string(&config).unwrap();
    let toml = toml.replace("require = { issue = \"triage\" }", "require = { issue = \"missing-rubric\" }");
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("verdict-check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["kind"], "config");
    assert!(json["error"]["message"].as_str().unwrap().contains("missing-rubric"));
}
```

- [ ] **Step 2: Run to verify red**

`cargo test -p pds-cli verdict` — clap rejects the unknown `verdict-check` subcommand.

- [ ] **Step 3: Implement**

In `cli/pds-cli/Cargo.toml` `[dev-dependencies]` add:

```toml
pds-core = { workspace = true }
```

In `cli/pds-core/src/verdicts.rs`, the orchestration (mirroring `run_lint`; imports gain `std::path::Path`, `serde_json::Map`, `crate::builder::run_build`, `crate::config::Config`, `crate::error::Error`, `crate::outcome::Outcome`):

```rust
/// Resolve the directive the `verdict` role maps to, or a config error.
/// Only called when the verdicts table is present.
pub(crate) fn verdict_directive(config: &Config) -> Result<&str, Error> {
    config
        .roles
        .get("verdict")
        .map(String::as_str)
        .ok_or_else(|| Error::Config {
            message: "role \"verdict\" is not defined in [tool.patdhlk-skills.roles]; \
                      `pds verdict-check` requires it when [tool.patdhlk-skills.verdicts] \
                      is configured"
                .to_string(),
        })
}

/// `pds verdict-check`: fresh build, then the four-bucket check. Absent
/// verdicts table ⇒ clean no-op without building (the lint activation model).
pub fn run_verdict_check(config: &Config, project_root: &Path) -> Result<Outcome, Error> {
    let Some(verdicts) = config.verdicts.as_ref() else {
        let mut payload = Map::new();
        payload.insert("findings".to_string(), Value::Array(Vec::new()));
        return Ok(Outcome::clean(payload));
    };
    let directive = verdict_directive(config)?.to_string();

    let build = run_build(config, project_root)?;
    if build.is_failed() {
        return Ok(build);
    }
    let corpus = NeedsCorpus::load(&config.needs_json)?;

    let findings = verdict_check_corpus(
        &corpus,
        verdicts,
        &config.rubrics,
        &directive,
        &config.exempt_statuses,
    );
    let arr: Vec<Value> = findings.iter().map(finding_json).collect();
    let failed = !arr.is_empty();
    let mut payload = Map::new();
    payload.insert("findings".to_string(), Value::Array(arr));
    payload.insert(
        "needs_json".to_string(),
        Value::String(config.needs_json.to_string_lossy().into_owned()),
    );
    if failed {
        Ok(Outcome::failed(payload))
    } else {
        Ok(Outcome::clean(payload))
    }
}
```

`lib.rs`: `pub use verdicts::{Bucket, VerdictFinding, fingerprint, run_verdict_check, verdict_check_corpus};` (plus `finding_json` if exported per Task 3's note — name-collides with lint's `finding_json` at the lib re-export level, so do NOT re-export either from lib.rs; `checker.rs` uses crate paths).

In `cli/pds-cli/src/main.rs`:
1. Variant after `Dedup`:

```rust
    /// Check verdict coverage: missing / failing / stale / malformed (exit 1 on any).
    VerdictCheck,
```

(clap derives the kebab-case name `verdict-check` automatically.)

2. `verb()` arm: `Commands::VerdictCheck => "verdict-check",`
3. Dispatch arm: `Commands::VerdictCheck => pds_core::run_verdict_check(&config, &project.root),`

- [ ] **Step 4: Run to verify green**

`cargo test -p pds-cli && cargo test -p pds-core`; clippy workspace-wide `-D warnings`; fmt.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/verdicts.rs cli/pds-core/src/lib.rs cli/pds-cli/src/main.rs cli/pds-cli/tests/cli.rs cli/pds-cli/Cargo.toml cli/Cargo.lock
git commit -m "feat(pds): pds verdict-check verb — flagless four-bucket gate (ISSUE_0014)"
```

---

### Task 5: pds check integration + lint skips verdict-typed needs

**Files:**
- Modify: `cli/pds-core/src/checker.rs`
- Modify: `cli/pds-core/src/lint.rs`
- Modify: `cli/pds-core/src/verdicts.rs` (if visibility tweaks needed)
- Modify: `cli/pds-cli/tests/cli.rs`

- [ ] **Step 1: Write the failing e2e tests**

Append to `cli/pds-cli/tests/cli.rs`:

```rust
#[cfg(unix)]
#[test]
fn check_with_verdicts_table_appends_verdict_findings() {
    // Clean builder, no lint table, missing verdict ⇒ pds check exits 1 with
    // the verdict finding in the one findings array.
    let (_tmp, config) = verdict_project(None);

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["verb"], "check");
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "verdict:missing");
}

#[cfg(unix)]
#[test]
fn check_clean_when_verdict_passes_and_fresh() {
    let fp = pds_core::fingerprint("the title", "the body");
    let fields = format!(r#""rubric":"triage","axes_failed":"","fingerprint":"{fp}""#);
    let (_tmp, config) = verdict_project(Some(&fields));

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["findings"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn check_builder_failure_skips_verdict_check() {
    // Failing builder ⇒ only the build finding; verdict-check must not run.
    let (_tmp, config) = verdict_project(None);
    // Point the config's sphinx_command at a failing script.
    let toml = std::fs::read_to_string(&config).unwrap();
    let root = std::path::Path::new(&config).parent().unwrap().to_path_buf();
    let fail = root.join("fail-sphinx.sh");
    write_script(&fail, FAKE_SPHINX_FAIL);
    let toml = {
        // Replace the sphinx_command line wholesale.
        let mut out = String::new();
        for line in toml.lines() {
            if line.starts_with("sphinx_command = ") {
                out.push_str(&format!("sphinx_command = [\"{}\"]\n", fail.display()));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        out
    };
    std::fs::write(&config, toml).unwrap();

    let assert = pds().arg("check").arg("--config").arg(&config).assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    let findings = json["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0]["check"], "build");
}
```

And one unit test in `cli/pds-core/src/lint.rs` `mod tests` (lint skipping verdict-typed needs):

```rust
    #[test]
    fn verdict_typed_needs_are_skipped_by_lint() {
        // A verdict need whose body would violate a rule targeting its
        // directive is still skipped: verdicts are exempt from lint
        // (ADR_0015), by type, not by configuration discipline.
        let corpus = corpus_from(
            r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "VERDICT_ISSUE_0001": {"id":"VERDICT_ISSUE_0001","type":"verdict",
                "title":"v","content":"All inputs shall be robust."}
        }}}}"#,
        );
        let lint = LintConfig {
            required_sections: None,
            body_length: None,
            weasel_words: Some(LintWeaselWords {
                words: vec!["robust".to_string()],
                directives: vec!["verdict".to_string()],
            }),
            unenumerated_quantifiers: None,
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt(), Some("verdict"));
        assert!(findings.is_empty(), "verdict needs must be lint-exempt");
    }
```

- [ ] **Step 2: Run to verify red**

The lint unit test fails to compile (`lint_corpus` has no 4th parameter); the check e2e tests fail (verdict findings not appended).

- [ ] **Step 3: Implement**

1. `cli/pds-core/src/lint.rs` — `lint_corpus` gains a 4th parameter:

```rust
pub fn lint_corpus(
    corpus: &NeedsCorpus,
    lint: &LintConfig,
    exempt_statuses: &[String],
    verdict_directive: Option<&str>,
) -> Vec<LintFinding> {
```

and the loop gains, right after the status-exemption `continue`:

```rust
        // Verdicts are exempt from lint by type (ADR_0015).
        if verdict_directive.is_some_and(|d| d == need.need_type) {
            continue;
        }
```

Update existing callers: `run_lint` passes `config.roles.get("verdict").map(String::as_str)`; every existing `lint_corpus(...)` call in `lint.rs` tests passes `None` (mechanical; ~15 call sites — use the same pattern the tests already use for `exempt_statuses`).

2. `cli/pds-core/src/checker.rs` — in `run_check`, after the lint block (line ~130), append:

```rust
    // Verdict-check runs under the same conditions as lint: clean builder
    // gate, table present (flagless — the lint activation model). Findings
    // append to the same array, after lint's.
    if findings.is_empty() && config.verdicts.is_some() {
        let directive = crate::verdicts::verdict_directive(config)?.to_string();
        let corpus = NeedsCorpus::load(&config.needs_json)?;
        let verdicts = config.verdicts.as_ref().expect("checked is_some");
        let vfindings = crate::verdicts::verdict_check_corpus(
            &corpus,
            verdicts,
            &config.rubrics,
            &directive,
            &config.exempt_statuses,
        );
        findings.extend(vfindings.iter().map(crate::verdicts::finding_json));
    }
```

NOTE the condition: `findings.is_empty()` means verdict-check is skipped when the builder OR lint already failed. That matches "builder failure skips both", and avoids judging a corpus that lint already flagged — but it differs from "lint findings and verdict findings both appear". DECISION (pinned): verdict-check runs when the BUILDER gate is clean, even if lint found findings — both check stages see the same fresh corpus. So track builder findings separately:

In `run_check`, change the lint/verdict section to:

```rust
    let builder_clean = findings.is_empty();
    if builder_clean && any_rule_enabled(config.lint.as_ref()) {
        let corpus = NeedsCorpus::load(&config.needs_json)?;
        let lint = config.lint.as_ref().expect("any_rule_enabled implies Some");
        let lint_findings = lint_corpus(
            &corpus,
            lint,
            &config.exempt_statuses,
            config.roles.get("verdict").map(String::as_str),
        );
        findings.extend(lint_findings.iter().map(finding_json));
    }
    if builder_clean && config.verdicts.is_some() {
        let directive = crate::verdicts::verdict_directive(config)?.to_string();
        let corpus = NeedsCorpus::load(&config.needs_json)?;
        let verdicts = config.verdicts.as_ref().expect("checked is_some");
        let vfindings = crate::verdicts::verdict_check_corpus(
            &corpus,
            verdicts,
            &config.rubrics,
            &directive,
            &config.exempt_statuses,
        );
        findings.extend(vfindings.iter().map(crate::verdicts::finding_json));
    }
```

(Loading the corpus twice is fine at this scale; refactor to a shared load only if clippy complains — it won't.)

3. Make `verdict_directive` and `finding_json` visible to `checker.rs` (`pub(crate)` on `verdict_directive` is already planned; `finding_json` is `pub` per Task 3).

- [ ] **Step 4: Run to verify green**

`cargo test -p pds-core && cargo test -p pds-cli`; clippy; fmt. Also re-run the full lint e2e suite — the `lint_corpus` signature change must not alter any existing behavior (all existing tests green unchanged).

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/checker.rs cli/pds-core/src/lint.rs cli/pds-core/src/verdicts.rs cli/pds-cli/tests/cli.rs
git commit -m "feat(pds): pds check runs verdict-check flaglessly; lint skips verdict needs"
```

---

### Task 6: Dogfood — verdict type, tracer verdict, docs, ADR amendment

**Files:**
- Modify: `ubproject.toml` (repo root)
- Create: `spec/verdicts/index.rst`
- Modify: `spec/index.rst`
- Modify: `spec/architecture/index.rst` (ADR_0016 amendment)
- Modify: `spec/issues/index.rst` (ISSUE_0014 → done)
- Modify: `CLAUDE.md`

- [ ] **Step 1: Declare the verdict type, fields, role, rubric, and requirements**

In `ubproject.toml`:

1. After the `[needs.fields.github]` table:

```toml
# Verdict fields (ADR_0015): rubric judged, failed axes (comma-separated,
# empty/absent = pass), and the judged need's content fingerprint.
[needs.fields.rubric]
description = "Rubric a verdict was judged against (verdict type only)"
nullable = true
schema = { type = "string" }

[needs.fields.axes_failed]
description = "Comma-separated failed axis names; empty or absent = pass"
nullable = true
schema = { type = "string" }

[needs.fields.fingerprint]
description = "sha256:<16hex> fingerprint of the judged need's title+body"
nullable = true
schema = { type = "string" }
```

2. After the `test` `[[needs.types]]` entry:

```toml
[[needs.types]]
directive = "verdict"
title = "Verdict"
prefix = "VERDICT_"
color = "#C8A2C8"
style = "node"
```

3. In `[tool.patdhlk-skills.roles]`: add `verdict = "verdict"`.

4. After the `[tool.patdhlk-skills.dedup]` table:

```toml
# Verdict gate (pds verdict-check, ADR_0015/ADR_0016): issues in the listed
# statuses must carry a passing, fresh triage verdict. Axis semantics live
# in the judging skills' prose (ISSUE_0022), not in the binary.
[tool.patdhlk-skills.rubrics.triage]
axes = ["category", "state", "actionability", "duplicate-check"]

[tool.patdhlk-skills.verdicts]
require = { issue = "triage" }
statuses = ["ready-for-agent", "ready-for-human", "in-progress"]
```

- [ ] **Step 2: Create spec/verdicts/index.rst with the tracer verdict**

```rst
Verdicts
========

Gate-checkable judgments (:need:`ADR_0015`): one ``verdict`` directive per
judged need, derived ID, status-less, staleness computed from a content
fingerprint by ``pds verdict-check``. Excluded from the status-overview
needtables by design.

.. verdict:: Triage verdict for ISSUE_0014
   :id: VERDICT_ISSUE_0014
   :links: ISSUE_0014
   :rubric: triage
   :fingerprint: sha256:PLACEHOLDER0000

   *This was generated by AI during triage.*

   Judged in the grilling session of 2026-06-07. **category**: feature,
   clear-cut. **state**: ready-for-agent — five design decisions resolved
   and recorded in the agent brief; normative shape pinned by
   :need:`ADR_0015` / :need:`ADR_0016`. **actionability**: acceptance
   criteria are independently verifiable; landing spots groomed
   (shared corpus-check plumbing, findings envelope).
   **duplicate-check**: ``pds search`` evidence ranked ADR_0015 (0.92)
   and no issue twin; the skill-wiring half was split to ISSUE_0022
   past the dedup gate.
```

(`:axes_failed:` is omitted entirely — absent = pass.)

Add `verdicts/index` to the `spec/index.rst` toctree after `architecture/index`.

- [ ] **Step 3: Compute the real fingerprint**

Run `make strict` from the repo root — expect exit 1 with a `verdict:stale` finding whose message carries the recomputed fingerprint for ISSUE_0014 (this is the designed re-fingerprinting flow). Replace `sha256:PLACEHOLDER0000` with the recomputed value from the message. Re-run `make strict` — expect exit 0.

NOTE: `ISSUE_0014`'s status flips to `done` in Step 5, which takes it OUT of the verdicts scope (and into `exempt_statuses`) — the tracer verdict then attests nothing the gate demands, but remains valid corpus content. Order matters: do Step 3 BEFORE Step 5 so the stale→fresh flow is actually exercised against a gate that demands it, and capture both runs' JSON in the task report. After Step 5, re-run `make strict` once more (expect exit 0 — fingerprint still matches; the issue edit is in the BODY, so recompute and update the verdict's `:fingerprint:` again as part of Step 5's edit. The status flip itself does NOT change the fingerprint — options never enter the hash — but Step 5 may not touch the body at all, in which case no update is needed).

- [ ] **Step 4: Docs**

1. `CLAUDE.md` — in the strict-gate bullet, extend the lint sentence:

```markdown
  When `[tool.patdhlk-skills.lint]` is configured, `pds check` also runs
  lint; findings carry `"check":"lint:<rule>"` and the offending need ID.
```

becomes:

```markdown
  When `[tool.patdhlk-skills.lint]` is configured, `pds check` also runs
  lint (findings `"check":"lint:<rule>"`); when
  `[tool.patdhlk-skills.verdicts]` is configured it also runs
  verdict-check (findings `"check":"verdict:<bucket>"` — missing /
  failing / stale / malformed, ADR_0015). `pds verdict-check` runs the
  verdict gate standalone.
```

2. `spec/architecture/index.rst` — ADR_0016 precision amendment: in the **Decision** paragraph, after the sentence ending `...maps need types to required rubrics.`, insert:

```rst
An optional ``statuses`` key on the same table scopes the demand: a
verdict is required only when the judged need's status is in the
list; absent means all non-exempt statuses (decided closing
ISSUE_0014's grill, 2026-06-07).
```

- [ ] **Step 5: Close the issue**

In `spec/issues/index.rst`, change ISSUE_0014's `:status: ready-for-agent` to `:status: done`. Re-run `make strict` — exit 0 (the status flip moves ISSUE_0014 out of scope; the tracer verdict stays valid).

- [ ] **Step 6: Stale-flip verification (acceptance criterion — verify once, then restore)**

1. Append a single word to ISSUE_0014's body (e.g. change one sentence), run `make strict` — expect... ISSUE_0014 is now `done` (exempt), so NO stale finding. To exercise the criterion against a gated need instead: temporarily set ISSUE_0014's status back to `ready-for-agent`, append a word to its body, run `make strict` → expect exit 1 with `verdict:stale` naming the recomputed fingerprint. Then revert both edits (status back to `done`, word removed), run `make strict` → exit 0. Capture the failing JSON in the report. Nothing from this step is committed except the final clean state.

- [ ] **Step 7: Full gates**

```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && make strict
```

All green.

- [ ] **Step 8: Commit**

```bash
git add ubproject.toml spec/verdicts/index.rst spec/index.rst spec/architecture/index.rst spec/issues/index.rst CLAUDE.md
git commit -m "feat(spec): dogfood verdict gate — tracer VERDICT_ISSUE_0014; ISSUE_0014 -> done"
```

---

## Self-review notes

- **Brief coverage:** config (Task 2), verdict shape + fingerprint (Tasks 1, 3), four buckets with pinned precedence and `need`-field mapping (Task 3), flagless check integration + lint exclusion (Task 5), standalone verb + missing-role error + drift exit-2 (Task 4), `statuses` scoping incl. statusless rule (Task 3 tests), exempt skip for all buckets (Task 3), stale message carries recomputed fingerprint (Tasks 3, 4), dogfood + ADR_0016 amendment + CLAUDE.md + stale-flip verify (Task 6). Out-of-scope respected: no skill wiring, no new flags, no severity levels.
- **Type consistency:** `VerdictsConfig { require, statuses }`, `verdict_check_corpus(corpus, verdicts, rubrics, verdict_directive, exempt_statuses)`, `fingerprint(title, content)`, `Bucket::check()`, `run_verdict_check(config, root)`, `lint_corpus(corpus, lint, exempt_statuses, verdict_directive)` — names match across tasks.
- **Known sequencing hazards:** Task 5 changes `lint_corpus`'s signature — all existing lint tests need the `None` argument (mechanical). Task 6 Step 3 deliberately uses the stale-finding message to obtain the real fingerprint (chicken-and-egg solved by design). The lib.rs re-export must NOT include both lint's and verdicts' `finding_json` (name collision) — neither is re-exported from lib.rs; `fingerprint` IS re-exported (the e2e fixtures need it via the new pds-core dev-dependency of pds-cli).
- **New dependency:** `sha2` in pds-core — the single deliberate departure from the zero-deps habit; hand-rolling SHA-256 is the wrong risk.
