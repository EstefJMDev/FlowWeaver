// Pattern Detector — Fase 2 (T-2-001)
// Propósito: detección de patrones longitudinales sobre domain/category.
// Distinción R12: este módulo opera sobre historial completo en SQLCipher
// (días/semanas), no sobre sesiones activas. Ver episode_detector.rs para
// detección de sesión. Ambos módulos son independientes semánticamente.
// Constraints activos: D1 (solo domain/category), D8 (sin LLM requerido),
// D17 (módulo completo en Fase 2, no dividir entre fases).
//
// Pattern Detector vs Episode Detector (R12):
// | Dimensión        | episode_detector.rs        | pattern_detector.rs (este)
// | Propósito        | Sesión activa              | Historial longitudinal
// | Temporalidad     | Tiempo real                | Días/semanas
// | Input            | Vec<SessionResource>       | Query SQLCipher
// | Estado persistido| Ninguno                    | Patrones acumulados
// | Acceso a title   | Sí (campo en memoria)      | NUNCA — solo domain/category
// | Algoritmo        | Jaccard sobre tokens       | Co-ocurrencia por ventana

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Recursos consecutivos con `captured_at` separado por <= 30 min componen
/// una "sesión de captura" (paso 1 del algoritmo).
const SESSION_GAP_SECS: i64 = 30 * 60;

/// Si dos patrones comparten más que este Jaccard de categorías, se considera
/// duplicado y se conserva el de mayor frequency (paso 5).
const OVERLAP_THRESHOLD: f64 = 0.8;

/// Única query SQLCipher autorizada por D1: domain, category, captured_at.
/// Inspeccionada por test_no_url_or_title_in_query.
pub(crate) const RESOURCES_QUERY: &str = "SELECT domain, category, captured_at \
     FROM resources \
     WHERE captured_at >= ?1 \
     ORDER BY captured_at ASC";

#[derive(Debug, Clone)]
pub struct PatternConfig {
    pub min_frequency: usize,
    pub lookback_days: u32,
    pub time_bucket_boundaries: [u32; 2],
}

impl Default for PatternConfig {
    fn default() -> Self {
        PatternConfig {
            min_frequency: 3,
            lookback_days: 30,
            time_bucket_boundaries: [12, 18],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum TimeBucket {
    Morning,
    Afternoon,
    Evening,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryWeight {
    pub category: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainWeight {
    pub domain: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalWindow {
    pub time_bucket: TimeBucket,
    pub day_of_week_mask: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPattern {
    pub pattern_id: String,
    pub label: String,
    pub category_signature: Vec<CategoryWeight>,
    pub domain_signature: Vec<DomainWeight>,
    pub temporal_window: TemporalWindow,
    pub frequency: usize,
    pub first_seen: i64,
    pub last_seen: i64,
}

#[derive(Debug)]
pub enum PatternDetectorError {
    Sqlite(rusqlite::Error),
    Time(std::time::SystemTimeError),
}

impl From<rusqlite::Error> for PatternDetectorError {
    fn from(e: rusqlite::Error) -> Self {
        PatternDetectorError::Sqlite(e)
    }
}

impl From<std::time::SystemTimeError> for PatternDetectorError {
    fn from(e: std::time::SystemTimeError) -> Self {
        PatternDetectorError::Time(e)
    }
}

impl std::fmt::Display for PatternDetectorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternDetectorError::Sqlite(e) => write!(f, "sqlite: {e}"),
            PatternDetectorError::Time(e) => write!(f, "system time: {e}"),
        }
    }
}

impl std::error::Error for PatternDetectorError {}

/// Analiza el historial en SQLCipher y devuelve patrones recurrentes.
/// Solo accede a domain, category, captured_at (D1).
pub fn detect_patterns(
    conn: &Connection,
    config: &PatternConfig,
) -> Result<Vec<DetectedPattern>, PatternDetectorError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs() as i64;
    let cutoff = now - (config.lookback_days as i64) * 86400;

    let mut stmt = conn.prepare(RESOURCES_QUERY)?;
    let rows: Vec<(String, String, i64)> = stmt
        .query_map([cutoff], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let sessions = group_into_sessions(rows);
    let labeled: Vec<LabeledSession> = sessions
        .into_iter()
        .map(|s| label_session(s, config))
        .collect();

    let mut groups: HashMap<(Vec<String>, TimeBucket), Vec<LabeledSession>> = HashMap::new();
    for s in labeled {
        let mut cats: Vec<String> = s.category_counts.keys().cloned().collect();
        cats.sort();
        let key = (cats, s.time_bucket.clone());
        groups.entry(key).or_default().push(s);
    }

    let mut candidates: Vec<DetectedPattern> = groups
        .into_iter()
        .filter(|(_, v)| v.len() >= config.min_frequency)
        .map(|((_, time_bucket), sessions)| build_pattern(time_bucket, sessions))
        .collect();

    candidates.sort_by(|a, b| b.frequency.cmp(&a.frequency));
    let mut kept: Vec<DetectedPattern> = Vec::with_capacity(candidates.len());
    for c in candidates {
        let dup = kept.iter().any(|k| category_overlap(k, &c) > OVERLAP_THRESHOLD);
        if !dup {
            kept.push(c);
        }
    }
    Ok(kept)
}

struct LabeledSession {
    first_at: i64,
    last_at: i64,
    time_bucket: TimeBucket,
    day_of_week_bit: u8,
    category_counts: HashMap<String, usize>,
    domain_counts: HashMap<String, usize>,
}

fn group_into_sessions(rows: Vec<(String, String, i64)>) -> Vec<Vec<(String, String, i64)>> {
    let mut out: Vec<Vec<(String, String, i64)>> = Vec::new();
    let mut current: Vec<(String, String, i64)> = Vec::new();
    let mut last_ts: Option<i64> = None;
    for r in rows {
        if let Some(prev) = last_ts {
            if r.2 - prev > SESSION_GAP_SECS && !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        }
        last_ts = Some(r.2);
        current.push(r);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn label_session(rows: Vec<(String, String, i64)>, config: &PatternConfig) -> LabeledSession {
    let first_at = rows.first().map(|r| r.2).unwrap_or(0);
    let last_at = rows.last().map(|r| r.2).unwrap_or(0);
    let time_bucket = derive_time_bucket(first_at, config.time_bucket_boundaries);
    let day_of_week_bit = derive_day_of_week_bit(first_at);
    let mut category_counts: HashMap<String, usize> = HashMap::new();
    let mut domain_counts: HashMap<String, usize> = HashMap::new();
    for (domain, category, _) in rows {
        *category_counts.entry(category).or_insert(0) += 1;
        *domain_counts.entry(domain).or_insert(0) += 1;
    }
    LabeledSession {
        first_at,
        last_at,
        time_bucket,
        day_of_week_bit,
        category_counts,
        domain_counts,
    }
}

fn derive_time_bucket(ts: i64, boundaries: [u32; 2]) -> TimeBucket {
    let hour = (ts.rem_euclid(86400) / 3600) as u32;
    if hour < boundaries[0] {
        TimeBucket::Morning
    } else if hour < boundaries[1] {
        TimeBucket::Afternoon
    } else {
        TimeBucket::Evening
    }
}

// Unix epoch (1970-01-01) cae en jueves. Lunes = bit 0 ⇒ offset = 3
// para que `derive_day_of_week_bit(epoch) == 3` (jueves).
fn derive_day_of_week_bit(ts: i64) -> u8 {
    let days = ts.div_euclid(86400);
    ((days + 3).rem_euclid(7)) as u8
}

fn build_pattern(time_bucket: TimeBucket, sessions: Vec<LabeledSession>) -> DetectedPattern {
    let frequency = sessions.len();
    let first_seen = sessions.iter().map(|s| s.first_at).min().unwrap_or(0);
    let last_seen = sessions.iter().map(|s| s.last_at).max().unwrap_or(0);
    let mut day_of_week_mask: u8 = 0;
    let mut cat_counts: HashMap<String, usize> = HashMap::new();
    let mut dom_counts: HashMap<String, usize> = HashMap::new();
    for s in &sessions {
        day_of_week_mask |= 1u8 << s.day_of_week_bit;
        for (k, v) in &s.category_counts {
            *cat_counts.entry(k.clone()).or_insert(0) += v;
        }
        for (k, v) in &s.domain_counts {
            *dom_counts.entry(k.clone()).or_insert(0) += v;
        }
    }
    let total: usize = cat_counts.values().sum();
    let denom = total.max(1) as f64;

    let mut category_signature: Vec<CategoryWeight> = cat_counts
        .into_iter()
        .map(|(category, n)| CategoryWeight { category, weight: n as f64 / denom })
        .collect();
    category_signature.sort_by(|a, b| {
        b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.category.cmp(&b.category))
    });

    let mut domain_signature: Vec<DomainWeight> = dom_counts
        .into_iter()
        .map(|(domain, n)| DomainWeight { domain, weight: n as f64 / denom })
        .collect();
    domain_signature.sort_by(|a, b| {
        b.weight.partial_cmp(&a.weight).unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.domain.cmp(&b.domain))
    });

    let dominant_category = category_signature
        .first()
        .map(|c| c.category.clone())
        .unwrap_or_default();
    let label = format!("{} ({})", dominant_category, time_bucket_es(&time_bucket));

    DetectedPattern {
        pattern_id: Uuid::new_v4().to_string(),
        label,
        category_signature,
        domain_signature,
        temporal_window: TemporalWindow { time_bucket, day_of_week_mask },
        frequency,
        first_seen,
        last_seen,
    }
}

fn time_bucket_es(b: &TimeBucket) -> &'static str {
    match b {
        TimeBucket::Morning => "mañana",
        TimeBucket::Afternoon => "tarde",
        TimeBucket::Evening => "noche",
    }
}

fn category_overlap(a: &DetectedPattern, b: &DetectedPattern) -> f64 {
    use std::collections::BTreeSet;
    let cats_a: BTreeSet<&str> = a.category_signature.iter().map(|c| c.category.as_str()).collect();
    let cats_b: BTreeSet<&str> = b.category_signature.iter().map(|c| c.category.as_str()).collect();
    if cats_a.is_empty() || cats_b.is_empty() {
        return 0.0;
    }
    let inter = cats_a.intersection(&cats_b).count();
    let union = cats_a.union(&cats_b).count();
    inter as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Db, NewResource};
    use std::path::Path;

    const DAY: i64 = 86400;
    const HOUR: i64 = 3600;
    const MIN_S: i64 = 60;

    fn open_mem() -> Db {
        let db = Db::open(Path::new(":memory:"), "test-key").expect("open");
        db.migrate().expect("migrate");
        db
    }

    fn insert_at(db: &Db, domain: &str, category: &str, captured_at: i64) {
        db.insert_resource(&NewResource {
            uuid: Uuid::new_v4().to_string(),
            url: format!("enc-{domain}"),
            title: format!("enc-title-{domain}"),
            domain: domain.into(),
            category: category.into(),
            captured_at,
        }).expect("insert");
    }

    fn test_config() -> PatternConfig {
        PatternConfig {
            min_frequency: 3,
            lookback_days: 36500, // ~100 años: cubre cualquier timestamp histórico
            time_bucket_boundaries: [12, 18],
        }
    }

    // 1970-01-01 (Jueves) = day 0. Primer Lunes = day 4 (1970-01-05).
    // day_of_week: 0=Lun, 1=Mar, 2=Mié, 3=Jue, 4=Vie, 5=Sáb, 6=Dom.
    fn ts_at(day_of_week: u8, week: i64, hour: i64, minute: i64) -> i64 {
        let day_offset = 4 + (day_of_week as i64) + 7 * week;
        day_offset * DAY + hour * HOUR + minute * MIN_S
    }

    #[test]
    fn test_detect_known_pattern_development_morning() {
        let db = open_mem();
        for week in 0..3 {
            let t = ts_at(0, week, 9, 15);
            insert_at(&db, "github.com", "desarrollo", t);
            insert_at(&db, "docs.rs", "desarrollo", t + 15 * MIN_S);
            insert_at(&db, "crates.io", "desarrollo", t + 30 * MIN_S);
        }
        let patterns = detect_patterns(db.conn(), &test_config()).expect("detect");
        let p = patterns.iter().find(|p| p.label == "desarrollo (mañana)")
            .expect("development morning pattern not found");
        assert!(p.frequency >= 3, "frequency should be >= 3, got {}", p.frequency);
        assert_eq!(p.temporal_window.day_of_week_mask & 0b0000_0001, 0b0000_0001,
            "monday bit should be set");
        assert!(matches!(p.temporal_window.time_bucket, TimeBucket::Morning));
    }

    #[test]
    fn test_detect_known_pattern_media_afternoon() {
        let db = open_mem();
        for week in 0..3 {
            let t = ts_at(2, week, 15, 10);
            insert_at(&db, "youtube.com", "media", t);
            insert_at(&db, "spotify.com", "media", t + 10 * MIN_S);
        }
        let patterns = detect_patterns(db.conn(), &test_config()).expect("detect");
        let p = patterns.iter().find(|p| p.label == "media (tarde)")
            .expect("media afternoon pattern not found");
        assert!(p.frequency >= 3);
        assert_eq!(p.temporal_window.day_of_week_mask & 0b0000_0100, 0b0000_0100,
            "wednesday bit should be set");
        assert!(matches!(p.temporal_window.time_bucket, TimeBucket::Afternoon));
    }

    #[test]
    fn test_below_min_frequency_not_detected() {
        let db = open_mem();
        let t = ts_at(4, 0, 20, 0);
        insert_at(&db, "nytimes.com", "news", t);
        let patterns = detect_patterns(db.conn(), &test_config()).expect("detect");
        assert!(patterns.is_empty(),
            "expected no patterns with frequency < 3, got {patterns:?}");
    }

    #[test]
    fn test_no_url_or_title_in_query() {
        assert!(!RESOURCES_QUERY.contains("url"), "query must not reference url (D1)");
        assert!(!RESOURCES_QUERY.contains("title"), "query must not reference title (D1)");
    }

    #[test]
    fn test_pattern_id_is_uuid() {
        let db = open_mem();
        for week in 0..3 {
            let t = ts_at(0, week, 9, 15);
            insert_at(&db, "github.com", "desarrollo", t);
            insert_at(&db, "docs.rs", "desarrollo", t + 15 * MIN_S);
        }
        let patterns = detect_patterns(db.conn(), &test_config()).expect("detect");
        assert!(!patterns.is_empty());
        for p in &patterns {
            Uuid::parse_str(&p.pattern_id).expect("pattern_id is not a valid UUID");
        }
    }
}
