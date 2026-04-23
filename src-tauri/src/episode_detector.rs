/// Episode Detector — Phase 0b. Dual-mode detection.
/// Precise mode: Jaccard similarity over title token sets.
/// Broad mode:   category grouping fallback when precision fails.
/// Independent module. Does NOT extend grouper.rs (R12 active).
/// No network, no LLM, no persistent state (D1, D8, D9).

use std::collections::{HashMap, HashSet};
use serde::Serialize;
use uuid::Uuid;

use crate::session_builder::{Session, SessionResource};

/// Minimum avg pairwise Jaccard for a cluster to be "precise".
const JACCARD_THRESHOLD: f64 = 0.20;
/// Minimum cluster size for precise mode.
const PRECISE_MIN: usize = 3;
/// Minimum group size for broad mode.
const BROAD_MIN: usize = 3;

#[derive(Debug, Serialize, Clone, PartialEq)]
#[serde(rename_all = "PascalCase")]
pub enum DetectionMode {
    Precise,
    Broad,
}

/// An actionable episode: a coherent group of resources within a session.
#[derive(Debug, Serialize)]
pub struct Episode {
    pub episode_id: String,
    /// Dominant keyword or category label.
    pub label: String,
    pub resources: Vec<SessionResource>,
    pub mode: DetectionMode,
    /// Average pairwise Jaccard (Precise) or 1.0 (Broad).
    pub coherence: f64,
}

/// Detect episodes within a session.
/// Returns an empty vec if no actionable group is found.
/// Tries precise mode first; falls back to broad mode.
pub fn detect(session: &Session) -> Vec<Episode> {
    let precise = detect_precise(&session.resources);
    if !precise.is_empty() {
        return precise;
    }
    detect_broad(&session.resources)
}

// ── Precise mode (Jaccard) ────────────────────────────────────────────────────

fn detect_precise(resources: &[SessionResource]) -> Vec<Episode> {
    if resources.len() < PRECISE_MIN {
        return Vec::new();
    }

    let tokenized: Vec<Vec<String>> = resources.iter().map(|r| tokenize(&r.title)).collect();
    let n = resources.len();
    let mut assigned = vec![false; n];
    let mut episodes = Vec::new();

    for seed in 0..n {
        if assigned[seed] || tokenized[seed].is_empty() {
            continue;
        }

        let mut group = vec![seed];
        let mut jaccard_sum = 0.0f64;
        let mut pairs = 0usize;

        for other in (seed + 1)..n {
            if assigned[other] {
                continue;
            }
            let j = jaccard(&tokenized[seed], &tokenized[other]);
            if j >= JACCARD_THRESHOLD {
                group.push(other);
                jaccard_sum += j;
                pairs += 1;
            }
        }

        if group.len() < PRECISE_MIN {
            continue;
        }

        let coherence = if pairs > 0 { jaccard_sum / pairs as f64 } else { JACCARD_THRESHOLD };
        for &i in &group {
            assigned[i] = true;
        }

        let group_resources: Vec<SessionResource> = group.iter().map(|&i| resources[i].clone()).collect();
        let label = dominant_token(&group_resources).unwrap_or_else(|| "mixed".into());

        episodes.push(Episode {
            episode_id: Uuid::new_v4().to_string(),
            label,
            resources: group_resources,
            mode: DetectionMode::Precise,
            coherence,
        });
    }

    episodes
}

// ── Broad mode (category fallback) ───────────────────────────────────────────

fn detect_broad(resources: &[SessionResource]) -> Vec<Episode> {
    let mut by_category: HashMap<String, Vec<SessionResource>> = HashMap::new();
    for r in resources {
        by_category.entry(r.category.clone()).or_default().push(r.clone());
    }

    let mut episodes: Vec<Episode> = by_category
        .into_iter()
        .filter(|(_, g)| g.len() >= BROAD_MIN)
        .map(|(category, group_resources)| Episode {
            episode_id: Uuid::new_v4().to_string(),
            label: category,
            resources: group_resources,
            mode: DetectionMode::Broad,
            coherence: 1.0,
        })
        .collect();

    // Stable order: most resources first
    episodes.sort_by(|a, b| b.resources.len().cmp(&a.resources.len()));
    episodes
}

// ── Utilities ─────────────────────────────────────────────────────────────────

fn jaccard(a: &[String], b: &[String]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let set_a: HashSet<&String> = a.iter().collect();
    let set_b: HashSet<&String> = b.iter().collect();
    let inter = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    inter as f64 / union as f64
}

fn dominant_token(resources: &[SessionResource]) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for r in resources {
        for tok in tokenize(&r.title) {
            *counts.entry(tok).or_insert(0) += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, c)| *c).map(|(tok, _)| tok)
}

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
        .filter(|t| !STOPWORDS.contains(&t.as_str()) && !t.chars().all(|c| c.is_ascii_digit()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_res(title: &str, category: &str) -> SessionResource {
        SessionResource {
            uuid: Uuid::new_v4().to_string(),
            title: title.into(),
            domain: "example.com".into(),
            category: category.into(),
            captured_at: 0,
        }
    }

    fn make_session(resources: Vec<SessionResource>) -> Session {
        Session {
            session_id: Uuid::new_v4().to_string(),
            window_start: 0,
            window_end: 0,
            is_bootstrap: false,
            resources,
        }
    }

    #[test]
    fn precise_mode_detects_shared_token_cluster() {
        let session = make_session(vec![
            make_res("React hooks tutorial", "development"),
            make_res("React component patterns", "development"),
            make_res("React state management", "development"),
            make_res("Vue.js getting started", "development"),
        ]);
        let episodes = detect(&session);
        assert!(!episodes.is_empty());
        let ep = &episodes[0];
        assert_eq!(ep.mode, DetectionMode::Precise);
        assert_eq!(ep.resources.len(), 3);
        assert_eq!(ep.label, "react");
    }

    #[test]
    fn broad_mode_fallback_on_category() {
        let session = make_session(vec![
            make_res("Python beginner guide", "development"),
            make_res("Rust ownership explained", "development"),
            make_res("Go concurrency patterns", "development"),
            make_res("Recipe for pasta", "lifestyle"),
        ]);
        let episodes = detect(&session);
        assert!(!episodes.is_empty());
        let ep = &episodes[0];
        assert_eq!(ep.mode, DetectionMode::Broad);
        assert_eq!(ep.resources.len(), 3);
        assert_eq!(ep.label, "development");
    }

    #[test]
    fn empty_session_yields_no_episodes() {
        let session = make_session(vec![]);
        assert!(detect(&session).is_empty());
    }

    #[test]
    fn small_session_below_threshold_yields_no_episodes() {
        let session = make_session(vec![
            make_res("React tutorial", "development"),
            make_res("Vue guide", "development"),
        ]);
        // Below PRECISE_MIN=3 and BROAD_MIN=3
        assert!(detect(&session).is_empty());
    }
}
