# pds search + pds dedup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement ISSUE_0017 — two new `pds` verbs: `pds search "<query>"` (neutral ranked retrieval, always exit 0) and `pds dedup "<candidate>"` (pre-filing duplicate gate, exit 1 when an issue-typed hit reaches the threshold), backed by a hand-rolled Okapi BM25 engine with self-score-normalized 0–1 scores per ADR_0021.

**Architecture:** A new `retrieval` module in `pds-core` holds pure, lib-testable functions (tokenizer, BM25 index, ranking, verdict) mirroring the shape of `queries.rs`, plus the two `run_*` orchestrators that reuse the existing guard→build→load preamble. The CLI gains two clap subcommands with string payloads. Zero new dependencies.

**Tech Stack:** Rust (edition 2024), clap derive, serde_json; tests via `cargo test`, e2e via assert_cmd with fake-builder shell scripts (existing pattern in `cli/pds-cli/tests/cli.rs`).

**Authoritative spec:** the Agent Brief on ISSUE_0017 in `spec/issues/index.rst` and ADR_0021 in `spec/architecture/index.rst`. Read both before starting.

**Working directory for all cargo commands:** `cli/` (the workspace root).

---

### Task 1: Tokenizer

**Files:**
- Create: `cli/pds-core/src/retrieval.rs`
- Modify: `cli/pds-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `cli/pds-core/src/retrieval.rs` with module doc, the tokenizer signature stubbed via `todo!()`, and tests:

```rust
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
    todo!()
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
```

Register the module in `cli/pds-core/src/lib.rs`: in the `pub mod` list add (alphabetical, after `queries`):

```rust
pub mod retrieval;
```

and at the end of the `pub use` block add:

```rust
pub use retrieval::tokenize;
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pds-core retrieval` (from `cli/`)
Expected: FAIL — panics with `not yet implemented` (the `todo!()`).

- [ ] **Step 3: Implement the tokenizer**

Replace the `todo!()` body:

```rust
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pds-core retrieval`
Expected: 4 passed.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/retrieval.rs cli/pds-core/src/lib.rs
git commit -m "feat(pds-core): retrieval tokenizer — lowercase alphanumeric split (ISSUE_0017)"
```

---

### Task 2: BM25 index and normalized ranking

**Files:**
- Modify: `cli/pds-core/src/retrieval.rs`
- Modify: `cli/pds-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Append to `cli/pds-core/src/retrieval.rs` (above the tests module) the types and constants with stubbed methods:

```rust
use std::collections::HashMap;

use crate::needs::{Need, NeedsCorpus};

/// Okapi BM25 term-frequency saturation parameter. Standard value; not
/// configurable — nobody should tune this per-repo.
const K1: f64 = 1.2;
/// Okapi BM25 length-normalization parameter. Standard value.
const B: f64 = 0.75;
/// Field weights: a title-term match should dominate a content match, so
/// title tokens are repeated into the document. Poor man's BM25F.
const TITLE_WEIGHT: usize = 3;
const TAGS_WEIGHT: usize = 2;
const CONTENT_WEIGHT: usize = 1;
/// Hits are capped — ranked output below ~10 entries is noise for an agent.
const MAX_HITS: usize = 10;

/// A ranked retrieval hit: a need plus its normalized 0–1 score.
pub struct Hit<'a> {
    pub need: &'a Need,
    /// `raw BM25 score / the query's self-score`, clamped to 1.0 (ADR_0021).
    pub score: f64,
}

/// Count occurrences per token.
fn term_frequencies(tokens: &[String]) -> HashMap<String, usize> {
    let mut tf: HashMap<String, usize> = HashMap::new();
    for t in tokens {
        *tf.entry(t.clone()).or_insert(0) += 1;
    }
    tf
}

/// One indexed document: the need, its weighted term frequencies, and its
/// weighted token count.
struct IndexedDoc<'a> {
    need: &'a Need,
    tf: HashMap<String, usize>,
    len: usize,
}

/// An in-memory BM25 inverted index over a needs corpus.
///
/// Deterministic: corpus iteration is id-sorted, ranking ties break on id.
pub struct Index<'a> {
    docs: Vec<IndexedDoc<'a>>,
    /// Document frequency per term.
    df: HashMap<String, usize>,
    avg_len: f64,
}

impl<'a> Index<'a> {
    /// Index every need in the corpus, all types included (ADR_0021): an
    /// ADR hit on a dedup candidate means "already decided/shipped", which
    /// is exactly what an agent needs to see before filing.
    pub fn build(corpus: &'a NeedsCorpus) -> Index<'a> {
        todo!()
    }

    /// Rank the corpus against `query`, returning normalized hits sorted by
    /// descending score (id ascending on ties), capped at [`MAX_HITS`].
    /// Zero-scoring documents are omitted. Empty token streams (empty query,
    /// empty corpus) return no hits.
    pub fn rank(&self, query: &str) -> Vec<Hit<'a>> {
        todo!()
    }
}
```

Append tests inside the existing `mod tests`:

```rust
    /// Build an in-memory corpus from inline needs JSON (object form),
    /// mirroring the helper in queries.rs.
    fn corpus_from(json: &str) -> NeedsCorpus {
        use std::io::Write;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(json.as_bytes()).unwrap();
        NeedsCorpus::load(f.path()).unwrap()
    }

    const RETRIEVAL_CORPUS: &str = r#"{
        "current_version": "",
        "project": "t",
        "versions": { "": { "needs": {
            "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"lint gate",
                           "status":"done","content":"strict corpus checks"},
            "ISSUE_0002": {"id":"ISSUE_0002","type":"issue","title":"release pipeline",
                           "status":"ready-for-agent","content":"the lint gate is mentioned here"},
            "ADR_0001":   {"id":"ADR_0001","type":"arch-decision","title":"strict build gate",
                           "status":"accepted","content":"every mutation runs the gate"}
        } } }
    }"#;

    #[test]
    fn self_match_scores_one() {
        // A query identical to a document's full text hits the normalization
        // ceiling: score == 1.0 (clamped).
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let index = Index::build(&corpus);
        let hits = index.rank("lint gate strict corpus checks");
        assert_eq!(hits[0].need.id, "ISSUE_0001");
        assert!(
            (hits[0].score - 1.0).abs() < 1e-9,
            "self-match must clamp to 1.0, got {}",
            hits[0].score
        );
    }

    #[test]
    fn title_match_outranks_content_match() {
        // "lint gate" appears in ISSUE_0001's title (weight 3) and in
        // ISSUE_0002's content (weight 1): the title match must rank first.
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let index = Index::build(&corpus);
        let hits = index.rank("lint gate");
        assert_eq!(hits[0].need.id, "ISSUE_0001");
        assert!(hits.iter().any(|h| h.need.id == "ISSUE_0002"));
        let pos1 = hits.iter().position(|h| h.need.id == "ISSUE_0001").unwrap();
        let pos2 = hits.iter().position(|h| h.need.id == "ISSUE_0002").unwrap();
        assert!(pos1 < pos2, "title match must outrank content match");
    }

    #[test]
    fn scores_are_normalized_into_unit_interval() {
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let index = Index::build(&corpus);
        for hit in index.rank("strict gate") {
            assert!(
                hit.score > 0.0 && hit.score <= 1.0,
                "{} scored {}, outside (0, 1]",
                hit.need.id,
                hit.score
            );
        }
    }

    #[test]
    fn unknown_query_terms_yield_no_hits() {
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let index = Index::build(&corpus);
        assert!(index.rank("zebra astronomy quantum").is_empty());
    }

    #[test]
    fn empty_corpus_yields_no_hits() {
        let corpus = corpus_from(
            r#"{"current_version":"","project":"t","versions":{"":{"needs":{}}}}"#,
        );
        let index = Index::build(&corpus);
        assert!(index.rank("anything").is_empty());
    }

    #[test]
    fn empty_query_yields_no_hits() {
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let index = Index::build(&corpus);
        assert!(index.rank("").is_empty());
        assert!(index.rank("!!! ***").is_empty());
    }

    #[test]
    fn ranking_is_deterministic_with_id_tiebreak() {
        // Two needs with identical text must tie on score and order by id.
        let corpus = corpus_from(
            r#"{
            "current_version": "", "project": "t",
            "versions": { "": { "needs": {
                "ISSUE_0002": {"id":"ISSUE_0002","type":"issue","title":"same words"},
                "ISSUE_0001": {"id":"ISSUE_0001","type":"issue","title":"same words"}
            } } } }"#,
        );
        let index = Index::build(&corpus);
        let hits = index.rank("same words");
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].need.id, "ISSUE_0001");
        assert_eq!(hits[1].need.id, "ISSUE_0002");
        assert!((hits[0].score - hits[1].score).abs() < 1e-9);
    }

    #[test]
    fn hits_are_capped_at_max() {
        // 15 needs all matching the query: only MAX_HITS (10) come back.
        let mut needs = String::new();
        for i in 1..=15 {
            if i > 1 {
                needs.push(',');
            }
            needs.push_str(&format!(
                r#""ISSUE_{i:04}": {{"id":"ISSUE_{i:04}","type":"issue","title":"common term {i}"}}"#
            ));
        }
        let json = format!(
            r#"{{"current_version":"","project":"t","versions":{{"":{{"needs":{{{needs}}}}}}}}}"#
        );
        let corpus = corpus_from(&json);
        let index = Index::build(&corpus);
        let hits = index.rank("common term");
        assert_eq!(hits.len(), 10);
    }
```

Export the new types from `cli/pds-core/src/lib.rs` — replace the `pub use retrieval::tokenize;` line with:

```rust
pub use retrieval::{Hit, Index, tokenize};
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pds-core retrieval`
Expected: tokenizer tests pass, the 8 new tests FAIL with `not yet implemented`.

- [ ] **Step 3: Implement build and rank**

Replace the two `todo!()` bodies:

```rust
    pub fn build(corpus: &'a NeedsCorpus) -> Index<'a> {
        let mut docs: Vec<IndexedDoc<'a>> = Vec::new();
        for need in corpus.iter() {
            let mut tokens: Vec<String> = Vec::new();
            for _ in 0..TITLE_WEIGHT {
                tokens.extend(tokenize(&need.title));
            }
            for _ in 0..TAGS_WEIGHT {
                for tag in &need.tags {
                    tokens.extend(tokenize(tag));
                }
            }
            for _ in 0..CONTENT_WEIGHT {
                tokens.extend(tokenize(&need.content));
            }
            let len = tokens.len();
            let tf = term_frequencies(&tokens);
            docs.push(IndexedDoc { need, tf, len });
        }
        let mut df: HashMap<String, usize> = HashMap::new();
        for doc in &docs {
            for term in doc.tf.keys() {
                *df.entry(term.clone()).or_insert(0) += 1;
            }
        }
        let avg_len = if docs.is_empty() {
            0.0
        } else {
            docs.iter().map(|d| d.len).sum::<usize>() as f64 / docs.len() as f64
        };
        Index { docs, df, avg_len }
    }

    pub fn rank(&self, query: &str) -> Vec<Hit<'a>> {
        let query_tokens = tokenize(query);
        if query_tokens.is_empty() || self.docs.is_empty() {
            return Vec::new();
        }
        let query_tf = term_frequencies(&query_tokens);
        // The query scored as if it were a document: the theoretical maximum
        // raw score for this query, used as the normalization denominator
        // (ADR_0021). Always > 0 when the query has at least one token.
        let self_score = self.raw_score(&query_tf, query_tokens.len(), &query_tf);
        if self_score <= 0.0 {
            return Vec::new();
        }
        let mut hits: Vec<Hit<'a>> = self
            .docs
            .iter()
            .filter_map(|doc| {
                let raw = self.raw_score(&doc.tf, doc.len, &query_tf);
                if raw <= 0.0 {
                    return None;
                }
                // A document can out-score the query itself (heavier term
                // frequency, shorter length), so clamp at the ceiling.
                Some(Hit {
                    need: doc.need,
                    score: (raw / self_score).min(1.0),
                })
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.need.id.cmp(&b.need.id))
        });
        hits.truncate(MAX_HITS);
        hits
    }
```

And add the two private scoring helpers to the `impl` block:

```rust
    /// Lucene-style BM25 IDF: `ln((N - df + 0.5) / (df + 0.5) + 1)`.
    /// The `+ 1` keeps IDF positive even for terms in more than half the
    /// documents — important on tiny corpora, where classic BM25 IDF goes
    /// negative and breaks the ranking.
    fn idf(&self, term: &str) -> f64 {
        let n = self.docs.len() as f64;
        let df = self.df.get(term).copied().unwrap_or(0) as f64;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Raw Okapi BM25 score of a document (given by its term frequencies and
    /// length) against the query's term frequencies.
    fn raw_score(
        &self,
        doc_tf: &HashMap<String, usize>,
        doc_len: usize,
        query_tf: &HashMap<String, usize>,
    ) -> f64 {
        let mut score = 0.0;
        for term in query_tf.keys() {
            let f = doc_tf.get(term).copied().unwrap_or(0) as f64;
            if f == 0.0 {
                continue;
            }
            let denom = f + K1 * (1.0 - B + B * (doc_len as f64) / self.avg_len);
            score += self.idf(term) * (f * (K1 + 1.0)) / denom;
        }
        score
    }
```

Note: `avg_len` can only be 0.0 when `docs` is empty, and `rank` returns early in that case — `raw_score` never divides by zero.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pds-core retrieval`
Expected: 12 passed.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/retrieval.rs cli/pds-core/src/lib.rs
git commit -m "feat(pds-core): hand-rolled BM25 index with self-score-normalized ranking (ADR_0021)"
```

---

### Task 3: Dedup verdict

**Files:**
- Modify: `cli/pds-core/src/retrieval.rs`
- Modify: `cli/pds-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add the stub above the tests module:

```rust
/// The dedup verdict over ranked hits: `"duplicate"` iff at least one
/// **issue-typed** hit reaches the threshold (ADR_0021). Non-issue hits
/// (ADRs, feats, terms) inform the agent but never gate the filing.
pub fn dedup_verdict(hits: &[Hit<'_>], issue_directive: &str, threshold: f64) -> &'static str {
    todo!()
}
```

Add tests inside `mod tests`:

```rust
    /// Hits fixture: an issue and an ADR at given scores.
    fn fixture_hits(corpus: &NeedsCorpus, scores: &[(&str, f64)]) -> Vec<Hit<'_>> {
        scores
            .iter()
            .map(|(id, score)| Hit {
                need: corpus.iter().find(|n| n.id == *id).unwrap(),
                score: *score,
            })
            .collect()
    }

    #[test]
    fn issue_hit_at_threshold_is_duplicate() {
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let hits = fixture_hits(&corpus, &[("ISSUE_0001", 0.5)]);
        assert_eq!(dedup_verdict(&hits, "issue", 0.5), "duplicate");
    }

    #[test]
    fn issue_hit_below_threshold_is_unique() {
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let hits = fixture_hits(&corpus, &[("ISSUE_0001", 0.49)]);
        assert_eq!(dedup_verdict(&hits, "issue", 0.5), "unique");
    }

    #[test]
    fn non_issue_hit_above_threshold_never_gates() {
        // The ADR scores 0.9 but is not issue-typed: verdict stays unique.
        let corpus = corpus_from(RETRIEVAL_CORPUS);
        let hits = fixture_hits(&corpus, &[("ADR_0001", 0.9), ("ISSUE_0001", 0.2)]);
        assert_eq!(dedup_verdict(&hits, "issue", 0.5), "unique");
    }

    #[test]
    fn empty_hits_are_unique() {
        assert_eq!(dedup_verdict(&[], "issue", 0.5), "unique");
    }
```

Update the lib.rs export line to:

```rust
pub use retrieval::{Hit, Index, dedup_verdict, tokenize};
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pds-core retrieval`
Expected: the 4 new tests FAIL with `not yet implemented`.

- [ ] **Step 3: Implement**

```rust
pub fn dedup_verdict(hits: &[Hit<'_>], issue_directive: &str, threshold: f64) -> &'static str {
    if hits
        .iter()
        .any(|h| h.need.need_type == issue_directive && h.score >= threshold)
    {
        "duplicate"
    } else {
        "unique"
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pds-core retrieval`
Expected: 16 passed.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/retrieval.rs cli/pds-core/src/lib.rs
git commit -m "feat(pds-core): dedup verdict — only issue-typed hits gate (ADR_0021)"
```

---

### Task 4: Threshold configuration

**Files:**
- Modify: `cli/pds-core/src/config.rs`
- Modify: `cli/pds-core/src/retrieval.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `mod tests` in `cli/pds-core/src/config.rs`:

```rust
    // ------------------------------------------------------------------
    // dedup table: threshold
    // ------------------------------------------------------------------

    #[test]
    fn absent_dedup_table_gives_default_threshold() {
        let toml = "[project]\nname = \"x\"";
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, crate::retrieval::DEFAULT_THRESHOLD);
    }

    #[test]
    fn explicit_dedup_threshold_overrides_default() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 0.6
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, 0.6);
    }

    #[test]
    fn dedup_threshold_zero_is_config_error() {
        // 0 would gate on any positive-scoring hit — config noise, reject.
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 0.0
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        assert!(err.to_string().contains("threshold"));
    }

    #[test]
    fn dedup_threshold_above_one_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 1.5
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
        let msg = err.to_string();
        assert!(msg.contains("threshold") && msg.contains("1.5"), "got: {msg}");
    }

    #[test]
    fn dedup_threshold_negative_is_config_error() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = -0.2
"#;
        let (_tmp, project) = make_project(toml);
        let err = Config::load(&project).unwrap_err();
        assert!(matches!(err, Error::Config { .. }));
    }

    #[test]
    fn dedup_threshold_exactly_one_is_legal() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 1.0
"#;
        let (_tmp, project) = make_project(toml);
        let cfg = Config::load(&project).unwrap();
        assert_eq!(cfg.dedup_threshold, 1.0);
    }

    #[test]
    fn unknown_dedup_keys_are_ignored() {
        let toml = r#"
[tool.patdhlk-skills.dedup]
threshold = 0.4
future_engine_key = "embed"
"#;
        let (_tmp, project) = make_project(toml);
        assert!(Config::load(&project).is_ok());
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pds-core config`
Expected: the 7 new tests FAIL to compile (no `dedup_threshold` field, no `DEFAULT_THRESHOLD`). A compile error is the failing state here.

- [ ] **Step 3: Implement**

In `cli/pds-core/src/retrieval.rs`, add next to the other constants (this one is `pub` — config needs it, and consumers may want to display it):

```rust
/// The shipped dedup threshold when `[tool.patdhlk-skills.dedup]` has none.
/// Chosen by eyeballing real hits against this repo's corpus (final task of
/// the ISSUE_0017 plan tunes and re-records this value).
pub const DEFAULT_THRESHOLD: f64 = 0.35;
```

In `cli/pds-core/src/config.rs`:

1. Add the field to `Config` (after `lint`):

```rust
    /// Similarity threshold for `pds dedup` (0–1], from
    /// `[tool.patdhlk-skills.dedup]`; defaults to
    /// [`crate::retrieval::DEFAULT_THRESHOLD`] when absent.
    pub dedup_threshold: f64,
```

2. Add to `RawPatdhlkSkills` (after `lint`):

```rust
    dedup: Option<RawDedup>,
```

and the raw type (after `RawLintConfig`'s sibling raw types):

```rust
/// Raw `[tool.patdhlk-skills.dedup]` — threshold optional; unknown keys ignored.
#[derive(Deserialize, Default)]
struct RawDedup {
    threshold: Option<f64>,
}
```

3. In `from_raw`, destructure the new field in the existing `RawPatdhlkSkills { ... }` pattern (add `dedup: raw_dedup,` after `lint: raw_lint,`), and resolve it after the `lint` block:

```rust
        // dedup table
        let dedup_threshold = match raw_dedup.and_then(|d| d.threshold) {
            Some(t) => {
                validate_threshold(t, "dedup.threshold")?;
                t
            }
            None => crate::retrieval::DEFAULT_THRESHOLD,
        };
```

and add `dedup_threshold,` to the final `Ok(Config { ... })`.

4. Add the validator in the Helpers section (it is `pub(crate)` — `run_dedup` reuses it for the `--threshold` flag):

```rust
/// Validate a dedup similarity threshold: finite, > 0, ≤ 1.
///
/// `key` names the source in the error message (`"dedup.threshold"` for the
/// config table, `"--threshold"` for the CLI flag).
pub(crate) fn validate_threshold(value: f64, key: &str) -> Result<(), Error> {
    if !value.is_finite() || value <= 0.0 || value > 1.0 {
        return Err(Error::Config {
            message: format!("{key} must be a number in (0, 1], got {value}"),
        });
    }
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pds-core`
Expected: all pds-core tests pass (config + retrieval + existing).

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/config.rs cli/pds-core/src/retrieval.rs
git commit -m "feat(pds-core): [tool.patdhlk-skills.dedup] threshold config with (0,1] validation"
```

---

### Task 5: Extract corpus-only preamble in queries.rs

`pds search` ranks all types and must NOT require an `issue` role; `pds dedup` needs the role for gating. Split `prepare_corpus` so search can reuse the guard→build→load chain without the directive lookup. Pure refactor — no behavior change, existing tests stay green.

**Files:**
- Modify: `cli/pds-core/src/queries.rs`

- [ ] **Step 1: Refactor**

In `cli/pds-core/src/queries.rs`:

1. Make `CorpusResult` and `issue_directive` `pub(crate)` (they currently have no visibility modifier):

```rust
pub(crate) enum CorpusResult {
```

```rust
pub(crate) fn issue_directive(config: &Config) -> Result<&str, Error> {
```

2. Extract the corpus-only chain from `prepare_corpus` as a new `pub(crate)` function and rewrite `prepare_corpus` on top of it:

```rust
/// Guard the backend, run the non-gating build, and load the produced
/// corpus. The shared preamble for every retrieval/query verb that does not
/// need the issue role (e.g. `pds search`).
pub(crate) fn load_fresh_corpus(
    config: &Config,
    project_root: &Path,
    gh_hint: &str,
) -> Result<CorpusResult, Error> {
    guard_backend(config, gh_hint)?;
    let build = run_build(config, project_root)?;
    if build.is_failed() {
        return Ok(CorpusResult::BuildFailed(build));
    }
    let corpus = NeedsCorpus::load(&config.needs_json)?;
    Ok(CorpusResult::Ready(corpus))
}

/// [`load_fresh_corpus`] plus issue-role resolution — the preamble for verbs
/// that filter or gate on issue-typed needs (`status`, `next`, `dedup`).
pub(crate) fn prepare_corpus(
    config: &Config,
    project_root: &Path,
    gh_hint: &str,
) -> Result<(CorpusResult, String), Error> {
    let directive = issue_directive(config)?.to_string();
    let corpus_result = load_fresh_corpus(config, project_root, gh_hint)?;
    Ok((corpus_result, directive))
}
```

Note the ordering nuance: the original resolved the directive *before* running the build (config errors fire before a slow build). Keep that property — resolve `directive` first, as shown.

3. Update the module doc comment's mention if needed (the "Future verbs (lint, search, dedup, …)" sentence on `prepare_corpus` moves to `load_fresh_corpus` — reword to: "Future corpus verbs call one of these two preambles and add only their own pure query.").

- [ ] **Step 2: Run the full test suite to verify no behavior change**

Run: `cargo test -p pds-core && cargo test -p pds-cli`
Expected: all pass, unchanged counts.

- [ ] **Step 3: Commit**

```bash
git add cli/pds-core/src/queries.rs
git commit -m "refactor(pds-core): split corpus-only preamble from issue-role resolution"
```

---

### Task 6: run_search orchestration + `pds search` CLI verb

**Files:**
- Modify: `cli/pds-core/src/retrieval.rs`
- Modify: `cli/pds-core/src/lib.rs`
- Modify: `cli/pds-cli/src/main.rs`
- Modify: `cli/pds-cli/tests/cli.rs`

- [ ] **Step 1: Write the failing e2e tests**

Append to `cli/pds-cli/tests/cli.rs` (the `backlog_project` helper and `FAKE_SPHINX_BACKLOG` fixture already exist and serve these tests — `roles` can be empty for search):

```rust
// ---------------------------------------------------------------------------
// `pds search` / `pds dedup` E2E — retrieval verbs (unix-only: fake builders).
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn search_ranks_hits_with_engine_and_normalized_scores() {
    // No roles table: search must not require the issue role.
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("search")
        .arg("ready")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "search");
    assert_eq!(json["engine"], "bm25");
    let hits = json["hits"].as_array().expect("hits array");
    assert!(!hits.is_empty(), "\"ready\" appears in two issue titles");
    for h in hits {
        assert!(h["id"].is_string());
        assert!(h["type"].is_string());
        assert!(h["title"].is_string());
        // status may be null (ISSUE_0005 has none) but the key must exist.
        assert!(h.get("status").is_some());
        let score = h["score"].as_f64().expect("score is a number");
        assert!(score > 0.0 && score <= 1.0, "score {score} outside (0, 1]");
    }
    let ids: Vec<&str> = hits.iter().map(|h| h["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"ISSUE_0001"), "got: {ids:?}");
    assert!(ids.contains(&"ISSUE_0004"), "got: {ids:?}");
}

#[cfg(unix)]
#[test]
fn search_with_no_matches_is_clean_empty_hits() {
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("search")
        .arg("zebra astronomy quantum")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "search");
    assert!(json["hits"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn search_empty_query_is_config_error() {
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("search")
        .arg("   ")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "search");
    assert_eq!(json["error"]["kind"], "config");
}

#[cfg(unix)]
#[test]
fn search_github_backend_is_tool_error_naming_gh() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "");

    let assert = pds()
        .arg("search")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "search");
    assert_eq!(json["error"]["kind"], "tool");
    assert!(
        json["error"]["message"].as_str().unwrap().contains("gh search issues"),
        "got: {}",
        json["error"]["message"]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pds-cli search`
Expected: FAIL — clap rejects the unknown `search` subcommand (exit 2 but with a clap error envelope mismatch / non-JSON stdout).

- [ ] **Step 3: Implement run_search and the CLI verb**

In `cli/pds-core/src/retrieval.rs`, add the orchestration section (imports at top of file extend to):

```rust
use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::config::Config;
use crate::error::Error;
use crate::needs::{Need, NeedsCorpus};
use crate::outcome::Outcome;
use crate::queries::{CorpusResult, load_fresh_corpus};
```

then:

```rust
// ---------------------------------------------------------------------------
// Orchestration (guard → build → load → rank → Outcome)
// ---------------------------------------------------------------------------

/// Assemble the shared hits array: `[{id, type, status, title, score}]`
/// (ADR_0021 — `engine` is top-level, not per hit).
fn hits_payload(hits: &[Hit<'_>]) -> Value {
    Value::Array(
        hits.iter()
            .map(|h| {
                json!({
                    "id": h.need.id,
                    "type": h.need.need_type,
                    "status": h.need.status,
                    "title": h.need.title,
                    "score": h.score,
                })
            })
            .collect(),
    )
}

/// `pds search`: rank all need types against the query from a fresh corpus.
/// Pure ranking — no threshold, always a clean outcome (exit 0); exit 2 is
/// reserved for tool/config errors, including an empty query.
pub fn run_search(config: &Config, project_root: &Path, query: &str) -> Result<Outcome, Error> {
    if query.trim().is_empty() {
        return Err(Error::Config {
            message: "search query must not be empty".to_string(),
        });
    }
    let gh_hint = "gh search issues --state all \"<query>\"";
    let corpus = match load_fresh_corpus(config, project_root, gh_hint)? {
        CorpusResult::Ready(c) => c,
        CorpusResult::BuildFailed(failed) => return Ok(failed),
    };
    let index = Index::build(&corpus);
    let hits = index.rank(query);

    let mut payload = Map::new();
    payload.insert("engine".to_string(), Value::String("bm25".to_string()));
    payload.insert("hits".to_string(), hits_payload(&hits));
    Ok(Outcome::clean(payload))
}
```

Update `cli/pds-core/src/lib.rs`'s retrieval export to:

```rust
pub use retrieval::{DEFAULT_THRESHOLD, Hit, Index, dedup_verdict, run_search, tokenize};
```

In `cli/pds-cli/src/main.rs`:

1. Add the variant to `Commands` (after `Next`):

```rust
    /// Rank needs by similarity to a query (always exit 0).
    Search {
        /// The query text.
        query: String,
    },
```

2. Add to `verb()`:

```rust
            Commands::Search { .. } => "search",
```

3. In `run()`, change the dispatch to match by reference and add the arm:

```rust
    match &cli.command {
        Commands::Build => pds_core::run_build(&config, &project.root),
        Commands::Check => pds_core::run_check(&config, &project.root),
        Commands::Lint => pds_core::run_lint(&config, &project.root),
        Commands::Status => pds_core::run_status(&config, &project.root),
        Commands::Next => pds_core::run_next(&config, &project.root),
        Commands::Search { query } => pds_core::run_search(&config, &project.root, query),
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pds-cli && cargo test -p pds-core`
Expected: all pass, including the 4 new search e2e tests.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/retrieval.rs cli/pds-core/src/lib.rs cli/pds-cli/src/main.rs cli/pds-cli/tests/cli.rs
git commit -m "feat(pds): pds search — ranked BM25 retrieval over a fresh corpus (ISSUE_0017)"
```

---

### Task 7: run_dedup orchestration + `pds dedup` CLI verb with --threshold

**Files:**
- Modify: `cli/pds-core/src/retrieval.rs`
- Modify: `cli/pds-core/src/lib.rs`
- Modify: `cli/pds-cli/src/main.rs`
- Modify: `cli/pds-cli/tests/cli.rs`

- [ ] **Step 1: Write the failing e2e tests**

Append to `cli/pds-cli/tests/cli.rs`:

```rust
#[cfg(unix)]
#[test]
fn dedup_near_copy_of_existing_issue_exits_one_duplicate() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    // "first ready" is ISSUE_0001's exact title — a near-copy candidate.
    let assert = pds()
        .arg("dedup")
        .arg("first ready")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).expect("stdout is one JSON object");
    assert_eq!(json["schema"], 1);
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["engine"], "bm25");
    assert_eq!(json["verdict"], "duplicate");
    assert!(json["threshold"].as_f64().is_some());
    let hits = json["hits"].as_array().expect("hits array");
    assert_eq!(hits[0]["id"], "ISSUE_0001");
    assert!(hits[0]["score"].as_f64().unwrap() >= json["threshold"].as_f64().unwrap());
}

#[cfg(unix)]
#[test]
fn dedup_novel_candidate_exits_zero_unique() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("zebra astronomy quantum")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["verdict"], "unique");
    assert!(json["hits"].as_array().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn dedup_non_issue_hit_does_not_gate() {
    // "a feature" is FEAT_0001's exact title: a strong feat-typed hit must
    // NOT flip the verdict (ADR_0021) — exit 0, hits still listed.
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\nfeature = \"feat\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("a feature")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verdict"], "unique");
    let hits = json["hits"].as_array().unwrap();
    assert!(
        hits.iter().any(|h| h["id"] == "FEAT_0001"),
        "the feat hit must still be listed, got: {hits:?}"
    );
}

#[cfg(unix)]
#[test]
fn dedup_threshold_flag_flips_the_verdict() {
    // The candidate "ready zzzqqq" half-matches ISSUE_0001/ISSUE_0004
    // ("ready" matches, "zzzqqq" is unknown): its normalized score lands at
    // ~0.32 — deterministic, well away from the clamp ceiling. The same
    // candidate must be unique at --threshold 0.5 and duplicate at
    // --threshold 0.25, proving the flag governs the gate.
    //
    // NOTE: a single-token query like "ready" is the wrong probe here — the
    // ×3 title weighting saturates tf and clamps the score to 1.0.
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("ready zzzqqq")
        .arg("--threshold")
        .arg("0.5")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.success().code(0).get_output().clone();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verdict"], "unique");
    assert_eq!(json["threshold"], 0.5);
    assert!(!json["hits"].as_array().unwrap().is_empty());

    let assert = pds()
        .arg("dedup")
        .arg("ready zzzqqq")
        .arg("--threshold")
        .arg("0.25")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(1).get_output().clone();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verdict"], "duplicate");
    assert_eq!(json["threshold"], 0.25);
}

#[cfg(unix)]
#[test]
fn dedup_invalid_threshold_flag_is_config_error() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--threshold")
        .arg("1.5")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "config");
    assert!(
        json["error"]["message"].as_str().unwrap().contains("--threshold"),
        "got: {}",
        json["error"]["message"]
    );
}

#[cfg(unix)]
#[test]
fn dedup_empty_candidate_is_config_error() {
    let (_tmp, config) = backlog_project("", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("  ")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "config");
}

#[cfg(unix)]
#[test]
fn dedup_missing_issue_role_is_config_error() {
    // dedup gates on issue-typed hits, so it needs the issue role.
    let (_tmp, config) = backlog_project("", "");

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "config");
}

#[cfg(unix)]
#[test]
fn dedup_github_backend_is_tool_error() {
    let (_tmp, config) = backlog_project("issue_backend = \"github\"\n", "issue = \"issue\"\n");

    let assert = pds()
        .arg("dedup")
        .arg("anything")
        .arg("--config")
        .arg(&config)
        .assert();
    let out = assert.failure().code(2).get_output().clone();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["verb"], "dedup");
    assert_eq!(json["error"]["kind"], "tool");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pds-cli dedup`
Expected: FAIL — clap rejects the unknown `dedup` subcommand.

- [ ] **Step 3: Implement run_dedup and the CLI verb**

In `cli/pds-core/src/retrieval.rs`, extend the queries import to include `prepare_corpus`:

```rust
use crate::queries::{CorpusResult, load_fresh_corpus, prepare_corpus};
```

and add after `run_search`:

```rust
/// `pds dedup`: rank all need types against the candidate text, then gate —
/// verdict `"duplicate"` (failed outcome, exit 1) iff an issue-typed hit
/// reaches the threshold (ADR_0021). `threshold_flag` is the `--threshold`
/// CLI override; `None` falls back to the configured value.
pub fn run_dedup(
    config: &Config,
    project_root: &Path,
    candidate: &str,
    threshold_flag: Option<f64>,
) -> Result<Outcome, Error> {
    if candidate.trim().is_empty() {
        return Err(Error::Config {
            message: "dedup candidate text must not be empty".to_string(),
        });
    }
    let threshold = match threshold_flag {
        Some(t) => {
            crate::config::validate_threshold(t, "--threshold")?;
            t
        }
        None => config.dedup_threshold,
    };
    let gh_hint = "gh search issues --state all \"<candidate>\"";
    let (corpus_result, directive) = prepare_corpus(config, project_root, gh_hint)?;
    let corpus = match corpus_result {
        CorpusResult::Ready(c) => c,
        CorpusResult::BuildFailed(failed) => return Ok(failed),
    };
    let index = Index::build(&corpus);
    let hits = index.rank(candidate);
    let verdict = dedup_verdict(&hits, &directive, threshold);

    let mut payload = Map::new();
    payload.insert("engine".to_string(), Value::String("bm25".to_string()));
    payload.insert("threshold".to_string(), json!(threshold));
    payload.insert("verdict".to_string(), Value::String(verdict.to_string()));
    payload.insert("hits".to_string(), hits_payload(&hits));
    if verdict == "duplicate" {
        Ok(Outcome::failed(payload))
    } else {
        Ok(Outcome::clean(payload))
    }
}
```

Update the lib.rs retrieval export to its final form:

```rust
pub use retrieval::{DEFAULT_THRESHOLD, Hit, Index, dedup_verdict, run_dedup, run_search, tokenize};
```

In `cli/pds-cli/src/main.rs`:

1. Add the variant after `Search`:

```rust
    /// Pre-filing duplicate gate: exit 1 when an issue-typed hit reaches the threshold.
    Dedup {
        /// The candidate issue text (title plus body draft).
        candidate: String,
        /// Override the configured similarity threshold (a ratio in (0, 1]).
        #[arg(long, value_name = "RATIO")]
        threshold: Option<f64>,
    },
```

2. Add to `verb()`:

```rust
            Commands::Dedup { .. } => "dedup",
```

3. Add the dispatch arm:

```rust
        Commands::Dedup {
            candidate,
            threshold,
        } => pds_core::run_dedup(&config, &project.root, candidate, *threshold),
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pds-cli && cargo test -p pds-core`
Expected: all pass, including the 8 new dedup e2e tests.

- [ ] **Step 5: Commit**

```bash
git add cli/pds-core/src/retrieval.rs cli/pds-core/src/lib.rs cli/pds-cli/src/main.rs cli/pds-cli/tests/cli.rs
git commit -m "feat(pds): pds dedup — normalized-score pre-filing gate with --threshold (ISSUE_0017)"
```

---

### Task 8: Dogfood, tune the default threshold, docs, full gates

**Files:**
- Modify: `ubproject.toml` (repo root)
- Modify: `CLAUDE.md` (repo root)
- Possibly modify: `cli/pds-core/src/retrieval.rs` (`DEFAULT_THRESHOLD`)

- [ ] **Step 1: Dogfood against this repo's corpus**

From the repo root, run and read the ranked output of each:

```bash
cd cli && cargo run -q -p pds-cli -- search "lint gate" --config ../ubproject.toml
cargo run -q -p pds-cli -- dedup "pds dedup and pds search retrieval over the corpus" --config ../ubproject.toml; echo "exit: $?"
cargo run -q -p pds-cli -- dedup "support emoji reactions on glossary terms" --config ../ubproject.toml; echo "exit: $?"
```

Expected: search ranks gate-related needs (ADR_0007/ADR_0017 territory); the near-copy of ISSUE_0017's title exits 1 with `verdict: duplicate` and ISSUE_0017 as the top hit; the nonsense candidate exits 0 with `verdict: unique`.

- [ ] **Step 2: Tune the default threshold by eyeballing**

Run 3–4 more realistic candidates (paraphrases of existing issues, plausible-but-novel feature ideas) and check the scores. Pick the default that separates "paraphrase of an existing issue" (should gate) from "related but distinct work" (should not). If 0.35 misjudges any probe, adjust `DEFAULT_THRESHOLD` in `cli/pds-core/src/retrieval.rs` and re-run `cargo test -p pds-core` (the config default test compares against the constant, so it stays green).

- [ ] **Step 3: Dogfood the config table**

Add to `ubproject.toml` (repo root), after the `[tool.patdhlk-skills.lint.nontrivial_body]` table:

```toml
# Dedup gate (pds dedup): an issue-typed hit at or above this normalized
# similarity ratio (ADR_0021) blocks filing with exit 1. Tuned by probing
# this corpus with paraphrased and novel candidates (ISSUE_0017).
[tool.patdhlk-skills.dedup]
threshold = 0.35
```

(Use the tuned value from Step 2 if it changed.)

- [ ] **Step 4: Update CLAUDE.md**

In `CLAUDE.md`, extend the backlog-verbs bullet. Replace:

```markdown
- Backlog queries have dedicated verbs (each rebuilds needs.json first):
  `pds status` = per-status issue counts; `pds next` = the lowest-ID
  `ready-for-agent` issue (`{"issue": null, "reason": "none-ready"}` when
  the backlog is clean). Ad-hoc reads stay `jq`.
```

with:

```markdown
- Backlog queries have dedicated verbs (each rebuilds needs.json first):
  `pds status` = per-status issue counts; `pds next` = the lowest-ID
  `ready-for-agent` issue (`{"issue": null, "reason": "none-ready"}` when
  the backlog is clean); `pds search "<query>"` = BM25-ranked hits over
  all need types (always exit 0); `pds dedup "<candidate>"` = the same
  ranking as a pre-filing gate — exit 1 when an issue-typed hit reaches
  the threshold (ADR_0021; `[tool.patdhlk-skills.dedup]` /
  `--threshold`). Ad-hoc reads stay `jq`.
```

- [ ] **Step 5: Run every gate**

```bash
cd cli && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && make strict
```

Expected: all green. If `cargo fmt --check` fails, run `cargo fmt` and re-check.

- [ ] **Step 6: Flip ISSUE_0017 to in-progress → done**

In `spec/issues/index.rst`, change ISSUE_0017's `:status: ready-for-agent` to `:status: done` (the implementation is complete and gated). Re-run `make strict`.

Note: ISSUE_0021 (skill wiring) stays `needs-triage` — explicitly out of scope.

- [ ] **Step 7: Commit**

```bash
git add ubproject.toml CLAUDE.md spec/issues/index.rst cli/pds-core/src/retrieval.rs
git commit -m "feat(pds): dogfood dedup threshold, document retrieval verbs; ISSUE_0017 -> done"
```

---

## Self-review notes

- **Spec coverage:** brief's desired behaviors map: shared preamble (Task 5/6/7), all-types ranking (Task 2), issue-only gating (Task 3/7), JSON shape + cap (Task 6/7 + e2e), hand-rolled BM25 constants (Task 2), tokenization + ×3/×2/×1 (Task 1/2), self-score ratio (Task 2), config + flag (Task 4/7), degenerate cases (Task 2 tests + e2e), dogfood criteria + CLAUDE.md (Task 8). Out-of-scope items are absent by construction.
- **Type consistency:** `Hit<'a>`, `Index<'a>`, `dedup_verdict`, `run_search(config, root, query)`, `run_dedup(config, root, candidate, threshold_flag)`, `validate_threshold(value, key)`, `load_fresh_corpus` / `prepare_corpus` — names used identically across tasks.
- **Ordering hazard:** Task 4 references `crate::retrieval::DEFAULT_THRESHOLD`, defined in the same task (Step 3 adds it before config uses it). Task 5 must land before Tasks 6/7 (they import `load_fresh_corpus`/`prepare_corpus`).
- **The `dedup_threshold_flag_flips_the_verdict` e2e** uses a half-known candidate ("ready zzzqqq") whose normalized score computes to ~0.32 against the FAKE_SPHINX_BACKLOG corpus (idf_ready = ln 2.8, idf_unknown = ln 14, avg_len = 5.5): unique at 0.5, duplicate at 0.25, both with comfortable margins and fully deterministic. Single-token queries are unusable as partial-match probes — title weighting saturates tf and clamps them to 1.0 (the `dedup_near_copy` test exploits exactly that).
