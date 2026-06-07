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

#[cfg(test)]
mod tests {
    use super::*;

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
}
