//! Integration tests that load the real repo's needs.json fixture.
//!
//! The fixture at `tests/fixtures/real-repo-needs.json` was copied from
//! `spec/_build/needs/needs.json` once and committed under `cli/`.
//! It exercises the full file without requiring the Sphinx build tree to exist.

use std::path::Path;

use pds_core::NeedsCorpus;

#[test]
fn real_repo_needs_json_loads_without_error() {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/real-repo-needs.json"
    ));
    let corpus = NeedsCorpus::load(fixture).unwrap();
    assert!(!corpus.is_empty(), "real corpus must not be empty");
}

#[test]
fn real_repo_contains_issue_0019_with_correct_type() {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/real-repo-needs.json"
    ));
    let corpus = NeedsCorpus::load(fixture).unwrap();
    let issue = corpus
        .get("ISSUE_0019")
        .expect("ISSUE_0019 must be present in the real corpus");
    assert_eq!(
        issue.need_type, "issue",
        "ISSUE_0019 must have type 'issue', got: {:?}",
        issue.need_type
    );
}

#[test]
fn real_repo_iteration_is_id_sorted() {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/real-repo-needs.json"
    ));
    let corpus = NeedsCorpus::load(fixture).unwrap();
    let ids: Vec<&str> = corpus.iter().map(|n| n.id.as_str()).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "corpus iteration must be sorted by id");
}

#[test]
fn real_repo_first_id_is_lexicographically_smallest() {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/real-repo-needs.json"
    ));
    let corpus = NeedsCorpus::load(fixture).unwrap();
    let first = corpus.iter().next().expect("corpus is non-empty");
    for need in corpus.iter() {
        assert!(
            need.id >= first.id,
            "need {:?} precedes first id {:?} — not sorted",
            need.id,
            first.id
        );
    }
}

#[test]
fn real_repo_all_needs_have_non_empty_id_type_title() {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/real-repo-needs.json"
    ));
    let corpus = NeedsCorpus::load(fixture).unwrap();
    for need in corpus.iter() {
        assert!(!need.id.is_empty(), "need has empty id");
        assert!(
            !need.need_type.is_empty(),
            "need {:?} has empty type",
            need.id
        );
        assert!(!need.title.is_empty(), "need {:?} has empty title", need.id);
    }
}

#[test]
fn real_repo_extras_preserved_for_issue_0019() {
    let fixture = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/real-repo-needs.json"
    ));
    let corpus = NeedsCorpus::load(fixture).unwrap();
    let issue = corpus.get("ISSUE_0019").unwrap();
    // The real file has "kind" and "__source__" beyond the standard fields.
    assert!(
        issue.extras.contains_key("kind"),
        "ISSUE_0019 extras must include 'kind'"
    );
    assert!(
        issue.extras.contains_key("__source__"),
        "ISSUE_0019 extras must include '__source__'"
    );
}
