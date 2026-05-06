// Synthesis Tokens — Fase 3 (T-3-008)
// Propósito: persistir el install_token de beta cifrado con local_key.
// El token nunca se almacena en claro — solo como AES-256-GCM ciphertext.
// Consultado por synthesis_engine.rs (pub(crate)) para construir Authorization header.
// Distinto de consent_log.rs (consentimiento) y pattern_blocks.rs (patrones) — R12.
// Constraints: D1 (sin url/title), D8 (sistema funciona sin token), D25 (opt-in explícito).

use rusqlite::Connection;

pub(crate) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS synthesis_tokens (
            id              INTEGER PRIMARY KEY CHECK (id = 1),
            token_encrypted TEXT NOT NULL,
            set_at          INTEGER NOT NULL
        );",
    )?;
    // Migración Fase 3 → 3.1: rename token_hash → token_encrypted si la columna antigua existe.
    let column_exists: bool = conn
        .query_row(
            "SELECT 1 FROM pragma_table_info('synthesis_tokens') WHERE name = 'token_hash'",
            [],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if column_exists {
        conn.execute_batch(
            "ALTER TABLE synthesis_tokens RENAME COLUMN token_hash TO token_encrypted;",
        )?;
    }
    Ok(())
}

pub(crate) fn set_token(
    conn: &Connection,
    token_encrypted: &str,
    now_unix: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO synthesis_tokens (id, token_encrypted, set_at) VALUES (1, ?1, ?2)",
        rusqlite::params![token_encrypted, now_unix],
    )?;
    Ok(())
}

pub(crate) fn get_token(conn: &Connection) -> Result<Option<String>, rusqlite::Error> {
    let result = conn.query_row(
        "SELECT token_encrypted FROM synthesis_tokens WHERE id = 1",
        [],
        |r| r.get::<_, String>(0),
    );
    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub(crate) fn clear_token(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM synthesis_tokens WHERE id = 1", [])?;
    Ok(())
}

pub(crate) fn is_token_set(conn: &Connection) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM synthesis_tokens WHERE id = 1",
        [],
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
    fn test_synthesis_token_round_trip() {
        let conn = open_mem();
        assert!(!is_token_set(&conn).unwrap());
        set_token(&conn, "encrypted-placeholder", 1_000_000).unwrap();
        assert!(is_token_set(&conn).unwrap());
        let stored = get_token(&conn).unwrap();
        assert_eq!(stored, Some("encrypted-placeholder".to_string()));
        clear_token(&conn).unwrap();
        assert!(!is_token_set(&conn).unwrap());
    }

    #[test]
    fn test_set_token_idempotent() {
        let conn = open_mem();
        set_token(&conn, "ciphertext-v1", 1_000).unwrap();
        set_token(&conn, "ciphertext-v2", 2_000).unwrap();
        let stored = get_token(&conn).unwrap();
        assert_eq!(stored, Some("ciphertext-v2".to_string()), "INSERT OR REPLACE overwrites");
        assert!(is_token_set(&conn).unwrap());
    }

    #[test]
    fn test_clear_token_idempotent() {
        let conn = open_mem();
        clear_token(&conn).expect("clear on empty table must not fail");
        set_token(&conn, "ct", 1_000).unwrap();
        clear_token(&conn).unwrap();
        clear_token(&conn).expect("double clear must not fail");
        assert!(!is_token_set(&conn).unwrap());
    }

    #[test]
    fn test_get_token_returns_none_when_empty() {
        let conn = open_mem();
        assert_eq!(get_token(&conn).unwrap(), None);
    }

    #[test]
    fn test_token_hash_to_token_encrypted_migration() {
        // Simula BD Fase 3 con schema viejo (token_hash).
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch(
            "CREATE TABLE synthesis_tokens (
                id         INTEGER PRIMARY KEY CHECK (id = 1),
                token_hash TEXT NOT NULL,
                set_at     INTEGER NOT NULL
            );
            INSERT INTO synthesis_tokens (id, token_hash, set_at) VALUES (1, 'legacy-ct', 999);",
        )
        .expect("create old schema");

        // Ejecutar ensure_schema sobre BD con schema viejo.
        ensure_schema(&conn).expect("migration failed");

        // La columna debe llamarse token_encrypted y los datos siguen.
        let col_new: bool = conn
            .query_row(
                "SELECT 1 FROM pragma_table_info('synthesis_tokens') WHERE name = 'token_encrypted'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(col_new, "token_encrypted column must exist after migration");

        let col_old: bool = conn
            .query_row(
                "SELECT 1 FROM pragma_table_info('synthesis_tokens') WHERE name = 'token_hash'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(!col_old, "token_hash column must not exist after migration");

        let stored = get_token(&conn).unwrap();
        assert_eq!(stored, Some("legacy-ct".to_string()), "data preserved after migration");
    }
}
