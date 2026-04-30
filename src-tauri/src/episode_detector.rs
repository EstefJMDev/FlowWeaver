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

    let tokenized: Vec<Vec<String>> = resources.iter().map(|r| tokenize_resource(r)).collect();
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

/// Combine title tokens (×2 for weight), domain stem, and URL path tokens.
/// Title is doubled so Jaccard favours title similarity over URL similarity (D8).
fn tokenize_resource(r: &SessionResource) -> Vec<String> {
    let title_tokens = tokenize(&r.title);
    // Duplicate title tokens — they contribute double weight in Jaccard
    let mut tokens: Vec<String> = title_tokens.clone();
    tokens.extend(title_tokens);
    // Domain stem (e.g. "imdb" from "imdb.com", "steampowered" from "store.steampowered.com")
    if let Some(stem) = domain_stem(&r.domain) {
        if stem.len() >= 3 {
            tokens.push(stem);
        }
    }
    tokens.extend(extract_url_tokens(&r.url));
    tokens
}

/// Extract meaningful tokens from a URL path+query string.
/// Skips scheme, host, and common noise tokens. No network access (D8).
fn extract_url_tokens(url: &str) -> Vec<String> {
    if url.is_empty() {
        return Vec::new();
    }
    // Skip scheme + host; keep everything from the first '/' of the path onward
    let path = if let Some(p) = url.find("://") {
        let after_scheme = &url[p + 3..];
        after_scheme.find('/').map(|i| &after_scheme[i..]).unwrap_or("")
    } else {
        url
    };
    const NOISE: &[&str] = &["www", "com", "html", "php", "htm", "asp", "aspx", "jsp", "org", "net"];
    path.split(|c| matches!(c, '/' | '?' | '&' | '=' | '#'))
        .filter(|t| t.len() >= 3)
        .map(|t| t.to_lowercase())
        .filter(|t| {
            !NOISE.contains(&t.as_str())
                && !t.starts_with("utm_")
                && !t.starts_with("fbclid")
                && !t.starts_with("gclid")
                && !t.chars().all(|c| c.is_ascii_digit())
        })
        .collect()
}

/// Return the registrable label of a domain without TLD.
/// "imdb.com" → "imdb", "store.steampowered.com" → "steampowered".
fn domain_stem(domain: &str) -> Option<String> {
    let parts: Vec<&str> = domain.split('.').collect();
    match parts.len() {
        0 => None,
        1 => Some(parts[0].to_lowercase()),
        _ => Some(parts[parts.len() - 2].to_lowercase()),
    }
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
            url: String::new(),
            captured_at: 0,
        }
    }

    fn make_res_with_url(title: &str, domain: &str, url: &str, category: &str) -> SessionResource {
        SessionResource {
            uuid: Uuid::new_v4().to_string(),
            title: title.into(),
            domain: domain.into(),
            category: category.into(),
            url: url.into(),
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
            make_res("React hooks tutorial", "desarrollo"),
            make_res("React component patterns", "desarrollo"),
            make_res("React state management", "desarrollo"),
            make_res("Vue.js getting started", "desarrollo"),
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
            make_res("Python beginner guide", "desarrollo"),
            make_res("Rust ownership explained", "desarrollo"),
            make_res("Go concurrency patterns", "desarrollo"),
            make_res("Recipe for pasta", "lifestyle"),
        ]);
        let episodes = detect(&session);
        assert!(!episodes.is_empty());
        let ep = &episodes[0];
        assert_eq!(ep.mode, DetectionMode::Broad);
        assert_eq!(ep.resources.len(), 3);
        assert_eq!(ep.label, "desarrollo");
    }

    #[test]
    fn empty_session_yields_no_episodes() {
        let session = make_session(vec![]);
        assert!(detect(&session).is_empty());
    }

    #[test]
    fn small_session_below_threshold_yields_no_episodes() {
        let session = make_session(vec![
            make_res("React tutorial", "desarrollo"),
            make_res("Vue guide", "desarrollo"),
        ]);
        // Below PRECISE_MIN=3 and BROAD_MIN=3
        assert!(detect(&session).is_empty());
    }

    /// H-003: resources with empty titles but URLs from the same domain and similar path
    /// must group in Precise mode via domain stem + path tokens.
    #[test]
    fn url_tokens_group_no_title_resources() {
        let session = make_session(vec![
            make_res_with_url("", "imdb.com", "https://imdb.com/title/tt0111161/", "entretenimiento"),
            make_res_with_url("", "imdb.com", "https://imdb.com/title/tt0068646/", "entretenimiento"),
            make_res_with_url("", "imdb.com", "https://imdb.com/title/tt0071562/", "entretenimiento"),
        ]);
        let episodes = detect(&session);
        assert!(!episodes.is_empty(), "should detect at least one episode");
        let ep = &episodes[0];
        assert_eq!(ep.mode, DetectionMode::Precise, "should be Precise mode via URL tokens");
        assert_eq!(ep.resources.len(), 3);
    }

    #[test]
    fn url_tokens_extract_path_segments() {
        let tokens = extract_url_tokens("https://github.com/rust-lang/rust/issues/123");
        assert!(tokens.contains(&"rust".to_string()) || tokens.contains(&"rust-lang".to_string()),
            "expected path segment tokens, got: {tokens:?}");
        assert!(!tokens.contains(&"com".to_string()), "TLD noise should be filtered");
        assert!(!tokens.contains(&"123".to_string()), "pure numerics should be filtered");
    }

    #[test]
    fn domain_stem_extracts_registrable_label() {
        assert_eq!(domain_stem("imdb.com"), Some("imdb".to_string()));
        assert_eq!(domain_stem("store.steampowered.com"), Some("steampowered".to_string()));
        assert_eq!(domain_stem("music.apple.com"), Some("apple".to_string()));
        assert_eq!(domain_stem("youtu.be"), Some("youtu".to_string()));
    }

    #[test]
    fn title_tokens_outweigh_url_tokens_in_jaccard() {
        // 3 resources share the "react" title token → form a Precise group.
        // The 4th (Vue) shares the same domain+stem but has a different title.
        // Vue must NOT be included in the React Precise episode.
        let session = make_session(vec![
            make_res_with_url("React hooks intro", "github.com",
                "https://github.com/react-hook-form/react-hook-form", "desarrollo"),
            make_res_with_url("React hooks advanced", "github.com",
                "https://github.com/facebook/react", "desarrollo"),
            make_res_with_url("React state management", "github.com",
                "https://github.com/reduxjs/redux", "desarrollo"),
            make_res_with_url("Vue getting started", "github.com",
                "https://github.com/vuejs/vue", "desarrollo"),
        ]);
        let episodes = detect(&session);
        assert!(!episodes.is_empty());
        let react_ep = episodes.iter()
            .find(|e| e.mode == DetectionMode::Precise
                && e.resources.iter().any(|r| r.title.contains("React")));
        assert!(react_ep.is_some(), "expected a Precise episode for React resources");
        let ep = react_ep.unwrap();
        assert_eq!(ep.resources.len(), 3, "only the 3 React resources should form the episode");
        assert!(ep.resources.iter().all(|r| r.title.contains("React")),
            "Vue resource must not be included in the React episode");
    }
}
