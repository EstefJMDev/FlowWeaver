// Trust Scorer — Fase 2 (T-2-002)
// Propósito: calcular trust_score y stability_score por patrón detectado.
// Trust Scorer produce inputs para la State Machine.
// No toma decisiones de acción (D4).
// Distinto de pattern_detector.rs (detección) y state_machine.rs (autoridad) — R12.
// Constraints activos: D1 (sin acceso a url/title), D4 (sin API de acción),
// D5 (stability_score con entropía normalizada en [0.0, 1.0] estricto),
// D8 (baseline determinístico sin LLM).
//
// Trust Scorer vs Pattern Detector vs State Machine (R12):
// | Dimensión       | pattern_detector.rs    | trust_scorer.rs (este)   | state_machine.rs
// | Propósito       | Detectar combinaciones | Asignar trust/stability  | Decidir transiciones
// | Input           | Query SQLCipher        | &[DetectedPattern]       | &[TrustScore] + estado
// | Output          | Vec<DetectedPattern>   | Vec<TrustScore>          | TrustState
// | Acceso BD       | Sí (única query D1)    | NO — input puro          | Solo persiste enum estado
// | Decide acciones | No                     | NO (D4)                  | SÍ — única autoridad (D4)
// | Persistencia    | Diferida (memoria)     | En memoria (recalculable)| Persiste current_state
// | Estado interno  | Sin estado             | Sin estado (fn pura)     | FSM con persistencia

use crate::pattern_detector::DetectedPattern;
use serde::{Deserialize, Serialize};

const WEIGHTS_TOLERANCE: f64 = 1e-6;

#[derive(Debug, Clone)]
pub struct TrustConfig {
    pub tier_low_max: f64,
    pub tier_high_min: f64,
    pub half_life_days: f64,
    pub frequency_saturation: f64,
    pub w_frequency: f64,
    pub w_recency: f64,
    pub w_temporal: f64,
}

impl Default for TrustConfig {
    fn default() -> Self {
        TrustConfig {
            tier_low_max: 0.4,
            tier_high_min: 0.75,
            half_life_days: 14.0,
            frequency_saturation: 12.0,
            w_frequency: 0.5,
            w_recency: 0.3,
            w_temporal: 0.2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidenceTier {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustScore {
    pub pattern_id: String,
    pub trust_score: f64,
    pub stability_score: f64,
    pub recency_weight: f64,
    pub confidence_tier: ConfidenceTier,
}

#[derive(Debug)]
pub enum TrustScorerError {
    InvalidConfig(String),
}

impl std::fmt::Display for TrustScorerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TrustScorerError::InvalidConfig(m) => write!(f, "invalid trust config: {m}"),
        }
    }
}

impl std::error::Error for TrustScorerError {}

/// Calcula scores de confianza para un conjunto de patrones detectados.
///
/// `now_unix` se pasa explícitamente para garantizar determinismo bit-exacto (D8):
/// dos llamadas con el mismo `(patterns, config, now_unix)` producen el mismo
/// `Vec<TrustScore>`.
///
/// El módulo no accede a SQLCipher ni a `url`/`title` (D1) y no decide acciones (D4).
pub fn score_patterns(
    patterns: &[DetectedPattern],
    config: &TrustConfig,
    now_unix: i64,
) -> Result<Vec<TrustScore>, TrustScorerError> {
    validate_config(config)?;

    let scores: Vec<TrustScore> = patterns
        .iter()
        .map(|p| score_one(p, config, now_unix))
        .collect();
    Ok(scores)
}

fn validate_config(config: &TrustConfig) -> Result<(), TrustScorerError> {
    let weights_sum = config.w_frequency + config.w_recency + config.w_temporal;
    if (weights_sum - 1.0).abs() > WEIGHTS_TOLERANCE {
        return Err(TrustScorerError::InvalidConfig(format!(
            "weights must sum to 1.0 (got {weights_sum})"
        )));
    }
    if config.tier_low_max >= config.tier_high_min {
        return Err(TrustScorerError::InvalidConfig(format!(
            "tier_low_max ({}) must be < tier_high_min ({})",
            config.tier_low_max, config.tier_high_min
        )));
    }
    if config.half_life_days <= 0.0 {
        return Err(TrustScorerError::InvalidConfig(format!(
            "half_life_days must be > 0 (got {})",
            config.half_life_days
        )));
    }
    if config.frequency_saturation <= 0.0 {
        return Err(TrustScorerError::InvalidConfig(format!(
            "frequency_saturation must be > 0 (got {})",
            config.frequency_saturation
        )));
    }
    Ok(())
}

fn score_one(p: &DetectedPattern, config: &TrustConfig, now_unix: i64) -> TrustScore {
    let frequency_factor = (p.frequency as f64 / config.frequency_saturation).min(1.0);
    let recency_weight = compute_recency_weight(p.last_seen, now_unix, config.half_life_days);
    let temporal_coherence = compute_temporal_coherence(p.temporal_window.day_of_week_mask);

    let trust_raw = config.w_frequency * frequency_factor
        + config.w_recency * recency_weight
        + config.w_temporal * temporal_coherence;
    let trust_score = trust_raw.max(0.0).min(1.0);

    let stability_score = compute_stability_score(&p.category_signature);

    let confidence_tier = if trust_score < config.tier_low_max {
        ConfidenceTier::Low
    } else if trust_score < config.tier_high_min {
        ConfidenceTier::Medium
    } else {
        ConfidenceTier::High
    };

    TrustScore {
        pattern_id: p.pattern_id.clone(),
        trust_score,
        stability_score,
        recency_weight,
        confidence_tier,
    }
}

fn compute_recency_weight(last_seen: i64, now_unix: i64, half_life_days: f64) -> f64 {
    let days_elapsed = (now_unix - last_seen) as f64 / 86400.0;
    let raw = 0.5_f64.powf(days_elapsed / half_life_days);
    raw.max(0.0).min(1.0)
}

fn compute_temporal_coherence(day_of_week_mask: u8) -> f64 {
    let popcount = (day_of_week_mask & 0b0111_1111).count_ones();
    if popcount == 0 {
        0.0
    } else {
        1.0 - (popcount as f64 - 1.0) / 6.0
    }
}

fn compute_stability_score(signature: &[crate::pattern_detector::CategoryWeight]) -> f64 {
    let active: Vec<f64> = signature
        .iter()
        .map(|c| c.weight)
        .filter(|w| *w > 0.0)
        .collect();
    let n = active.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 {
        return 1.0;
    }
    let h: f64 = active.iter().map(|w| -w * w.log2()).sum();
    let h_max = (n as f64).log2();
    let raw = 1.0 - h / h_max;
    raw.max(0.0).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pattern_detector::{
        CategoryWeight, DetectedPattern, DomainWeight, TemporalWindow, TimeBucket,
    };

    const DAY: i64 = 86400;

    fn pattern(
        pattern_id: &str,
        cats: &[(&str, f64)],
        bucket: TimeBucket,
        dow_mask: u8,
        frequency: usize,
        last_seen: i64,
    ) -> DetectedPattern {
        DetectedPattern {
            pattern_id: pattern_id.into(),
            label: "test".into(),
            category_signature: cats
                .iter()
                .map(|(c, w)| CategoryWeight { category: (*c).into(), weight: *w })
                .collect(),
            domain_signature: vec![DomainWeight { domain: "example.com".into(), weight: 1.0 }],
            temporal_window: TemporalWindow { time_bucket: bucket, day_of_week_mask: dow_mask },
            frequency,
            first_seen: last_seen - 30 * DAY,
            last_seen,
        }
    }

    #[test]
    fn test_pattern_frequent_recent_high_score() {
        let now = 1_700_000_000;
        let p = pattern(
            "p1",
            &[("development", 1.0)],
            TimeBucket::Morning,
            0b0000_0001,
            10,
            now,
        );
        let scores = score_patterns(&[p], &TrustConfig::default(), now).expect("score");
        let s = &scores[0];
        assert!(s.trust_score > 0.7, "trust_score should be > 0.7, got {}", s.trust_score);
        assert_eq!(s.confidence_tier, ConfidenceTier::High);
        assert!((s.recency_weight - 1.0).abs() < 1e-9, "recency should be ~1.0");
    }

    #[test]
    fn test_pattern_frequent_old_lower_score() {
        let now = 1_700_000_000;
        let recent = pattern(
            "recent",
            &[("development", 1.0)],
            TimeBucket::Morning,
            0b0000_0001,
            10,
            now,
        );
        let old = pattern(
            "old",
            &[("development", 1.0)],
            TimeBucket::Morning,
            0b0000_0001,
            10,
            now - 60 * DAY,
        );
        let cfg = TrustConfig::default();
        let scores = score_patterns(&[recent, old], &cfg, now).expect("score");
        let recent_s = &scores[0];
        let old_s = &scores[1];
        assert!(old_s.recency_weight < 0.1,
            "old recency_weight should be < 0.1 (60d / 14d half-life), got {}",
            old_s.recency_weight);
        assert!(old_s.trust_score < recent_s.trust_score,
            "old trust ({}) should be < recent trust ({})",
            old_s.trust_score, recent_s.trust_score);
    }

    #[test]
    fn test_pattern_dispersed_categories_low_stability() {
        let now = 1_700_000_000;
        let p = pattern(
            "dispersed",
            &[("a", 0.25), ("b", 0.25), ("c", 0.25), ("d", 0.25)],
            TimeBucket::Morning,
            0b0000_0001,
            5,
            now,
        );
        let scores = score_patterns(&[p], &TrustConfig::default(), now).expect("score");
        let s = &scores[0];
        assert!(s.stability_score < 0.05,
            "uniform 4-cat should give stability ≈ 0, got {}", s.stability_score);
    }

    #[test]
    fn test_pattern_single_category_max_stability() {
        let now = 1_700_000_000;
        let p = pattern(
            "single",
            &[("development", 1.0)],
            TimeBucket::Morning,
            0b0000_0001,
            5,
            now,
        );
        let scores = score_patterns(&[p], &TrustConfig::default(), now).expect("score");
        assert_eq!(scores[0].stability_score, 1.0,
            "N=1 must return stability_score = 1.0 exact");
    }

    #[test]
    fn test_scores_in_range() {
        let now = 1_700_000_000;
        let mut patterns = Vec::new();
        for freq in [1usize, 3, 12, 50] {
            for last_offset in [0i64, 30, 365] {
                for mask in [0b0000_0001u8, 0b0101_0101, 0b0111_1111, 0u8] {
                    for cats_n in 1..=6 {
                        let w = 1.0 / cats_n as f64;
                        let cats: Vec<(&str, f64)> = ["a", "b", "c", "d", "e", "f"]
                            .iter()
                            .take(cats_n)
                            .map(|c| (*c, w))
                            .collect();
                        patterns.push(pattern(
                            &format!("p-{freq}-{last_offset}-{mask}-{cats_n}"),
                            &cats,
                            TimeBucket::Morning,
                            mask,
                            freq,
                            now - last_offset * DAY,
                        ));
                    }
                }
            }
        }
        let scores = score_patterns(&patterns, &TrustConfig::default(), now).expect("score");
        for s in &scores {
            assert!((0.0..=1.0).contains(&s.trust_score),
                "trust_score out of range: {}", s.trust_score);
            assert!((0.0..=1.0).contains(&s.stability_score),
                "stability_score out of range: {}", s.stability_score);
            assert!((0.0..=1.0).contains(&s.recency_weight),
                "recency_weight out of range: {}", s.recency_weight);
        }
    }

    #[test]
    fn test_no_action_decision_api() {
        const SRC: &str = include_str!("trust_scorer.rs");
        // Inspect only the production section: everything before the test module,
        // so the forbidden literals listed below don't trigger self-detection.
        let public_section = SRC
            .split("#[cfg(test)]")
            .next()
            .expect("module always has a non-test prefix");
        let forbidden = [
            "pub fn recommend",
            "pub fn decide",
            "pub fn promote",
            "pub fn transition",
            "pub fn apply_action",
            "pub fn apply(",
            "pub fn should_",
        ];
        for token in forbidden {
            assert!(
                !public_section.contains(token),
                "D4 violation: forbidden public API '{token}' present in production section"
            );
        }
    }

    #[test]
    fn test_determinism_bit_exact() {
        let now = 1_700_000_000;
        let p = pattern(
            "det",
            &[("a", 0.6), ("b", 0.4)],
            TimeBucket::Afternoon,
            0b0010_1010,
            7,
            now - 5 * DAY,
        );
        let cfg = TrustConfig::default();
        let s1 = score_patterns(&[p.clone()], &cfg, now).expect("s1");
        let s2 = score_patterns(&[p], &cfg, now).expect("s2");
        assert_eq!(s1.len(), s2.len());
        for (a, b) in s1.iter().zip(s2.iter()) {
            assert_eq!(a.pattern_id, b.pattern_id);
            assert_eq!(a.trust_score.to_bits(), b.trust_score.to_bits(),
                "trust_score not bit-exact");
            assert_eq!(a.stability_score.to_bits(), b.stability_score.to_bits(),
                "stability_score not bit-exact");
            assert_eq!(a.recency_weight.to_bits(), b.recency_weight.to_bits(),
                "recency_weight not bit-exact");
            assert_eq!(a.confidence_tier, b.confidence_tier);
        }
    }

    #[test]
    fn test_invalid_config_weights() {
        let cfg = TrustConfig { w_frequency: 0.4, w_recency: 0.2, w_temporal: 0.1, ..TrustConfig::default() };
        let result = score_patterns(&[], &cfg, 0);
        assert!(matches!(result, Err(TrustScorerError::InvalidConfig(_))));
    }

    #[test]
    fn test_confidence_tier_thresholds_configurable() {
        let now = 1_700_000_000;
        // Patrón con stability máxima, frecuencia media, reciente, dos días activos:
        // freq_factor = 6/12 = 0.5; recency = 1.0; temporal = 1 - 1/6 ≈ 0.833
        // trust = 0.5*0.5 + 0.3*1.0 + 0.2*0.833 ≈ 0.717
        let p = pattern(
            "mid",
            &[("development", 1.0)],
            TimeBucket::Morning,
            0b0000_0011,
            6,
            now,
        );
        let strict = TrustConfig { tier_low_max: 0.8, tier_high_min: 0.95, ..TrustConfig::default() };
        let lenient = TrustConfig { tier_low_max: 0.3, tier_high_min: 0.6, ..TrustConfig::default() };
        let s_strict = &score_patterns(&[p.clone()], &strict, now).expect("strict")[0];
        let s_lenient = &score_patterns(&[p], &lenient, now).expect("lenient")[0];
        assert_eq!(s_strict.confidence_tier, ConfidenceTier::Low,
            "with strict thresholds tier should be Low (got {:?}, score {})",
            s_strict.confidence_tier, s_strict.trust_score);
        assert_eq!(s_lenient.confidence_tier, ConfidenceTier::High,
            "with lenient thresholds tier should be High (got {:?}, score {})",
            s_lenient.confidence_tier, s_lenient.trust_score);
    }
}
