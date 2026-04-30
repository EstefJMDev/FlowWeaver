/// Basic Similarity Grouper — T-0a-004.
/// Groups resources by domain+category (level 1) then by shared title tokens
/// (level 2, greedy, no Jaccard, no embeddings, no LLM).
/// Stateless: each call processes the full resource set; no memory between runs.

use std::collections::HashMap;
use serde::Serialize;
use crate::{crypto, storage::Db};

#[derive(Debug, Serialize, Clone)]
pub struct ClusterResource {
    pub uuid: String,
    pub title: String,   // decrypted for display
    pub domain: String,
    pub category: String,
}

/// A cluster of resources sharing domain+category, optionally refined by title tokens.
#[derive(Debug, Serialize)]
pub struct Cluster {
    /// Unique key: "category/domain" or "category/domain/sub_label"
    pub group_key: String,
    pub domain: String,
    pub category: String,
    /// Dominant shared token(s) within the sub-group; empty string if no level-2 split.
    pub sub_label: String,
    pub resources: Vec<ClusterResource>,
}

/// Produce clusters from the current SQLCipher state. No writes, no state persisted.
pub fn group(db: &Db, key: &str) -> Result<Vec<Cluster>, String> {
    let rows = db.all_resources().map_err(|e| e.to_string())?;

    // Decrypt titles; fall back to domain if decryption fails.
    let resources: Vec<ClusterResource> = rows
        .into_iter()
        .map(|r| ClusterResource {
            uuid: r.uuid,
            title: crypto::decrypt(&r.title, key).unwrap_or_else(|| r.domain.clone()),
            domain: r.domain,
            category: r.category,
        })
        .collect();

    // Level 1: group by category/domain.
    let mut l1: HashMap<String, Vec<ClusterResource>> = HashMap::new();
    for res in resources {
        let gk = format!("{}/{}", res.category, res.domain);
        l1.entry(gk).or_default().push(res);
    }

    // Stable ordering: sort group keys alphabetically.
    let mut keys: Vec<String> = l1.keys().cloned().collect();
    keys.sort();

    let mut clusters: Vec<Cluster> = Vec::new();
    for gk in keys {
        let group_resources = l1.remove(&gk).unwrap();
        let (category, domain) = split_group_key(&gk);

        // Level 2: token sub-grouping within the level-1 group.
        let sub_groups = sub_group_by_tokens(group_resources);
        for (sub_label, resources) in sub_groups {
            let cluster_key = if sub_label.is_empty() {
                gk.clone()
            } else {
                format!("{gk}/{sub_label}")
            };
            clusters.push(Cluster {
                group_key: cluster_key,
                domain: domain.to_string(),
                category: category.to_string(),
                sub_label,
                resources,
            });
        }
    }

    Ok(clusters)
}

fn split_group_key(key: &str) -> (&str, &str) {
    if let Some(pos) = key.find('/') {
        (&key[..pos], &key[pos + 1..])
    } else {
        (key, "")
    }
}

// ── Level-2 sub-grouping ──────────────────────────────────────────────────────

/// Split a group of resources into sub-groups by shared title tokens.
/// Returns a list of (sub_label, resources) pairs.
/// If no meaningful split is found, returns a single pair with sub_label = "".
fn sub_group_by_tokens(resources: Vec<ClusterResource>) -> Vec<(String, Vec<ClusterResource>)> {
    // Not enough resources for sub-grouping to be meaningful.
    if resources.len() < 3 {
        return vec![("".to_string(), resources)];
    }

    let tokenized: Vec<Vec<String>> = resources.iter().map(|r| tokenize(&r.title)).collect();

    // token → resource indices that contain it
    let mut coverage: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, tokens) in tokenized.iter().enumerate() {
        for tok in tokens {
            coverage.entry(tok.clone()).or_default().push(i);
        }
    }

    // Shared tokens: appear in ≥2 resources but in ≤80% (avoids trivial splits).
    let max_cover = std::cmp::max(2, (resources.len() * 4) / 5);
    let mut shared: Vec<(String, Vec<usize>)> = coverage
        .into_iter()
        .filter(|(_, idx)| idx.len() >= 2 && idx.len() <= max_cover)
        .collect();

    if shared.is_empty() {
        return vec![("".to_string(), resources)];
    }

    // Sort by coverage descending — most common shared token first.
    shared.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));

    // Greedy assignment: assign each resource to the first shared token that covers it.
    let n = resources.len();
    let mut assignment: Vec<Option<usize>> = vec![None; n];
    let mut sub_labels: Vec<String> = Vec::new();

    for (token, indices) in &shared {
        let unassigned: Vec<usize> = indices.iter().copied().filter(|&i| assignment[i].is_none()).collect();
        if unassigned.len() >= 2 {
            let sg = sub_labels.len();
            sub_labels.push(token.clone());
            for i in unassigned {
                assignment[i] = Some(sg);
            }
        }
    }

    // Build result buckets.
    let mut buckets: Vec<Vec<ClusterResource>> = vec![Vec::new(); sub_labels.len()];
    let mut misc: Vec<ClusterResource> = Vec::new();

    for (i, res) in resources.into_iter().enumerate() {
        match assignment[i] {
            Some(sg) => buckets[sg].push(res),
            None => misc.push(res),
        }
    }

    let mut result: Vec<(String, Vec<ClusterResource>)> = sub_labels
        .into_iter()
        .zip(buckets)
        .filter(|(_, r)| !r.is_empty())
        .collect();

    if !misc.is_empty() {
        result.push(("".to_string(), misc));
    }

    // If all resources ended up in one bucket, discard the sub-grouping.
    if result.len() <= 1 {
        let merged = result.into_iter().flat_map(|(_, r)| r).collect();
        return vec![("".to_string(), merged)];
    }

    result
}

// ── Tokenizer ─────────────────────────────────────────────────────────────────

fn tokenize(title: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "are", "but", "not", "you", "all",
        "can", "has", "her", "was", "one", "our", "out", "day",
        "get", "how", "its", "let", "now", "old", "see", "two",
        "way", "who", "ask", "him", "his", "did", "yes", "off",
        "ago", "won", "use", "new", "may", "able", "about", "after",
        "also", "back", "been", "both", "come", "does", "each",
        "even", "from", "gave", "give", "going", "good", "have",
        "here", "into", "just", "know", "like", "made", "make",
        "more", "most", "much", "must", "need", "next", "only",
        "other", "over", "part", "same", "some", "such", "take",
        "than", "that", "them", "then", "there", "these", "they",
        "this", "time", "very", "want", "well", "were", "what",
        "when", "will", "with", "your",
    ];

    title
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_lowercase())
        .filter(|t| {
            !STOPWORDS.contains(&t.as_str())
                && !t.chars().all(|c| c.is_ascii_digit())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize() {
        let tokens = tokenize("React hooks tutorial for beginners");
        assert!(tokens.contains(&"react".to_string()));
        assert!(tokens.contains(&"hooks".to_string()));
        assert!(tokens.contains(&"tutorial".to_string()));
        assert!(!tokens.contains(&"for".to_string())); // stopword
    }

    #[test]
    fn test_sub_group_few_resources() {
        let resources = vec![
            make_res("React tutorial"),
            make_res("Vue guide"),
        ];
        let groups = sub_group_by_tokens(resources);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].0, "");
    }

    #[test]
    fn test_sub_group_shared_token() {
        // "guide" is absent from resource 1 to avoid cross-group token collision
        // that would make "guide" alphabetically outrank "react" in the greedy pass.
        let resources = vec![
            make_res("React hooks tutorial"),
            make_res("React component patterns"),
            make_res("Vue.js getting started"),
            make_res("Vue.js migration path"),
            make_res("Rust ownership explained"),
        ];
        let groups = sub_group_by_tokens(resources);
        // Should split into at least react + vue sub-groups
        assert!(groups.len() >= 2);
        let react_group = groups.iter().find(|(label, _)| label == "react");
        assert!(react_group.is_some());
        assert_eq!(react_group.unwrap().1.len(), 2);
    }

    fn make_res(title: &str) -> ClusterResource {
        ClusterResource {
            uuid: uuid::Uuid::new_v4().to_string(),
            title: title.to_string(),
            domain: "github.com".to_string(),
            category: "desarrollo".to_string(),
        }
    }
}
