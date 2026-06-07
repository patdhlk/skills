//! BM25 retrieval over a needs corpus for `pds search` and `pds dedup`.
//!
//! The contract is ADR_0021: both verbs share one hits shape, `score` is a
//! 0–1 ratio (raw score / the query's self-score), the engine is named once
//! at top level, and only issue-typed hits can flip the dedup verdict.
//!
//! Everything above the orchestration layer is pure and lib-testable:
//! [`tokenize`], [`Index`], and [`dedup_verdict`] operate on plain data.
//! Engine internals (BM25 parameters, field weights, hit cap) are code
//! constants, not configuration — they are implementation, not contract.

/// Lowercase the text and split it on non-alphanumeric boundaries.
///
/// No stemming, no stopword list: IDF already crushes ubiquitous terms, and
/// stemmer behavior on jargon ("dedup", "needs.json") is unpredictable.
/// Digit runs survive as tokens, so `ISSUE_0005` yields `["issue", "0005"]`
/// and need-id references in bodies become searchable terms.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_lowercases_and_splits_on_non_alphanumerics() {
        assert_eq!(tokenize("Lint Gate"), vec!["lint", "gate"]);
    }

    #[test]
    fn tokenize_splits_need_ids_keeping_digit_tokens() {
        assert_eq!(tokenize("ISSUE_0005"), vec!["issue", "0005"]);
    }

    #[test]
    fn tokenize_dissolves_rst_markup() {
        assert_eq!(
            tokenize("**Decision.** :need:`ADR_0007` is ``accepted``"),
            vec!["decision", "need", "adr", "0007", "is", "accepted"]
        );
    }

    #[test]
    fn tokenize_empty_and_symbol_only_strings_yield_no_tokens() {
        assert!(tokenize("").is_empty());
        assert!(tokenize("--- *** !!!").is_empty());
    }
}
