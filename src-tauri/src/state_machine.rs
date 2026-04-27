// State Machine — Fase 2 (T-2-003)
// Propósito: gestionar la FSM de confianza (Observing → Learning → Trusted → Autonomous).
// La State Machine es la ÚNICA autoridad de transición y de acción (D4).
// Distinto de pattern_detector.rs (detección) y trust_scorer.rs (cálculo de scores) — R12.
// Constraints activos: D4 (autoridad exclusiva), D8 (determinismo sin LLM),
// D1 (sin acceso a url/title transitivo), D14 (T-2-004 depende de este contrato).
//
// State Machine vs Pattern Detector vs Trust Scorer (R12):
// | Dimensión       | pattern_detector.rs    | trust_scorer.rs          | state_machine.rs (este)
// | Propósito       | Detectar combinaciones | Asignar trust/stability  | Decidir transiciones
// | Input           | Query SQLCipher        | &[DetectedPattern]       | &[TrustScore] + estado
// | Output          | Vec<DetectedPattern>   | Vec<TrustScore>          | TrustState
// | Acceso BD       | Sí (única query D1)    | NO — input puro          | Sólo persiste enum + ts
// | Decide acciones | No                     | NO (D4)                  | SÍ — única autoridad (D4)
// | Persistencia    | Diferida (memoria)     | En memoria (recalculable)| Persiste current_state
// | Estado interno  | Sin estado             | Sin estado (fn pura)     | FSM con persistencia
// | Determinismo    | D8                     | D8 — bit-exacto          | D8 — bit-exacto

use crate::trust_scorer::TrustScore;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustStateEnum {
    Observing,
    Learning,
    Trusted,
    Autonomous,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from: TrustStateEnum,
    pub to: TrustStateEnum,
    /// `true` para `Trusted → Autonomous` y para cualquier reset.
    /// `false` para promociones automáticas basadas en scores
    /// (`Observing → Learning`, `Learning → Trusted`).
    pub requires_user_action: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrustState {
    pub current_state: TrustStateEnum,
    pub available_transitions: Vec<Transition>,
    pub active_patterns_count: usize,
    pub last_transition_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationMode {
    /// Toma el `trust_score` máximo del slice. **Default y único implementado en T-2-003.**
    Max,
    /// Reservado — no implementado en T-2-003 (devuelve `InvalidConfig`).
    Median,
    /// Reservado — no implementado en T-2-003 (devuelve `InvalidConfig`).
    Mean,
}

#[derive(Debug, Clone)]
pub struct StateMachineConfig {
    /// Mínimo de patrones presentes en `&[TrustScore]` para abandonar `Observing`.
    pub min_patterns: usize,
    /// Umbral inferior: `Observing → Learning`.
    pub threshold_low: f64,
    /// Umbral superior: `Learning → Trusted`.
    pub threshold_high: f64,
    /// Política de agregación de `trust_score` sobre el slice.
    pub aggregation: AggregationMode,
}

impl Default for StateMachineConfig {
    fn default() -> Self {
        StateMachineConfig {
            min_patterns: 3,
            threshold_low: 0.4,
            threshold_high: 0.75,
            aggregation: AggregationMode::Max,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum UserAction {
    /// Reset desde cualquier estado a `Observing`.
    Reset,
    /// Activación explícita de modo autónomo desde `Trusted`.
    /// Requiere `confirmed: true` y `current == Trusted`; en otro caso devuelve
    /// `ConfirmationRequired` o `InvalidTransitionFromState`.
    EnableAutonomous { confirmed: bool },
}

#[derive(Debug)]
pub enum StateMachineError {
    /// Configuración inválida (umbrales inconsistentes, min_patterns = 0,
    /// agregación no implementada, etc.).
    InvalidConfig(String),
    /// `EnableAutonomous` invocado sin `confirmed: true`.
    ConfirmationRequired,
    /// `EnableAutonomous` invocado desde un estado distinto de `Trusted`.
    InvalidTransitionFromState(TrustStateEnum),
    /// Error de persistencia en SQLCipher.
    Persistence(rusqlite::Error),
}

impl std::fmt::Display for StateMachineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StateMachineError::InvalidConfig(m) => write!(f, "invalid state machine config: {m}"),
            StateMachineError::ConfirmationRequired => write!(f, "confirmation required"),
            StateMachineError::InvalidTransitionFromState(s) => {
                write!(f, "invalid transition from state {s:?}")
            }
            StateMachineError::Persistence(e) => write!(f, "persistence error: {e}"),
        }
    }
}

impl std::error::Error for StateMachineError {}

impl From<rusqlite::Error> for StateMachineError {
    fn from(e: rusqlite::Error) -> Self {
        StateMachineError::Persistence(e)
    }
}

/// Vista serializable del estado para el frontend (D14 — contrato estable).
/// Idéntico a `TrustState` por construcción: no hay campos sensibles que ocultar.
/// La existencia del wrapper preserva la libertad de ampliar `TrustState` con
/// campos internos en el futuro sin romper la superficie consumida por T-2-004.
#[derive(Debug, Clone, Serialize)]
pub struct TrustStateView {
    pub current_state: TrustStateEnum,
    pub available_transitions: Vec<Transition>,
    pub active_patterns_count: usize,
    pub last_transition_at: i64,
}

impl From<TrustState> for TrustStateView {
    fn from(s: TrustState) -> Self {
        TrustStateView {
            current_state: s.current_state,
            available_transitions: s.available_transitions,
            active_patterns_count: s.active_patterns_count,
            last_transition_at: s.last_transition_at,
        }
    }
}

/// Evalúa si procede una transición de estado dado el conjunto actual de scores,
/// el estado persistido y, opcionalmente, una acción explícita del usuario.
///
/// Función pura: no toca SQLCipher ni el reloj. La persistencia y el reloj son
/// responsabilidad del llamador (`commands.rs`).
///
/// Determinismo (D8): dos llamadas con el mismo
/// `(scores, current, last_transition_at, user_action, now_unix, config)`
/// producen bit-exactamente el mismo `TrustState`.
pub fn evaluate_transition(
    scores: &[TrustScore],
    current: TrustStateEnum,
    last_transition_at: i64,
    user_action: Option<UserAction>,
    now_unix: i64,
    config: &StateMachineConfig,
    user_blocked_pre: bool,
) -> Result<TrustState, StateMachineError> {
    validate_config(config)?;
    let active_patterns_count = scores.len();

    // 1) Acciones de usuario tienen prioridad sobre tick automático.
    if let Some(action) = user_action {
        return apply_user_action(action, current, now_unix, active_patterns_count);
    }

    // 2) Tick automático — solo promociones, nunca downgrade (postura opción b
    //    de TS-2-003 §"Postura sobre downgrade automático"; blindado por
    //    test_no_auto_downgrade_from_learning).
    //
    // user_blocked_pre se precomputa en commands::apply_trust_action consultando
    // pattern_blocks::list_blocked. Externalizar la consulta preserva D8 estricto
    // (esta función no abre Connection ni usa el reloj — TS-2-004 §"Edición
    // Mecánica").
    let aggregate = aggregate_trust(scores, config.aggregation)?;
    let next = match current {
        TrustStateEnum::Observing => {
            if scores.len() >= config.min_patterns && aggregate > config.threshold_low {
                TrustStateEnum::Learning
            } else {
                TrustStateEnum::Observing
            }
        }
        TrustStateEnum::Learning => {
            if aggregate > config.threshold_high && !user_blocked_pre {
                TrustStateEnum::Trusted
            } else {
                TrustStateEnum::Learning
            }
        }
        TrustStateEnum::Trusted | TrustStateEnum::Autonomous => current,
    };

    let new_last_ts = if next == current { last_transition_at } else { now_unix };
    Ok(TrustState {
        current_state: next,
        available_transitions: available_transitions_from(next),
        active_patterns_count,
        last_transition_at: new_last_ts,
    })
}

fn apply_user_action(
    action: UserAction,
    current: TrustStateEnum,
    now_unix: i64,
    active_patterns_count: usize,
) -> Result<TrustState, StateMachineError> {
    match action {
        UserAction::Reset => Ok(build_state(
            TrustStateEnum::Observing,
            now_unix,
            active_patterns_count,
        )),
        UserAction::EnableAutonomous { confirmed } => {
            if !confirmed {
                return Err(StateMachineError::ConfirmationRequired);
            }
            if current != TrustStateEnum::Trusted {
                return Err(StateMachineError::InvalidTransitionFromState(current));
            }
            Ok(build_state(
                TrustStateEnum::Autonomous,
                now_unix,
                active_patterns_count,
            ))
        }
    }
}

fn build_state(state: TrustStateEnum, ts: i64, active_patterns_count: usize) -> TrustState {
    TrustState {
        current_state: state,
        available_transitions: available_transitions_from(state),
        active_patterns_count,
        last_transition_at: ts,
    }
}

fn available_transitions_from(state: TrustStateEnum) -> Vec<Transition> {
    match state {
        TrustStateEnum::Observing => vec![
            Transition {
                from: TrustStateEnum::Observing,
                to: TrustStateEnum::Learning,
                requires_user_action: false,
            },
            Transition {
                from: TrustStateEnum::Observing,
                to: TrustStateEnum::Observing,
                requires_user_action: true,
            },
        ],
        TrustStateEnum::Learning => vec![
            Transition {
                from: TrustStateEnum::Learning,
                to: TrustStateEnum::Trusted,
                requires_user_action: false,
            },
            Transition {
                from: TrustStateEnum::Learning,
                to: TrustStateEnum::Observing,
                requires_user_action: true,
            },
        ],
        TrustStateEnum::Trusted => vec![
            Transition {
                from: TrustStateEnum::Trusted,
                to: TrustStateEnum::Autonomous,
                requires_user_action: true,
            },
            Transition {
                from: TrustStateEnum::Trusted,
                to: TrustStateEnum::Observing,
                requires_user_action: true,
            },
        ],
        TrustStateEnum::Autonomous => vec![Transition {
            from: TrustStateEnum::Autonomous,
            to: TrustStateEnum::Observing,
            requires_user_action: true,
        }],
    }
}

/// Iteración estable con `f64::max` para garantía de determinismo bit-exacto (D8).
/// Slice vacío devuelve `f64::NEG_INFINITY` — el caller comprueba `scores.len()`
/// antes de aplicar la rama de promoción, así que el valor centinela nunca
/// supera ningún umbral en `[0.0, 1.0]`.
fn aggregate_trust(scores: &[TrustScore], mode: AggregationMode) -> Result<f64, StateMachineError> {
    match mode {
        AggregationMode::Max => Ok(scores
            .iter()
            .fold(f64::NEG_INFINITY, |acc, s| acc.max(s.trust_score))),
        AggregationMode::Median | AggregationMode::Mean => Err(StateMachineError::InvalidConfig(
            "aggregation mode not implemented in T-2-003 baseline".into(),
        )),
    }
}

fn validate_config(config: &StateMachineConfig) -> Result<(), StateMachineError> {
    if config.min_patterns == 0 {
        return Err(StateMachineError::InvalidConfig(
            "min_patterns must be > 0".into(),
        ));
    }
    if config.threshold_low >= config.threshold_high {
        return Err(StateMachineError::InvalidConfig(format!(
            "threshold_low ({}) must be < threshold_high ({})",
            config.threshold_low, config.threshold_high
        )));
    }
    if !(0.0..=1.0).contains(&config.threshold_low)
        || !(0.0..=1.0).contains(&config.threshold_high)
    {
        return Err(StateMachineError::InvalidConfig(format!(
            "thresholds must be within [0.0, 1.0] (got low={}, high={})",
            config.threshold_low, config.threshold_high
        )));
    }
    if config.aggregation != AggregationMode::Max {
        return Err(StateMachineError::InvalidConfig(
            "aggregation mode not implemented in T-2-003 baseline".into(),
        ));
    }
    Ok(())
}

/// Crea (idempotente) la tabla `trust_state` y, si está vacía, inserta la fila
/// inicial `(1, 'Observing', now_unix, now_unix)`. Encapsulado aquí (en lugar
/// de `storage::Db::migrate`) por elección documentada en HO-013 §3
/// "Decisión operativa de integración" — el módulo dueño del schema lo
/// gestiona; `commands.rs` invoca antes de cada uso.
pub(crate) fn ensure_schema(conn: &Connection, now_unix: i64) -> Result<(), StateMachineError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS trust_state (
            id                 INTEGER PRIMARY KEY CHECK (id = 1),
            current_state      TEXT    NOT NULL CHECK (current_state IN
                                      ('Observing', 'Learning', 'Trusted', 'Autonomous')),
            last_transition_at INTEGER NOT NULL,
            updated_at         INTEGER NOT NULL
        );",
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO trust_state (id, current_state, last_transition_at, updated_at)
         VALUES (1, 'Observing', ?1, ?1)",
        rusqlite::params![now_unix],
    )?;
    Ok(())
}

/// Lee `(current_state, last_transition_at)` desde `trust_state`.
///
/// Si la tabla está vacía (no debería tras `ensure_schema`, pero defensiva
/// frente a una posible inversión accidental del orden de llamada) devuelve
/// `(Observing, 0)`. Devolvemos `0` (no `now_unix`) porque la firma no recibe
/// el reloj y mantener la función libre de `SystemTime::now()` preserva D8
/// transitivamente; la responsabilidad de inicializar la fila con `now_unix`
/// es de `ensure_schema`.
pub(crate) fn load_state(conn: &Connection) -> Result<(TrustStateEnum, i64), StateMachineError> {
    let result = conn.query_row(
        "SELECT current_state, last_transition_at FROM trust_state WHERE id = 1",
        [],
        |row| {
            let state_str: String = row.get(0)?;
            let last_ts: i64 = row.get(1)?;
            Ok((state_str, last_ts))
        },
    );
    match result {
        Ok((state_str, last_ts)) => {
            let state = parse_state(&state_str)?;
            Ok((state, last_ts))
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok((TrustStateEnum::Observing, 0)),
        Err(e) => Err(StateMachineError::Persistence(e)),
    }
}

/// Persiste el nuevo estado en `trust_state` (UPSERT sobre `id = 1`).
/// `updated_at` se actualiza siempre. `last_transition_at` se sustituye solo
/// cuando el estado cambia respecto a la fila previa; si el caller pasa el
/// mismo estado, se preserva el `last_transition_at` ya almacenado.
pub(crate) fn save_state(
    conn: &Connection,
    state: TrustStateEnum,
    last_transition_at: i64,
    now_unix: i64,
) -> Result<(), StateMachineError> {
    let state_str = serialize_state(state);
    let prev: Option<String> = conn
        .query_row(
            "SELECT current_state FROM trust_state WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let same_state = prev.as_deref() == Some(state_str);
    if same_state {
        conn.execute(
            "UPDATE trust_state SET updated_at = ?1 WHERE id = 1",
            rusqlite::params![now_unix],
        )?;
    } else {
        conn.execute(
            "INSERT INTO trust_state (id, current_state, last_transition_at, updated_at)
             VALUES (1, ?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               current_state = excluded.current_state,
               last_transition_at = excluded.last_transition_at,
               updated_at = excluded.updated_at",
            rusqlite::params![state_str, last_transition_at, now_unix],
        )?;
    }
    Ok(())
}

fn parse_state(s: &str) -> Result<TrustStateEnum, StateMachineError> {
    match s {
        "Observing" => Ok(TrustStateEnum::Observing),
        "Learning" => Ok(TrustStateEnum::Learning),
        "Trusted" => Ok(TrustStateEnum::Trusted),
        "Autonomous" => Ok(TrustStateEnum::Autonomous),
        other => Err(StateMachineError::InvalidConfig(format!(
            "unknown trust_state value '{other}'"
        ))),
    }
}

fn serialize_state(state: TrustStateEnum) -> &'static str {
    match state {
        TrustStateEnum::Observing => "Observing",
        TrustStateEnum::Learning => "Learning",
        TrustStateEnum::Trusted => "Trusted",
        TrustStateEnum::Autonomous => "Autonomous",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust_scorer::ConfidenceTier;

    const NOW: i64 = 1_700_000_000;

    fn score(pattern_id: &str, trust: f64, tier: ConfidenceTier) -> TrustScore {
        TrustScore {
            pattern_id: pattern_id.into(),
            trust_score: trust,
            stability_score: 1.0,
            recency_weight: 1.0,
            confidence_tier: tier,
        }
    }

    #[test]
    fn test_initial_state_is_observing() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn, NOW).unwrap();
        let (state, ts) = load_state(&conn).unwrap();
        assert_eq!(state, TrustStateEnum::Observing);
        assert_eq!(ts, NOW);
    }

    #[test]
    fn test_observing_to_learning_on_threshold() {
        let scores = vec![
            score("p1", 0.5, ConfidenceTier::Medium),
            score("p2", 0.5, ConfidenceTier::Medium),
            score("p3", 0.5, ConfidenceTier::Medium),
        ];
        let result = evaluate_transition(
            &scores,
            TrustStateEnum::Observing,
            NOW - 1000,
            None,
            NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();
        assert_eq!(result.current_state, TrustStateEnum::Learning);
        assert_eq!(result.last_transition_at, NOW);
    }

    #[test]
    fn test_learning_to_trusted_on_high_threshold() {
        let scores = vec![
            score("p1", 0.8, ConfidenceTier::High),
            score("p2", 0.8, ConfidenceTier::High),
            score("p3", 0.8, ConfidenceTier::High),
        ];
        let result = evaluate_transition(
            &scores,
            TrustStateEnum::Learning,
            NOW - 1000,
            None,
            NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();
        assert_eq!(result.current_state, TrustStateEnum::Trusted);
        assert_eq!(result.last_transition_at, NOW);
    }

    #[test]
    fn test_learning_to_trusted_blocked_when_user_blocked() {
        // Reactivado por T-2-004: con user_blocked_pre = true, un trust_score
        // alto NO promociona Learning → Trusted (TS-2-004 §"Edición Mecánica").
        let scores = vec![
            score("p1", 0.9, ConfidenceTier::High),
            score("p2", 0.9, ConfidenceTier::High),
            score("p3", 0.9, ConfidenceTier::High),
        ];
        let result = evaluate_transition(
            &scores,
            TrustStateEnum::Learning,
            NOW - 1000,
            None,
            NOW,
            &StateMachineConfig::default(),
            true,
        )
        .unwrap();
        assert_eq!(
            result.current_state,
            TrustStateEnum::Learning,
            "user_blocked_pre = true bloquea Learning → Trusted"
        );
    }

    #[test]
    fn test_trusted_to_autonomous_requires_explicit_action() {
        let scores = vec![
            score("p1", 1.0, ConfidenceTier::High),
            score("p2", 1.0, ConfidenceTier::High),
            score("p3", 1.0, ConfidenceTier::High),
        ];
        let cfg = StateMachineConfig::default();

        // Sin acción ⇒ se queda en Trusted aunque scores sean máximos.
        let no_action = evaluate_transition(
            &scores,
            TrustStateEnum::Trusted,
            NOW - 1000,
            None,
            NOW,
            &cfg,
            false,
        )
        .unwrap();
        assert_eq!(no_action.current_state, TrustStateEnum::Trusted);

        // Con acción confirmada ⇒ transiciona a Autonomous.
        let confirmed = evaluate_transition(
            &scores,
            TrustStateEnum::Trusted,
            NOW - 1000,
            Some(UserAction::EnableAutonomous { confirmed: true }),
            NOW,
            &cfg,
            false,
        )
        .unwrap();
        assert_eq!(confirmed.current_state, TrustStateEnum::Autonomous);
        assert_eq!(confirmed.last_transition_at, NOW);

        // Sin confirmación ⇒ ConfirmationRequired.
        let unconfirmed = evaluate_transition(
            &scores,
            TrustStateEnum::Trusted,
            NOW - 1000,
            Some(UserAction::EnableAutonomous { confirmed: false }),
            NOW,
            &cfg,
            false,
        );
        assert!(matches!(
            unconfirmed,
            Err(StateMachineError::ConfirmationRequired)
        ));

        // Desde estado distinto de Trusted ⇒ InvalidTransitionFromState.
        let from_observing = evaluate_transition(
            &scores,
            TrustStateEnum::Observing,
            NOW - 1000,
            Some(UserAction::EnableAutonomous { confirmed: true }),
            NOW,
            &cfg,
            false,
        );
        assert!(matches!(
            from_observing,
            Err(StateMachineError::InvalidTransitionFromState(
                TrustStateEnum::Observing
            ))
        ));
    }

    #[test]
    fn test_reset_from_each_state() {
        let scores = vec![
            score("p1", 0.9, ConfidenceTier::High),
            score("p2", 0.9, ConfidenceTier::High),
            score("p3", 0.9, ConfidenceTier::High),
        ];
        for from in [
            TrustStateEnum::Observing,
            TrustStateEnum::Learning,
            TrustStateEnum::Trusted,
            TrustStateEnum::Autonomous,
        ] {
            let r = evaluate_transition(
                &scores,
                from,
                NOW - 1000,
                Some(UserAction::Reset),
                NOW,
                &StateMachineConfig::default(),
                false,
            )
            .unwrap();
            assert_eq!(
                r.current_state,
                TrustStateEnum::Observing,
                "reset from {from:?} should land on Observing"
            );
            assert_eq!(r.last_transition_at, NOW);
        }
    }

    #[test]
    fn test_no_action_api_for_external_modules() {
        // Test estructural — TS-2-003 §"Restricción D4 — Autoridad Exclusiva"
        // (d). Evita falsos positivos de los literales prohibidos del propio
        // array haciendo split en `#[cfg(test)]` y revisando solo la sección
        // de producción.
        const SRC: &str = include_str!("state_machine.rs");
        let public_section = SRC
            .split("#[cfg(test)]")
            .next()
            .expect("module always has a non-test prefix");

        let forbidden_pub = [
            "pub fn force_transition",
            "pub fn promote_to",
            "pub fn set_state(",
            "pub fn override_state",
        ];
        for token in forbidden_pub {
            assert!(
                !public_section.contains(token),
                "D4 violation: forbidden public API '{token}' present"
            );
        }

        assert!(
            !public_section.contains("use crate::pattern_detector"),
            "D4 violation: state_machine must not import pattern_detector"
        );
        assert!(
            !public_section.contains("score_patterns("),
            "D4 violation: state_machine must not invoke trust_scorer::score_patterns"
        );
        assert!(
            !public_section.contains("detect_patterns("),
            "D4 violation: state_machine must not invoke pattern_detector::detect_patterns"
        );
    }

    #[test]
    fn test_determinism_bit_exact() {
        let scores = vec![
            score("p1", 0.6, ConfidenceTier::Medium),
            score("p2", 0.7, ConfidenceTier::Medium),
            score("p3", 0.8, ConfidenceTier::High),
        ];
        let cfg = StateMachineConfig::default();
        let r1 = evaluate_transition(
            &scores,
            TrustStateEnum::Observing,
            NOW - 1000,
            None,
            NOW,
            &cfg,
            false,
        )
        .unwrap();
        let r2 = evaluate_transition(
            &scores,
            TrustStateEnum::Observing,
            NOW - 1000,
            None,
            NOW,
            &cfg,
            false,
        )
        .unwrap();
        assert_eq!(r1.current_state, r2.current_state);
        // i64 — comparación bit-exacta directa por igualdad estructural.
        assert_eq!(r1.last_transition_at, r2.last_transition_at);
        assert_eq!(r1.active_patterns_count, r2.active_patterns_count);
        assert_eq!(
            r1.available_transitions.len(),
            r2.available_transitions.len()
        );
    }

    #[test]
    fn test_persistence_round_trip() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_schema(&conn, NOW).unwrap();

        let (s0, _ts0) = load_state(&conn).unwrap();
        assert_eq!(s0, TrustStateEnum::Observing);

        save_state(&conn, TrustStateEnum::Trusted, NOW + 100, NOW + 100).unwrap();

        let (s1, ts1) = load_state(&conn).unwrap();
        assert_eq!(s1, TrustStateEnum::Trusted);
        assert_eq!(ts1, NOW + 100);
    }

    #[test]
    fn test_invalid_config() {
        let scores = vec![score("p1", 0.5, ConfidenceTier::Medium)];

        // threshold_low >= threshold_high.
        let bad = StateMachineConfig {
            min_patterns: 3,
            threshold_low: 0.8,
            threshold_high: 0.5,
            aggregation: AggregationMode::Max,
        };
        let r = evaluate_transition(&scores, TrustStateEnum::Observing, 0, None, NOW, &bad, false);
        assert!(matches!(r, Err(StateMachineError::InvalidConfig(_))));

        // min_patterns == 0.
        let bad = StateMachineConfig {
            min_patterns: 0,
            threshold_low: 0.4,
            threshold_high: 0.75,
            aggregation: AggregationMode::Max,
        };
        let r = evaluate_transition(&scores, TrustStateEnum::Observing, 0, None, NOW, &bad, false);
        assert!(matches!(r, Err(StateMachineError::InvalidConfig(_))));

        // Aggregation distinto de Max — Median y Mean reservados pero no
        // implementados en T-2-003 (RK-2-003-3).
        let bad = StateMachineConfig {
            aggregation: AggregationMode::Median,
            ..StateMachineConfig::default()
        };
        let r = evaluate_transition(&scores, TrustStateEnum::Observing, 0, None, NOW, &bad, false);
        assert!(matches!(r, Err(StateMachineError::InvalidConfig(_))));

        let bad = StateMachineConfig {
            aggregation: AggregationMode::Mean,
            ..StateMachineConfig::default()
        };
        let r = evaluate_transition(&scores, TrustStateEnum::Observing, 0, None, NOW, &bad, false);
        assert!(matches!(r, Err(StateMachineError::InvalidConfig(_))));
    }

    // ── Tests recomendados (TS-2-003 §"Tests recomendados adicionales") ──

    #[test]
    fn test_observing_blocked_when_below_min_patterns() {
        let scores = vec![
            score("p1", 1.0, ConfidenceTier::High),
            score("p2", 1.0, ConfidenceTier::High),
        ];
        let r = evaluate_transition(
            &scores,
            TrustStateEnum::Observing,
            NOW - 1000,
            None,
            NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();
        assert_eq!(r.current_state, TrustStateEnum::Observing);
    }

    #[test]
    fn test_no_auto_downgrade_from_learning() {
        // Postura opción (b): scores bajos NO degradan automáticamente
        // Learning → Observing. La única vía de bajada es UserAction::Reset.
        let scores = vec![
            score("p1", 0.1, ConfidenceTier::Low),
            score("p2", 0.1, ConfidenceTier::Low),
            score("p3", 0.1, ConfidenceTier::Low),
        ];
        let r = evaluate_transition(
            &scores,
            TrustStateEnum::Learning,
            NOW - 1000,
            None,
            NOW,
            &StateMachineConfig::default(),
            false,
        )
        .unwrap();
        assert_eq!(
            r.current_state,
            TrustStateEnum::Learning,
            "scores bajos no deben degradar Learning automáticamente — opción (b)"
        );
    }
}
