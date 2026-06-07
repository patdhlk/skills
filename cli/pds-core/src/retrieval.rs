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
#[allow(dead_code)] // documents the weight for parity with TITLE_WEIGHT / TAGS_WEIGHT
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
            // CONTENT_WEIGHT = 1: direct append, no repetition.
            tokens.extend(tokenize(&need.content));
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

    /// Rank the corpus against `query`, returning normalized hits sorted by
    /// descending score (id ascending on ties), capped at [`MAX_HITS`].
    /// Zero-scoring documents are omitted. Empty token streams (empty query,
    /// empty corpus) return no hits.
    pub fn rank(&self, query: &str) -> Vec<Hit<'a>> {
        let query_tokens = tokenize(query);
        // Also bail when avg_len is zero: an all-empty-text corpus would
        // otherwise divide by zero inside raw_score and NaN-poison every score.
        if query_tokens.is_empty() || self.docs.is_empty() || self.avg_len <= 0.0 {
            return Vec::new();
        }
        let query_tf = term_frequencies(&query_tokens);
        // The query scored as if it were an average-length document: this is
        // the length-neutral maximum raw score, used as the normalization
        // denominator (ADR_0021).  Using avg_len here (rather than
        // query_tokens.len()) eliminates the length-bonus that a very short
        // query would otherwise receive, ensuring that a document whose term
        // frequencies are ≥ the query's *and whose length is ≤ avg_len* will
        // always score ≥ self_score and clamp to 1.0.  Always > 0 when the
        // query has at least one token.
        let self_score = self.raw_score(&query_tf, self.avg_len, &query_tf);
        if self_score <= 0.0 {
            return Vec::new();
        }
        let mut hits: Vec<Hit<'a>> = self
            .docs
            .iter()
            .filter_map(|doc| {
                let raw = self.raw_score(&doc.tf, doc.len as f64, &query_tf);
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
        doc_len: f64,
        query_tf: &HashMap<String, usize>,
    ) -> f64 {
        let mut score = 0.0;
        for term in query_tf.keys() {
            let f = doc_tf.get(term).copied().unwrap_or(0) as f64;
            if f == 0.0 {
                continue;
            }
            let denom = f + K1 * (1.0 - B + B * doc_len / self.avg_len);
            score += self.idf(term) * (f * (K1 + 1.0)) / denom;
        }
        score
    }
}

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
        // The weighted document has higher tf than the query (title tokens
        // repeated ×3), so the raw score exceeds the self-score and the
        // clamp lands it at exactly 1.0.
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
        let corpus =
            corpus_from(r#"{"current_version":"","project":"t","versions":{"":{"needs":{}}}}"#);
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
    fn single_doc_corpus_scores_nonzero() {
        let corpus = corpus_from(
            r#"{"current_version":"","project":"t","versions":{"":{"needs":{
            "ISSUE_0001":{"id":"ISSUE_0001","type":"issue","title":"lone entry"}
        }}}}"#,
        );
        let index = Index::build(&corpus);
        let hits = index.rank("lone entry");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].score > 0.0 && hits[0].score <= 1.0);
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
}
