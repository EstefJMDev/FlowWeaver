// Pattern Blocks — Fase 2 (T-2-004)
// Propósito: persistir intención del usuario de bloquear patrones detectados.
// Consultado por commands::apply_trust_action en cada tick automático para
// precomputar user_blocked_pre antes de invocar state_machine::evaluate_transition.
// Distinto de pattern_detector.rs (detección) y state_machine.rs (autoridad) — R12.
// Constraints activos: D1 (sin url/title — solo pattern_id), D4 (no decide
// transiciones — solo persiste intención), D8 (operaciones deterministas).

use rusqlite::Connection;
use std::collections::HashSet;

pub(crate) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS pattern_blocks (
            pattern_id TEXT PRIMARY KEY,
            blocked_at INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

pub(crate) fn block(
    conn: &Connection,
    pattern_id: &str,
    now_unix: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR IGNORE INTO pattern_blocks (pattern_id, blocked_at) VALUES (?1, ?2)",
        rusqlite::params![pattern_id, now_unix],
    )?;
    Ok(())
}

pub(crate) fn unblock(conn: &Connection, pattern_id: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "DELETE FROM pattern_blocks WHERE pattern_id = ?1",
        rusqlite::params![pattern_id],
    )?;
    Ok(())
}

pub(crate) fn list_blocked(conn: &Connection) -> Result<HashSet<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT pattern_id FROM pattern_blocks")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out: HashSet<String> = HashSet::new();
    for row in rows {
        out.insert(row?);
    }
    Ok(out)
}

#[allow(dead_code)]
pub(crate) fn is_blocked(conn: &Connection, pattern_id: &str) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM pattern_blocks WHERE pattern_id = ?1",
        rusqlite::params![pattern_id],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_mem() -> Connection {
        let conn = Connection::open_in_memory().expect("open");
        ensure_schema(&conn).expect("ensure_schema");
        conn
    }

    #[test]
    fn test_block_unblock_round_trip() {
        let conn = open_mem();
        assert!(!is_blocked(&conn, "p1").unwrap(), "fresh pattern is not blocked");
        block(&conn, "p1", 100).unwrap();
        assert!(is_blocked(&conn, "p1").unwrap(), "after block, is_blocked=true");
        unblock(&conn, "p1").unwrap();
        assert!(!is_blocked(&conn, "p1").unwrap(), "after unblock, is_blocked=false");
    }

    #[test]
    fn test_block_idempotent() {
        let conn = open_mem();
        block(&conn, "p1", 100).unwrap();
        block(&conn, "p1", 200).unwrap();
        block(&conn, "p1", 300).unwrap();
        let blocked = list_blocked(&conn).unwrap();
        assert_eq!(blocked.len(), 1, "duplicate blocks must collapse to single row");
        assert!(blocked.contains("p1"));
    }

    #[test]
    fn test_unblock_idempotent() {
        let conn = open_mem();
        unblock(&conn, "never-blocked").expect("unblock on missing row must not fail");
        block(&conn, "p1", 100).unwrap();
        unblock(&conn, "p1").unwrap();
        unblock(&conn, "p1").expect("second unblock must not fail");
        assert!(!is_blocked(&conn, "p1").unwrap());
    }

    #[test]
    fn test_list_blocked_returns_set() {
        let conn = open_mem();
        for id in ["p1", "p2", "p3"] {
            block(&conn, id, 100).unwrap();
        }
        let blocked = list_blocked(&conn).unwrap();
        assert_eq!(blocked.len(), 3);
        for id in ["p1", "p2", "p3"] {
            assert!(blocked.contains(id), "{id} must be in list");
        }
    }
}
