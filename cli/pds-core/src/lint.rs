//! Mechanical body-lint rules engine for `pds lint` and `pds check`.
//!
//! [`lint_corpus`] is a **pure** function over an already-loaded
//! [`NeedsCorpus`] and a validated [`LintConfig`]: no I/O, no process spawning.
//! It returns a deterministic [`Vec<LintFinding>`]; the bin/[`Outcome`] layer
//! serialises each finding into the shared `{check, severity, need, message}`
//! envelope.
//!
//! # Per-need flow
//!
//! 1. Needs whose `status` is in `exempt_statuses` are skipped entirely. A need
//!    with *no* status is **not** exempt (absence of a status is not "done").
//! 2. Each enabled rule is applied, but only to needs whose `need_type` (the
//!    directive) the rule targets.
//!
//! # Rules
//!
//! - `lint:required-sections` — for each configured section name `S` for the
//!   need's directive, the body must contain the bold lead-in `**S.**` *or*
//!   `**S**` (both accepted; reStructuredText authors write either). One
//!   finding per missing section.
//! - `lint:body-length` — body shorter than `min` → one finding; longer than
//!   `max` → one finding. Length is measured in **characters** (`chars().count()`),
//!   not bytes, so multibyte content is counted intuitively.
//! - `lint:weasel-words` — for targeted directives, each configured word is
//!   matched case-insensitively as a whole word. A hit is **exempt** when its
//!   sentence carries a numeric or behavioral criterion, approximated as: the
//!   sentence contains a digit **or** a comparison/criterion cue (`<`, `>`,
//!   `=`, "at least", "at most", "within", "no more than", "exactly"). One
//!   finding per offending word per need (deduped per word).
//! - `lint:unenumerated-quantifiers` — for targeted directives, each configured
//!   quantifier is matched case-insensitively as a whole word, and flagged when
//!   it is **not** followed (within the same sentence, after the quantifier) by
//!   an enumeration cue ("of the", "of these", "listed", "in the", "declared",
//!   "configured", "below", "above"). One finding per offending quantifier per
//!   need (deduped per quantifier).
//!
//! All sentence splitting is an approximation (split on `.`/`!`/`?`); the
//! finding messages name the criterion / enumeration exemption so authors
//! understand why a word did or did not fire.
//!
//! # Determinism
//!
//! Findings are sorted by `(need, rule, message)` before return, so output is
//! stable across runs regardless of corpus iteration or `HashMap` order.

use std::path::Path;

use serde_json::{Map, Value};

use crate::builder::run_build;
use crate::config::{Config, LintBodyLength, LintConfig};
use crate::error::Error;
use crate::needs::{Need, NeedsCorpus};
use crate::outcome::{self, Outcome};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// One body-lint violation. The `rule` is already namespaced (`lint:<rule>`);
/// the serialiser maps `need`/`rule`/`message` onto the shared finding shape
/// (`{check: rule, severity: "error", need, message}`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintFinding {
    /// The offending need's id (e.g. `ISSUE_0001`).
    pub need: String,
    /// The rule that fired, namespaced (`lint:required-sections`, …).
    pub rule: String,
    /// Human-actionable explanation of the violation.
    pub message: String,
}

// Rule name constants — single source of truth for the `lint:` namespace.
const RULE_REQUIRED_SECTIONS: &str = "lint:required-sections";
const RULE_BODY_LENGTH: &str = "lint:body-length";
const RULE_WEASEL_WORDS: &str = "lint:weasel-words";
const RULE_UNENUMERATED_QUANTIFIERS: &str = "lint:unenumerated-quantifiers";

/// Comparison / criterion cues that exempt a weasel word: their presence in a
/// sentence signals a measurable or behavioral criterion.
const CRITERION_CUES: &[&str] = &[
    "<",
    ">",
    "=",
    "at least",
    "at most",
    "within",
    "no more than",
    "exactly",
];

/// Enumeration cues that satisfy a quantifier: their presence after the
/// quantifier (same sentence) signals the set is stated.
const ENUMERATION_CUES: &[&str] = &[
    "of the",
    "of these",
    "listed",
    "in the",
    "declared",
    "configured",
    "below",
    "above",
];

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Apply every enabled rule in `lint` to every non-exempt need in `corpus`.
///
/// See the module docs for the per-need flow, the rule semantics, and the
/// deterministic ordering guarantee.
pub fn lint_corpus(
    corpus: &NeedsCorpus,
    lint: &LintConfig,
    exempt_statuses: &[String],
) -> Vec<LintFinding> {
    let mut findings: Vec<LintFinding> = Vec::new();

    for need in corpus.iter() {
        // Status-aware exemption: a need with a status in the exempt list is
        // skipped. A need with NO status is never exempt.
        if let Some(status) = need.status.as_deref()
            && exempt_statuses.iter().any(|s| s == status)
        {
            continue;
        }

        if let Some(sections_by_directive) = &lint.required_sections
            && let Some(sections) = sections_by_directive.get(&need.need_type)
        {
            check_required_sections(need, sections, &mut findings);
        }

        if let Some(bounds_by_directive) = &lint.body_length
            && let Some(bounds) = bounds_by_directive.get(&need.need_type)
        {
            check_body_length(need, bounds, &mut findings);
        }

        if let Some(rule) = &lint.weasel_words
            && rule.directives.iter().any(|d| d == &need.need_type)
        {
            check_weasel_words(need, &rule.words, &mut findings);
        }

        if let Some(rule) = &lint.unenumerated_quantifiers
            && rule.directives.iter().any(|d| d == &need.need_type)
        {
            check_unenumerated_quantifiers(need, &rule.quantifiers, &mut findings);
        }
    }

    // Deterministic order: by need, then rule, then message.
    findings.sort_by(|a, b| {
        a.need
            .cmp(&b.need)
            .then_with(|| a.rule.cmp(&b.rule))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

// ---------------------------------------------------------------------------
// Verb orchestration + serialization
// ---------------------------------------------------------------------------

/// True when `lint` has at least one rule key set. An absent table (`None`) or
/// a present-but-empty table both mean "no work to do".
pub fn any_rule_enabled(lint: Option<&LintConfig>) -> bool {
    match lint {
        None => false,
        Some(l) => {
            l.required_sections.is_some()
                || l.body_length.is_some()
                || l.weasel_words.is_some()
                || l.unenumerated_quantifiers.is_some()
        }
    }
}

/// Serialise a [`LintFinding`] into the shared finding envelope:
/// `{"check": "lint:<rule>", "severity": "error", "need": "<ID>", "message": …}`.
///
/// Delegates to [`crate::outcome::finding`] — the shared envelope constructor.
pub fn finding_json(finding: &LintFinding) -> Value {
    outcome::finding(
        &finding.rule,
        "error",
        Some(&finding.need),
        &finding.message,
    )
}

/// `pds lint`: run the body-lint rules over a fresh corpus.
///
/// **No github-backend guard and no issue-role resolution** — lint covers the
/// whole spec corpus (reqs, ADRs, …), which exists on every backend.
///
/// When `config.lint` is `None` or carries no enabled rule, there is nothing to
/// check: we return a clean `{"findings": []}` outcome **without building**.
/// Building a fresh `needs.json` only to run zero checks would waste seconds in
/// every consumer gate; the brief specifies an absent table = clean exit 0, and
/// "no enabled rules" is the same nothing-to-do state.
///
/// Otherwise: non-gating [`run_build`] → [`NeedsCorpus::load`] →
/// [`lint_corpus`] → an [`Outcome`] carrying the findings array plus the
/// `needs_json` path (like `pds check`). Non-empty findings ⇒ failed (exit 1).
/// A failed build is returned as a failed outcome under this verb;
/// unspawnable/unreadable inputs surface as [`Error::Tool`].
pub fn run_lint(config: &Config, project_root: &Path) -> Result<Outcome, Error> {
    // Nothing to do: clean exit 0, no build.
    let Some(lint) = config.lint.as_ref().filter(|l| any_rule_enabled(Some(l))) else {
        let mut payload = Map::new();
        payload.insert("findings".to_string(), Value::Array(Vec::new()));
        return Ok(Outcome::clean(payload));
    };

    // Fresh corpus: non-gating build, then load.
    let build = run_build(config, project_root)?;
    if build.is_failed() {
        return Ok(build);
    }
    let corpus = NeedsCorpus::load(&config.needs_json)?;

    let findings = lint_corpus(&corpus, lint, &config.exempt_statuses);
    Ok(lint_outcome(findings, &config.needs_json))
}

/// Assemble an [`Outcome`] from lint findings plus the corpus path. Empty
/// findings ⇒ clean; any finding ⇒ failed. The `needs_json` path is always
/// reported (the corpus was freshly built), mirroring `pds check`.
fn lint_outcome(findings: Vec<LintFinding>, needs_json: &Path) -> Outcome {
    let arr: Vec<Value> = findings.iter().map(finding_json).collect();
    let failed = !arr.is_empty();
    let mut payload = Map::new();
    payload.insert("findings".to_string(), Value::Array(arr));
    payload.insert(
        "needs_json".to_string(),
        Value::String(needs_json.to_string_lossy().into_owned()),
    );
    if failed {
        Outcome::failed(payload)
    } else {
        Outcome::clean(payload)
    }
}

// ---------------------------------------------------------------------------
// Individual rules
// ---------------------------------------------------------------------------

/// `lint:required-sections`: one finding per section name whose bold lead-in is
/// absent from the body. Accepts both `**Section.**` and `**Section**`.
fn check_required_sections(need: &Need, sections: &[String], out: &mut Vec<LintFinding>) {
    for section in sections {
        let with_dot = format!("**{section}.**");
        let without_dot = format!("**{section}**");
        if !need.content.contains(&with_dot) && !need.content.contains(&without_dot) {
            out.push(LintFinding {
                need: need.id.clone(),
                rule: RULE_REQUIRED_SECTIONS.to_string(),
                message: format!(
                    "missing required section {section:?}: body must contain the bold \
                     lead-in `**{section}.**` or `**{section}**`"
                ),
            });
        }
    }
}

/// `lint:body-length`: one finding when the body (in chars) is below `min`,
/// one when above `max`.
fn check_body_length(need: &Need, bounds: &LintBodyLength, out: &mut Vec<LintFinding>) {
    let len = need.content.chars().count();
    if let Some(min) = bounds.min
        && len < min
    {
        out.push(LintFinding {
            need: need.id.clone(),
            rule: RULE_BODY_LENGTH.to_string(),
            message: format!(
                "body too short: {len} characters, minimum is {min} (length counted in characters)"
            ),
        });
    }
    if let Some(max) = bounds.max
        && len > max
    {
        out.push(LintFinding {
            need: need.id.clone(),
            rule: RULE_BODY_LENGTH.to_string(),
            message: format!(
                "body too long: {len} characters, maximum is {max} (length counted in characters)"
            ),
        });
    }
}

/// `lint:weasel-words`: one finding per offending word (deduped per word). A
/// word is offending when it appears as a whole word in a sentence that lacks a
/// numeric or comparison/criterion cue.
fn check_weasel_words(need: &Need, words: &[String], out: &mut Vec<LintFinding>) {
    for word in words {
        let mut offends = false;
        for sentence in sentences(&need.content) {
            if !contains_whole_word(sentence, word) {
                continue;
            }
            if sentence_has_criterion(sentence) {
                continue; // exempt: this sentence carries a criterion.
            }
            offends = true;
            break;
        }
        if offends {
            out.push(LintFinding {
                need: need.id.clone(),
                rule: RULE_WEASEL_WORDS.to_string(),
                message: format!(
                    "weasel word {word:?} used without a numeric or behavioral criterion \
                     in its sentence (a sentence is exempt when it contains a digit or a \
                     comparison cue such as `<`, `>`, \"at least\", \"within\")"
                ),
            });
        }
    }
}

/// `lint:unenumerated-quantifiers`: one finding per offending quantifier
/// (deduped per quantifier). A quantifier offends when ANY whole-word occurrence
/// of it in a sentence is not followed, between that occurrence and the next
/// occurrence of the same quantifier (or the sentence end), by an enumeration
/// cue. Each occurrence is checked against its own inter-occurrence window so
/// a sentence like "All inputs and all of the outputs" fires for the first
/// unenumerated "all" even though "of the" appears later in the sentence (it
/// belongs to the second occurrence's window, not the first's).
fn check_unenumerated_quantifiers(need: &Need, quantifiers: &[String], out: &mut Vec<LintFinding>) {
    for quantifier in quantifiers {
        let mut offends = false;
        'outer: for sentence in sentences(&need.content) {
            // Collect (start, end) byte-offset pairs for every whole-word
            // occurrence so we can bound each occurrence's tail to the start
            // of the next occurrence.
            let positions: Vec<(usize, usize)> =
                whole_word_positions_with_start(sentence, quantifier).collect();
            for (i, (_start, end)) in positions.iter().enumerate() {
                // Tail: from after this occurrence up to the start of the next
                // (or sentence end if this is the last occurrence).
                let tail_end = positions
                    .get(i + 1)
                    .map(|(next_start, _)| *next_start)
                    .unwrap_or(sentence.len());
                let after = &sentence[*end..tail_end];
                if !has_enumeration_cue(after) {
                    offends = true;
                    break 'outer;
                }
                // This occurrence is enumerated within its window; continue.
            }
        }
        if offends {
            out.push(LintFinding {
                need: need.id.clone(),
                rule: RULE_UNENUMERATED_QUANTIFIERS.to_string(),
                message: format!(
                    "quantifier {quantifier:?} used without naming its set; follow it with an \
                     enumeration cue such as \"of the\", \"listed\", \"in the\", \"declared\""
                ),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Text helpers
// ---------------------------------------------------------------------------

/// Split `text` into sentence-ish slices on `.`, `!`, and `?`. This is an
/// approximation (it does not understand abbreviations); the rule messages say
/// so. Empty trailing fragments are skipped.
fn sentences(text: &str) -> impl Iterator<Item = &str> {
    text.split(['.', '!', '?']).filter(|s| !s.trim().is_empty())
}

/// True when `haystack` contains `word` as a case-insensitive whole word, where
/// a "word" is bounded by non-alphanumeric characters (so `all` does not match
/// inside `small` or `recall`).
fn contains_whole_word(haystack: &str, word: &str) -> bool {
    whole_word_positions(haystack, word).next().is_some()
}

/// Iterate `(start, end)` byte-offset pairs for each case-insensitive
/// whole-word match of `word` in `haystack`. A match is a whole word when the
/// characters immediately before and after it are not alphanumeric.
///
/// Both `start` and `end` are byte offsets into the *original* `haystack` (not
/// the lowercased copy). `end - start == word.len()` for ASCII words.
fn whole_word_positions_with_start(
    haystack: &str,
    word: &str,
) -> impl Iterator<Item = (usize, usize)> {
    let hay_lower = haystack.to_lowercase();
    let word_lower = word.to_lowercase();
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    if !word_lower.is_empty() {
        let bytes = hay_lower.as_bytes();
        let wlen = word_lower.len();
        let mut start = 0;
        while let Some(pos) = hay_lower[start..].find(&word_lower) {
            let abs = start + pos;
            let end = abs + wlen;
            let before_ok = abs == 0 || !is_word_byte(bytes[abs - 1]);
            let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
            if before_ok && after_ok {
                pairs.push((abs, end));
            }
            start = abs + 1;
        }
    }
    pairs.into_iter()
}

/// Iterate byte offsets of the *end* of each case-insensitive whole-word match
/// of `word` in `haystack`. A match is a whole word when the characters
/// immediately before and after it are not alphanumeric.
fn whole_word_positions(haystack: &str, word: &str) -> impl Iterator<Item = usize> {
    whole_word_positions_with_start(haystack, word).map(|(_start, end)| end)
}

/// A byte counts as part of a word for boundary purposes when it is ASCII
/// alphanumeric. Non-ASCII bytes are treated as non-word (boundary) bytes, which
/// means the matcher can OVER-fire when non-ASCII characters are adjacent to an
/// ASCII word: for example, `"caféall"` would match `"all"` because `é` (a
/// multi-byte sequence) is seen as a boundary before `"all"`.
fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric()
}

/// True when `sentence` carries a numeric or comparison/criterion cue.
fn sentence_has_criterion(sentence: &str) -> bool {
    if sentence.chars().any(|c| c.is_ascii_digit()) {
        return true;
    }
    let lower = sentence.to_lowercase();
    CRITERION_CUES.iter().any(|cue| lower.contains(cue))
}

/// True when `tail` contains an enumeration cue as a substring.
fn has_enumeration_cue(tail: &str) -> bool {
    let lower = tail.to_lowercase();
    ENUMERATION_CUES.iter().any(|cue| lower.contains(cue))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{LintUnenumeratedQuantifiers, LintWeaselWords};
    use std::collections::HashMap;

    /// Build an in-memory corpus from inline JSON (object form), reusing the
    /// needs reader so fixtures match the production wire shape.
    fn corpus_from(json: &str) -> NeedsCorpus {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        NeedsCorpus::load(f.path()).unwrap()
    }

    /// Wrap one need's fields into a minimal object-form corpus JSON.
    fn one_need(id: &str, ntype: &str, status: Option<&str>, content: &str) -> String {
        let status_field = match status {
            Some(s) => format!(r#","status":"{s}""#),
            None => String::new(),
        };
        // JSON-escape the content minimally (tests use simple ASCII bodies).
        let escaped = content.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            r#"{{
                "current_version": "",
                "project": "t",
                "versions": {{ "": {{ "needs": {{
                    "{id}": {{"id":"{id}","type":"{ntype}","title":"T","content":"{escaped}"{status_field}}}
                }} }} }}
            }}"#
        )
    }

    fn empty_lint() -> LintConfig {
        LintConfig {
            required_sections: None,
            body_length: None,
            weasel_words: None,
            unenumerated_quantifiers: None,
        }
    }

    fn no_exempt() -> Vec<String> {
        Vec::new()
    }

    // ------------------------------------------------------------------
    // required-sections
    // ------------------------------------------------------------------

    #[test]
    fn required_sections_fires_one_finding_per_missing_section() {
        let corpus = corpus_from(&one_need(
            "ADR_0001",
            "arch-decision",
            Some("ready-for-agent"),
            "**Context.** Some background here.",
        ));
        let mut rs = HashMap::new();
        rs.insert(
            "arch-decision".to_string(),
            vec![
                "Context".to_string(),
                "Decision".to_string(),
                "Consequences".to_string(),
            ],
        );
        let lint = LintConfig {
            required_sections: Some(rs),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        // Context present; Decision + Consequences missing.
        assert_eq!(findings.len(), 2);
        assert!(findings.iter().all(|f| f.rule == RULE_REQUIRED_SECTIONS));
        assert!(findings.iter().all(|f| f.need == "ADR_0001"));
        assert!(findings.iter().any(|f| f.message.contains("Decision")));
        assert!(findings.iter().any(|f| f.message.contains("Consequences")));
    }

    #[test]
    fn required_sections_accepts_both_bold_lead_in_forms() {
        // "Context" with trailing dot, "Decision" without dot — both satisfy.
        let corpus = corpus_from(&one_need(
            "ADR_0001",
            "arch-decision",
            None,
            "**Context.** background **Decision** the call",
        ));
        let mut rs = HashMap::new();
        rs.insert(
            "arch-decision".to_string(),
            vec!["Context".to_string(), "Decision".to_string()],
        );
        let lint = LintConfig {
            required_sections: Some(rs),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert!(
            findings.is_empty(),
            "both `**Context.**` and `**Decision**` forms must satisfy, got: {findings:?}"
        );
    }

    #[test]
    fn required_sections_ignores_non_targeted_directive() {
        // The rule targets arch-decision but the need is an issue.
        let corpus = corpus_from(&one_need("ISSUE_0001", "issue", None, "no sections here"));
        let mut rs = HashMap::new();
        rs.insert("arch-decision".to_string(), vec!["Context".to_string()]);
        let lint = LintConfig {
            required_sections: Some(rs),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert!(
            findings.is_empty(),
            "issue is not targeted, got: {findings:?}"
        );
    }

    // ------------------------------------------------------------------
    // body-length
    // ------------------------------------------------------------------

    #[test]
    fn body_length_fires_when_too_short() {
        let corpus = corpus_from(&one_need("ISSUE_0001", "issue", None, "tiny"));
        let mut bl = HashMap::new();
        bl.insert(
            "issue".to_string(),
            LintBodyLength {
                min: Some(50),
                max: None,
            },
        );
        let lint = LintConfig {
            body_length: Some(bl),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RULE_BODY_LENGTH);
        assert!(findings[0].message.contains("too short"));
        assert!(findings[0].message.contains("50"));
        assert!(findings[0].message.contains('4')); // actual length 4
    }

    #[test]
    fn body_length_fires_when_too_long() {
        let long = "x".repeat(20);
        let corpus = corpus_from(&one_need("ISSUE_0001", "issue", None, &long));
        let mut bl = HashMap::new();
        bl.insert(
            "issue".to_string(),
            LintBodyLength {
                min: None,
                max: Some(10),
            },
        );
        let lint = LintConfig {
            body_length: Some(bl),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(findings.len(), 1);
        assert!(findings[0].message.contains("too long"));
        assert!(findings[0].message.contains("20"));
        assert!(findings[0].message.contains("10"));
    }

    #[test]
    fn body_length_quiet_within_bounds() {
        let body = "x".repeat(30);
        let corpus = corpus_from(&one_need("ISSUE_0001", "issue", None, &body));
        let mut bl = HashMap::new();
        bl.insert(
            "issue".to_string(),
            LintBodyLength {
                min: Some(10),
                max: Some(50),
            },
        );
        let lint = LintConfig {
            body_length: Some(bl),
            ..empty_lint()
        };
        assert!(lint_corpus(&corpus, &lint, &no_exempt()).is_empty());
    }

    #[test]
    fn body_length_counts_characters_not_bytes() {
        // Five 2-byte chars = 5 characters but 10 bytes. min 6 must fire.
        let corpus = corpus_from(&one_need("ISSUE_0001", "issue", None, "ééééé"));
        let mut bl = HashMap::new();
        bl.insert(
            "issue".to_string(),
            LintBodyLength {
                min: Some(6),
                max: None,
            },
        );
        let lint = LintConfig {
            body_length: Some(bl),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(findings.len(), 1, "5 chars < 6 must fire");
        assert!(
            findings[0].message.contains('5'),
            "must report 5 characters, got: {}",
            findings[0].message
        );
    }

    // ------------------------------------------------------------------
    // weasel-words
    // ------------------------------------------------------------------

    fn weasel_lint(words: &[&str], directives: &[&str]) -> LintConfig {
        LintConfig {
            weasel_words: Some(LintWeaselWords {
                words: words.iter().map(|s| s.to_string()).collect(),
                directives: directives.iter().map(|s| s.to_string()).collect(),
            }),
            ..empty_lint()
        }
    }

    #[test]
    fn weasel_word_fires_without_criterion() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "The system shall be robust.",
        ));
        let lint = weasel_lint(&["robust"], &["req"]);
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RULE_WEASEL_WORDS);
        assert!(findings[0].message.contains("robust"));
    }

    #[test]
    fn weasel_word_exempt_when_sentence_has_digit() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "The system shall be robust against 3 concurrent failures.",
        ));
        let lint = weasel_lint(&["robust"], &["req"]);
        assert!(
            lint_corpus(&corpus, &lint, &no_exempt()).is_empty(),
            "digit in sentence exempts the weasel word"
        );
    }

    #[test]
    fn weasel_word_exempt_when_sentence_has_at_least_cue() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "The response shall be appropriate for at least the listed roles.",
        ));
        let lint = weasel_lint(&["appropriate"], &["req"]);
        assert!(
            lint_corpus(&corpus, &lint, &no_exempt()).is_empty(),
            "\"at least\" cue exempts the weasel word"
        );
    }

    #[test]
    fn weasel_word_is_case_insensitive_whole_word() {
        // "Robust" capitalised must still fire; "robustness" must NOT (not whole word).
        let corpus = corpus_from(&one_need("REQ_0001", "req", None, "Robust design."));
        let lint = weasel_lint(&["robust"], &["req"]);
        assert_eq!(lint_corpus(&corpus, &lint, &no_exempt()).len(), 1);

        let corpus2 = corpus_from(&one_need("REQ_0001", "req", None, "Robustness matters."));
        assert!(
            lint_corpus(&corpus2, &lint, &no_exempt()).is_empty(),
            "substring inside a longer word must not match"
        );
    }

    #[test]
    fn weasel_word_dedupes_per_word() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "It is robust. It stays robust. Truly robust.",
        ));
        let lint = weasel_lint(&["robust"], &["req"]);
        assert_eq!(
            lint_corpus(&corpus, &lint, &no_exempt()).len(),
            1,
            "multiple occurrences of one word yield one finding"
        );
    }

    #[test]
    fn weasel_word_ignores_non_targeted_directive() {
        let corpus = corpus_from(&one_need("ISSUE_0001", "issue", None, "very robust thing"));
        let lint = weasel_lint(&["robust"], &["req"]);
        assert!(lint_corpus(&corpus, &lint, &no_exempt()).is_empty());
    }

    // ------------------------------------------------------------------
    // unenumerated-quantifiers
    // ------------------------------------------------------------------

    fn quant_lint(quants: &[&str], directives: &[&str]) -> LintConfig {
        LintConfig {
            unenumerated_quantifiers: Some(LintUnenumeratedQuantifiers {
                quantifiers: quants.iter().map(|s| s.to_string()).collect(),
                directives: directives.iter().map(|s| s.to_string()).collect(),
            }),
            ..empty_lint()
        }
    }

    #[test]
    fn quantifier_fires_when_unenumerated() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "All inputs shall be validated.",
        ));
        let lint = quant_lint(&["all"], &["req"]);
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].rule, RULE_UNENUMERATED_QUANTIFIERS);
        assert!(findings[0].message.contains("all"));
    }

    #[test]
    fn quantifier_exempt_with_enumeration_cue() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "All of the declared inputs shall be validated.",
        ));
        let lint = quant_lint(&["all"], &["req"]);
        assert!(
            lint_corpus(&corpus, &lint, &no_exempt()).is_empty(),
            "\"of the\" / \"declared\" after the quantifier exempts it"
        );
    }

    #[test]
    fn quantifier_cue_must_follow_not_precede() {
        // "listed" appears BEFORE "all" — that does not enumerate the quantifier.
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "The listed roles. Then all users shall comply.",
        ));
        let lint = quant_lint(&["all"], &["req"]);
        assert_eq!(
            lint_corpus(&corpus, &lint, &no_exempt()).len(),
            1,
            "an enumeration cue in an earlier sentence must not exempt the quantifier"
        );
    }

    #[test]
    fn quantifier_is_case_insensitive_whole_word() {
        // "each" capitalised fires; "reach" must not match.
        let corpus = corpus_from(&one_need("REQ_0001", "req", None, "Each module logs."));
        let lint = quant_lint(&["each"], &["req"]);
        assert_eq!(lint_corpus(&corpus, &lint, &no_exempt()).len(), 1);

        let corpus2 = corpus_from(&one_need("REQ_0001", "req", None, "Reach the goal."));
        assert!(lint_corpus(&corpus2, &lint, &no_exempt()).is_empty());
    }

    #[test]
    fn quantifier_dedupes_per_quantifier() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "All this. All that. All other.",
        ));
        let lint = quant_lint(&["all"], &["req"]);
        assert_eq!(lint_corpus(&corpus, &lint, &no_exempt()).len(), 1);
    }

    /// Two occurrences of a quantifier in one sentence: the second has an
    /// enumeration cue but the first does not. The rule must still fire because
    /// the first occurrence is unenumerated (each occurrence is checked
    /// independently, not just the first).
    #[test]
    fn quantifier_fires_when_first_occurrence_has_no_cue_and_second_does() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            // "all inputs" — no cue; "all of the outputs" — cue ("of the") after second "all".
            "All inputs shall be valid and all of the outputs shall be logged.",
        ));
        let lint = quant_lint(&["all"], &["req"]);
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(
            findings.len(),
            1,
            "first unenumerated occurrence must fire even though the second has a cue"
        );
        assert_eq!(findings[0].rule, RULE_UNENUMERATED_QUANTIFIERS);
    }

    /// Two occurrences of a quantifier in one sentence where BOTH have an
    /// enumeration cue: the rule must stay silent.
    #[test]
    fn quantifier_quiet_when_all_occurrences_have_cues() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            None,
            "All of the inputs and all of the outputs shall be validated.",
        ));
        let lint = quant_lint(&["all"], &["req"]);
        assert!(
            lint_corpus(&corpus, &lint, &no_exempt()).is_empty(),
            "both occurrences have enumeration cues; must not fire"
        );
    }

    // ------------------------------------------------------------------
    // exemption + ordering
    // ------------------------------------------------------------------

    #[test]
    fn exempt_status_need_is_skipped() {
        let corpus = corpus_from(&one_need(
            "REQ_0001",
            "req",
            Some("done"),
            "The system shall be robust.",
        ));
        let lint = weasel_lint(&["robust"], &["req"]);
        assert!(
            lint_corpus(&corpus, &lint, &["done".to_string()]).is_empty(),
            "a done need is exempt from all rules"
        );
    }

    #[test]
    fn need_with_no_status_is_not_exempt() {
        let corpus = corpus_from(&one_need("REQ_0001", "req", None, "Be robust."));
        let lint = weasel_lint(&["robust"], &["req"]);
        assert_eq!(
            lint_corpus(&corpus, &lint, &["done".to_string(), "wontfix".to_string()]).len(),
            1,
            "absence of a status must NOT be treated as exempt"
        );
    }

    #[test]
    fn findings_are_sorted_by_need_then_rule_then_message() {
        // Two needs each violating two rules; assert global ordering.
        let json = r#"{
            "current_version": "",
            "project": "t",
            "versions": { "": { "needs": {
                "REQ_0002": {"id":"REQ_0002","type":"req","title":"T","content":"All robust things."},
                "REQ_0001": {"id":"REQ_0001","type":"req","title":"T","content":"All robust things."}
            } } }
        }"#;
        let corpus = corpus_from(json);
        let lint = LintConfig {
            weasel_words: Some(LintWeaselWords {
                words: vec!["robust".to_string()],
                directives: vec!["req".to_string()],
            }),
            unenumerated_quantifiers: Some(LintUnenumeratedQuantifiers {
                quantifiers: vec!["all".to_string()],
                directives: vec!["req".to_string()],
            }),
            ..empty_lint()
        };
        let findings = lint_corpus(&corpus, &lint, &no_exempt());
        assert_eq!(findings.len(), 4);
        // REQ_0001 before REQ_0002; within each, rule order is alphabetical:
        // "lint:unenumerated-quantifiers" > "lint:weasel-words"? compare strings.
        let keys: Vec<(String, String)> = findings
            .iter()
            .map(|f| (f.need.clone(), f.rule.clone()))
            .collect();
        let mut sorted = keys.clone();
        sorted.sort();
        assert_eq!(keys, sorted, "findings must be globally sorted");
        assert_eq!(findings[0].need, "REQ_0001");
        assert_eq!(findings[3].need, "REQ_0002");
    }
}
