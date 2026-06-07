//! Mechanical ADR similarity — the deterministic engine behind `related` and
//! `dedupe`. TF-IDF cosine over the corpus: no AI, no network, no embeddings
//! (the semantic/embeddings upgrade is future work). Pure + unit-tested.

use std::collections::HashMap;

/// One ADR in the corpus to rank.
pub struct Doc {
    /// Routing token (e.g. `"1"` / a slug) — the stable id used to find the target
    /// and to exclude already-linked ADRs.
    pub id: String,
    /// Display reference (e.g. `"ADR-0006"`).
    pub reference: String,
    pub title: String,
    /// The text compared (title + body).
    pub text: String,
}

/// A ranked similarity result.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "manifest", derive(schemars::JsonSchema))]
pub struct Match {
    pub reference: String,
    pub title: String,
    /// Cosine similarity in `(0, 1]`.
    pub score: f64,
    /// Routing token (so a caller can filter by it); not serialized.
    #[serde(skip)]
    pub id: String,
}

/// Very common words that shouldn't drive similarity (English + ADR boilerplate).
const STOPWORDS: &[&str] = &[
    "the",
    "a",
    "an",
    "and",
    "or",
    "but",
    "of",
    "to",
    "in",
    "on",
    "for",
    "with",
    "is",
    "are",
    "be",
    "this",
    "that",
    "it",
    "as",
    "by",
    "we",
    "our",
    "you",
    "your",
    "not",
    "at",
    "from",
    "adr",
    "decision",
    "status",
    "proposed",
    "accepted",
    "context",
    "options",
    "consequences",
];

fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3 && !STOPWORDS.contains(t))
        .map(str::to_string)
        .collect()
}

/// Rank every doc by TF-IDF cosine similarity to `target_id`, most similar first
/// (the target itself and zero-overlap docs are excluded). Empty if the target
/// isn't present or the corpus is too small.
pub fn rank(docs: &[Doc], target_id: &str) -> Vec<Match> {
    let n = docs.len() as f64;
    if docs.len() < 2 {
        return Vec::new();
    }

    // Term frequencies per doc, and document frequency across the corpus.
    let tfs: Vec<HashMap<String, f64>> = docs
        .iter()
        .map(|d| {
            let mut m = HashMap::new();
            for t in tokenize(&d.text) {
                *m.entry(t).or_insert(0.0) += 1.0;
            }
            m
        })
        .collect();
    let mut df: HashMap<&str, f64> = HashMap::new();
    for tf in &tfs {
        for k in tf.keys() {
            *df.entry(k).or_insert(0.0) += 1.0;
        }
    }
    let idf = |t: &str| (n / df.get(t).copied().unwrap_or(1.0)).ln() + 1.0;

    // TF-IDF vectors + their norms.
    let vecs: Vec<HashMap<String, f64>> = tfs
        .iter()
        .map(|tf| tf.iter().map(|(t, &c)| (t.clone(), c * idf(t))).collect())
        .collect();
    let norms: Vec<f64> = vecs.iter().map(norm).collect();

    let Some(ti) = docs.iter().position(|d| d.id == target_id) else {
        return Vec::new();
    };

    let mut out: Vec<Match> = Vec::new();
    for (i, d) in docs.iter().enumerate() {
        if i == ti {
            continue;
        }
        let s = cosine(&vecs[ti], norms[ti], &vecs[i], norms[i]);
        if s > 0.0 {
            out.push(Match {
                reference: d.reference.clone(),
                title: d.title.clone(),
                score: s,
                id: d.id.clone(),
            });
        }
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out
}

fn norm(v: &HashMap<String, f64>) -> f64 {
    v.values().map(|x| x * x).sum::<f64>().sqrt()
}

fn cosine(a: &HashMap<String, f64>, anorm: f64, b: &HashMap<String, f64>, bnorm: f64) -> f64 {
    if anorm == 0.0 || bnorm == 0.0 {
        return 0.0;
    }
    let dot: f64 = a
        .iter()
        .map(|(k, &va)| va * b.get(k).copied().unwrap_or(0.0))
        .sum();
    dot / (anorm * bnorm)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, title: &str, text: &str) -> Doc {
        Doc {
            id: id.into(),
            reference: format!("ADR-{id}"),
            title: title.into(),
            text: text.into(),
        }
    }

    #[test]
    fn ranks_topically_similar_adrs_first() {
        let docs = vec![
            doc(
                "1",
                "Postgres",
                "adopt postgresql relational database for primary datastore storage",
            ),
            doc(
                "2",
                "Redis cache",
                "use redis caching layer database for hot key lookups",
            ),
            doc(
                "3",
                "Frontend framework",
                "choose vue react svelte for the browser dashboard ui",
            ),
        ];
        let r = rank(&docs, "1");
        assert!(!r.is_empty());
        // The other database ADR should outrank the frontend one.
        assert_eq!(r[0].reference, "ADR-2");
        assert!(r[0].score > 0.0);
        if r.len() > 1 {
            assert!(r[0].score >= r[1].score);
        }
    }

    #[test]
    fn unknown_target_or_tiny_corpus_is_empty() {
        let docs = vec![doc("1", "Only one", "single document corpus")];
        assert!(rank(&docs, "1").is_empty());
        let two = vec![doc("1", "A", "alpha beta"), doc("2", "B", "gamma delta")];
        assert!(rank(&two, "99").is_empty());
    }
}
