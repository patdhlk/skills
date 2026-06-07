//! Deterministic backlog queries over an issue corpus for `pds status` and
//! `pds next`.
//!
//! Both verbs answer from a *fresh* corpus (ADR_0006: rebuild before every
//! query — stale reads are worse than slow reads). The orchestration functions
//! ([`run_status`], [`run_next`]) guard the configured issue backend, run the
//! non-gating build, load the resulting `needs.json`, and run a pure query over
//! the in-memory corpus.
//!
//! The pure query functions ([`status_counts`], [`next_issue`]) take a
//! `&NeedsCorpus` plus the resolved issue-role directive string and return
//! plain data, so they are lib-testable without spawning a builder.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::builder::run_build;
use crate::config::{Config, IssueBackend};
use crate::error::Error;
use crate::needs::{Need, NeedsCorpus};
use crate::outcome::Outcome;

/// The role key, looked up in `config.roles`, whose mapped directive identifies
/// issue-typed needs. Part of the role map contract, not the state machine.
const ISSUE_ROLE: &str = "issue";

/// The triage status a need must carry to be the "next" actionable issue.
/// Part of the skills' triage state machine (ADR_0005).
const READY_FOR_AGENT: &str = "ready-for-agent";

/// The status key used for issue-typed needs that carry no `status`.
///
/// **Latent constraint (ADR_0005):** this name must never equal any real triage
/// state (`needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`,
/// `in-progress`, `done`, `wontfix`). If the triage state machine gains a new
/// state, verify it does not collide with `"none"`.
const NO_STATUS: &str = "none";

// ---------------------------------------------------------------------------
// Pure queries (lib-testable over a corpus)
// ---------------------------------------------------------------------------

/// Per-status counts over the issue-typed needs in `corpus`.
///
/// `issue_directive` is the directive string identifying issue-typed needs
/// (the value `config.roles["issue"]` maps to). Needs whose `need_type` does
/// not equal `issue_directive` are excluded. Needs with no status are counted
/// under the [`NO_STATUS`] key. The returned map is a `BTreeMap`, so key order
/// is deterministic.
pub fn status_counts(corpus: &NeedsCorpus, issue_directive: &str) -> BTreeMap<String, usize> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for need in corpus.iter().filter(|n| n.need_type == issue_directive) {
        let key = need.status.as_deref().unwrap_or(NO_STATUS);
        *counts.entry(key.to_string()).or_insert(0) += 1;
    }
    counts
}

/// The next actionable issue: the lowest-id issue-typed need whose status is
/// [`READY_FOR_AGENT`], or `None` when the backlog has none ready.
///
/// `issue_directive` is as in [`status_counts`]. The corpus iterates id-sorted,
/// so the first match is the lowest id.
pub fn next_issue<'a>(corpus: &'a NeedsCorpus, issue_directive: &str) -> Option<&'a Need> {
    corpus
        .iter()
        .find(|n| n.need_type == issue_directive && n.status.as_deref() == Some(READY_FOR_AGENT))
}

/// Assemble the JSON payload for a `pds next` response given the matched need.
///
/// Returns a map with `"issue"` (the need's id/title/status/links) and
/// `"reason": null` — the null-reason shape for a found issue.
pub(crate) fn next_payload(need: &Need) -> Map<String, Value> {
    let mut payload = Map::new();
    payload.insert(
        "issue".to_string(),
        json!({
            "id": need.id,
            "title": need.title,
            "status": need.status,
            "links": need.links,
        }),
    );
    payload.insert("reason".to_string(), Value::Null);
    payload
}

// ---------------------------------------------------------------------------
// Internal shared orchestration
// ---------------------------------------------------------------------------

/// The result of the shared build+load step: either a ready corpus or a
/// pre-formed failed [`Outcome`] to return directly to the caller.
enum CorpusResult {
    Ready(NeedsCorpus),
    BuildFailed(Outcome),
}

/// Reject the GitHub backend before doing any work, naming the equivalent `gh`
/// command for the invoked verb. pds v1 has no github driver (forward-compatible
/// JSON shapes let one land later without changing the envelope).
fn guard_backend(config: &Config, gh_hint: &str) -> Result<(), Error> {
    if config.issue_backend == IssueBackend::Github {
        return Err(Error::Tool {
            message: format!(
                "issue_backend \"github\" is not supported by pds v1 — run: {gh_hint}"
            ),
        });
    }
    Ok(())
}

/// Resolve the directive string that identifies issue-typed needs, or an
/// [`Error::Config`] naming the missing role when the role map has no `issue`
/// entry.
fn issue_directive(config: &Config) -> Result<&str, Error> {
    config
        .roles
        .get(ISSUE_ROLE)
        .map(String::as_str)
        .ok_or_else(|| Error::Config {
            message: format!(
                "role {ISSUE_ROLE:?} is not defined in [tool.patdhlk-skills.roles]; \
                 both `pds status` and `pds next` require it"
            ),
        })
}

/// Guard the backend, resolve the issue directive, run the non-gating build,
/// and load the produced corpus. Returns either a ready corpus + directive
/// string, or a failed build outcome to return directly, or an [`Error`].
///
/// This is the shared preamble for every backlog-query verb. Future verbs
/// (lint, search, dedup, …) call this and add only their own pure query.
fn prepare_corpus(
    config: &Config,
    project_root: &Path,
    gh_hint: &str,
) -> Result<(CorpusResult, String), Error> {
    guard_backend(config, gh_hint)?;
    let directive = issue_directive(config)?.to_string();
    let build = run_build(config, project_root)?;
    if build.is_failed() {
        return Ok((CorpusResult::BuildFailed(build), directive));
    }
    let corpus = NeedsCorpus::load(&config.needs_json)?;
    Ok((CorpusResult::Ready(corpus), directive))
}

// ---------------------------------------------------------------------------
// Orchestration (guard → build → load → query → Outcome)
// ---------------------------------------------------------------------------

/// `pds status`: per-status counts over issue-typed needs from a fresh corpus.
pub fn run_status(config: &Config, project_root: &Path) -> Result<Outcome, Error> {
    let gh_hint = "gh issue list --state all --json labels \
                   --jq '[.[].labels[].name] | group_by(.) | map({(.[0]): length}) | add'";
    let (corpus_result, directive) = prepare_corpus(config, project_root, gh_hint)?;

    let corpus = match corpus_result {
        CorpusResult::Ready(c) => c,
        CorpusResult::BuildFailed(failed) => return Ok(failed),
    };

    let counts = status_counts(&corpus, &directive);
    let total: usize = counts.values().sum();

    let counts_obj: Map<String, Value> = counts
        .into_iter()
        .map(|(k, v)| (k, Value::from(v)))
        .collect();

    let mut payload = Map::new();
    payload.insert("counts".to_string(), Value::Object(counts_obj));
    payload.insert("total".to_string(), Value::from(total));
    Ok(Outcome::clean(payload))
}

/// `pds next`: the lowest-id ready-for-agent issue from a fresh corpus, or a
/// clean `none-ready` outcome when the backlog has none.
pub fn run_next(config: &Config, project_root: &Path) -> Result<Outcome, Error> {
    let gh_hint = "gh issue list --label ready-for-agent --state open \
                   --json number,title,labels --limit 1";
    let (corpus_result, directive) = prepare_corpus(config, project_root, gh_hint)?;

    let corpus = match corpus_result {
        CorpusResult::Ready(c) => c,
        CorpusResult::BuildFailed(failed) => return Ok(failed),
    };

    let payload = match next_issue(&corpus, &directive) {
        Some(need) => next_payload(need),
        None => {
            let mut m = Map::new();
            m.insert("issue".to_string(), Value::Null);
            m.insert(
                "reason".to_string(),
                Value::String("none-ready".to_string()),
            );
            m
        }
    };
    Ok(Outcome::clean(payload))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build an in-memory corpus from inline JSON (object form).
    fn corpus_from(json: &str) -> NeedsCorpus {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        NeedsCorpus::load(f.path()).unwrap()
    }

    const MIXED: &str = r#"{
        "current_version": "",
        "project": "t",
        "versions": { "": { "needs": {
            "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"a","status":"ready-for-agent"},
            "ISSUE_0002": {"id":"ISSUE_0002","type":"issue","title":"b","status":"done"},
            "ISSUE_0003": {"id":"ISSUE_0003","type":"issue","title":"c","status":"done"},
            "ISSUE_0004": {"id":"ISSUE_0004","type":"issue","title":"d","status":"ready-for-agent"},
            "ISSUE_0005": {"id":"ISSUE_0005","type":"issue","title":"e"},
            "FEAT_0001":  {"id":"FEAT_0001","type":"feat","title":"f","status":"done"},
            "TERM_0001":  {"id":"TERM_0001","type":"term","title":"g"}
        } } }
    }"#;

    #[test]
    fn status_counts_buckets_issues_excludes_non_issues_and_handles_no_status() {
        let corpus = corpus_from(MIXED);
        let counts = status_counts(&corpus, "issue");
        assert_eq!(counts.get("ready-for-agent"), Some(&2));
        assert_eq!(counts.get("done"), Some(&2));
        assert_eq!(counts.get("none"), Some(&1));
        // non-issue types excluded entirely
        let total: usize = counts.values().sum();
        assert_eq!(total, 5, "only the 5 issue-typed needs are counted");
    }

    #[test]
    fn next_issue_picks_lowest_ready_for_agent_id() {
        let corpus = corpus_from(MIXED);
        let next = next_issue(&corpus, "issue").expect("a ready issue exists");
        // ISSUE_0001 and ISSUE_0004 are ready; lowest id wins.
        assert_eq!(next.id, "ISSUE_0001");
        assert_eq!(next.status.as_deref(), Some("ready-for-agent"));
    }

    #[test]
    fn next_issue_ignores_ready_non_issue_types() {
        // A feat that is ready-for-agent must not be picked.
        let json = r#"{
            "current_version": "",
            "project": "t",
            "versions": { "": { "needs": {
                "FEAT_0001": {"id":"FEAT_0001","type":"feat","title":"f","status":"ready-for-agent"},
                "ISSUE_0009": {"id":"ISSUE_0009","type":"issue","title":"i","status":"ready-for-agent"}
            } } }
        }"#;
        let corpus = corpus_from(json);
        let next = next_issue(&corpus, "issue").expect("the issue, not the feat");
        assert_eq!(next.id, "ISSUE_0009");
    }

    #[test]
    fn next_issue_none_when_no_issue_is_ready() {
        let json = r#"{
            "current_version": "",
            "project": "t",
            "versions": { "": { "needs": {
                "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"a","status":"done"},
                "ISSUE_0002": {"id":"ISSUE_0002","type":"issue","title":"b","status":"needs-triage"}
            } } }
        }"#;
        let corpus = corpus_from(json);
        assert!(next_issue(&corpus, "issue").is_none());
    }

    #[test]
    fn missing_role_in_corpus_is_clean_zero_counts_and_none_ready() {
        // The directive maps to a type that no need in the corpus has.
        let corpus = corpus_from(MIXED);
        let counts = status_counts(&corpus, "nonexistent");
        assert!(counts.is_empty(), "no needs of that type => empty counts");
        assert!(next_issue(&corpus, "nonexistent").is_none());
    }

    #[test]
    fn next_payload_shape_has_issue_fields_and_null_reason() {
        // Pin the payload assembly: links and null-reason shape must be stable.
        let json = r#"{
            "current_version": "",
            "project": "t",
            "versions": { "": { "needs": {
                "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"first ready",
                               "status":"ready-for-agent","links":["FEAT_0001"]}
            } } }
        }"#;
        let corpus = corpus_from(json);
        let need = next_issue(&corpus, "issue").expect("issue exists");
        let payload = next_payload(need);

        let issue = payload.get("issue").expect("issue key present");
        assert_eq!(issue["id"], "ISSUE_0001");
        assert_eq!(issue["title"], "first ready");
        assert_eq!(issue["status"], "ready-for-agent");
        let links = issue["links"].as_array().expect("links is array");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0], "FEAT_0001");

        let reason = payload.get("reason").expect("reason key present");
        assert!(reason.is_null(), "reason must be null when issue found");
    }
}
