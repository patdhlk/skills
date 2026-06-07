//! Verdict directives and the `pds verdict-check` corpus check (ISSUE_0014).
//!
//! The shape contract is ADR_0015: one verdict per judged need, derived ID
//! `VERDICT_<judged-id>`, status-less, pass derived from an empty
//! `axes_failed`, staleness computed from a content fingerprint. Rubric
//! config is ADR_0016: the binary validates names and structure only.
//!
//! Mirrors `lint.rs`: [`verdict_check_corpus`] is pure and lib-testable;
//! [`run_verdict_check`] orchestrates build → load → check.

use std::collections::HashMap;

use serde_json::Value;

use crate::config::VerdictsConfig;
use crate::needs::{Need, NeedsCorpus};
use crate::outcome::finding;
use sha2::{Digest, Sha256};

/// Content fingerprint for staleness detection (ADR_0015, pinned in the
/// ISSUE_0014 brief): `"sha256:" + first 16 lowercase hex` of SHA-256 over
/// the UTF-8 of `norm(title) + "\n" + norm(content)`, where `norm` collapses
/// every whitespace run to a single space and trims. Each field is normalized
/// **separately** before joining, so the `"\n"` separator survives in the
/// hashed string — a word migrating across the title/body boundary
/// invalidates the fingerprint. Whitespace-only edits (RST reflow) within a
/// field never invalidate; any word change does. Directive options (status,
/// links) never enter the hash.
pub fn fingerprint(title: &str, content: &str) -> String {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    let normalized = format!("{}\n{}", norm(title), norm(content));
    let digest = Sha256::digest(normalized.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{}", &hex[..16])
}

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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn fingerprint_has_pinned_shape() {
        let fp = fingerprint("a title", "a body");
        assert!(fp.starts_with("sha256:"), "got: {fp}");
        assert_eq!(fp.len(), "sha256:".len() + 16, "16 hex chars, got: {fp}");
        assert!(
            fp["sha256:".len()..]
                .chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
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
        // Single-token boundary…
        assert_ne!(fingerprint("ab", "c"), fingerprint("a", "bc"));
        // …and the case joint normalization got wrong: a word migrating
        // across the boundary of a multi-word title must invalidate.
        assert_ne!(fingerprint("a b", "c"), fingerprint("a", "b c"));
    }

    #[test]
    fn leading_trailing_whitespace_is_trimmed() {
        assert_eq!(
            fingerprint("title", "body"),
            fingerprint("  title", "body  ")
        );
    }

    #[test]
    fn missing_verdict_is_flagged_on_the_judged_need() {
        let corpus = one_issue_corpus("ready-for-agent", None);
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert!(findings.is_empty(), "got: {findings:?}");
    }

    #[test]
    fn failing_axes_are_flagged_on_the_judged_need() {
        let fields = format!(
            r#""rubric":"triage","axes_failed":"state","fingerprint":"{}""#,
            fixture_fp()
        );
        let corpus = one_issue_corpus("ready-for-agent", Some(&fields));
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Failing);
        assert_eq!(findings[0].need, "ISSUE_0001");
        assert!(findings[0].message.contains("state"));
    }

    #[test]
    fn stale_fingerprint_is_flagged_and_message_names_the_recomputed_value() {
        let fields =
            r#""rubric":"triage","axes_failed":"","fingerprint":"sha256:0000000000000000""#;
        let corpus = one_issue_corpus("ready-for-agent", Some(fields));
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
        let fields =
            r#""rubric":"triage","axes_failed":"state","fingerprint":"sha256:0000000000000000""#;
        let corpus = one_issue_corpus("ready-for-agent", Some(fields));
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        // The verdict is malformed AND the requirement is unsatisfied (wrong
        // rubric ⇒ missing). Two findings, deterministic order by need id.
        assert_eq!(findings.len(), 2, "got: {findings:?}");
        assert!(
            findings
                .iter()
                .any(|f| f.bucket == Bucket::Malformed && f.need == "VERDICT_ISSUE_0001")
        );
        assert!(findings.iter().any(|f| f.bucket == Bucket::Missing
            && f.need == "ISSUE_0001"
            && f.message.contains("nonsense")));
    }

    #[test]
    fn missing_fingerprint_field_is_malformed() {
        let fields = r#""rubric":"triage","axes_failed":"""#;
        let corpus = one_issue_corpus("ready-for-agent", Some(fields));
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].bucket, Bucket::Malformed);
        assert_eq!(findings[0].need, "VERD_0001");
    }

    #[test]
    fn exempt_status_skips_all_buckets() {
        let corpus = one_issue_corpus("done", None);
        let findings =
            verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", &exempt_done());
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
        let unscoped = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
        assert_eq!(unscoped.len(), 1);
        assert_eq!(unscoped[0].bucket, Bucket::Missing);
    }

    #[test]
    fn non_required_types_never_need_verdicts() {
        let json = r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "ADR_0001": {"id":"ADR_0001","type":"arch-decision","title":"t","content":"b"}
        }}}}"#;
        let corpus = corpus_from(json);
        let findings = verdict_check_corpus(&corpus, &cfg(None), &rubrics(), "verdict", EXEMPT);
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
}
