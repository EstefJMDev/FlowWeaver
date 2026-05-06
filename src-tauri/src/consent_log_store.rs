// consent_log_store.rs — Fase 3 (T-3-012)
// Audit trail de consentimientos. revoked_at preserva quién consintió y cuándo,
// incluso tras revocación. has_consent solo cuenta filas activas (revoked_at IS NULL).

use rusqlite::Connection;

pub(crate) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS consent_log (
            id               INTEGER PRIMARY KEY,
            consent_type     TEXT NOT NULL,
            consent_version  TEXT NOT NULL,
            accepted_at      INTEGER NOT NULL,
            revoked_at       INTEGER
        );
        CREATE INDEX IF NOT EXISTS idx_consent_type
            ON consent_log(consent_type, consent_version);",
    )?;
    // Migración para BDs Fase 3 anteriores: añadir revoked_at si no existe.
    let _ = conn.execute_batch(
        "ALTER TABLE consent_log ADD COLUMN revoked_at INTEGER;"
    );
    Ok(())
}

/// Devuelve true si existe consentimiento activo (no revocado) para el tipo y versión dados.
pub(crate) fn has_consent(
    conn: &Connection,
    consent_type: &str,
    consent_version: &str,
) -> Result<bool, rusqlite::Error> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM consent_log
             WHERE consent_type = ?1 AND consent_version = ?2 AND revoked_at IS NULL",
            rusqlite::params![consent_type, consent_version],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count > 0)
}

pub(crate) fn record_consent(
    conn: &Connection,
    consent_type: &str,
    version: &str,
    now_unix: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO consent_log (consent_type, consent_version, accepted_at)
         VALUES (?1, ?2, ?3)",
        rusqlite::params![consent_type, version, now_unix],
    )?;
    Ok(())
}

/// Marca el consentimiento como revocado — no borra la fila para preservar el audit trail.
pub(crate) fn revoke_consent(
    conn: &Connection,
    consent_type: &str,
    now_unix: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE consent_log SET revoked_at = ?1
         WHERE consent_type = ?2 AND revoked_at IS NULL",
        rusqlite::params![now_unix, consent_type],
    )?;
    Ok(())
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
    fn test_revoke_keeps_audit_trail() {
        let conn = open_mem();

        record_consent(&conn, "synthesis", "synthesis_v1", 1_000_000).unwrap();
        assert!(has_consent(&conn, "synthesis", "synthesis_v1").unwrap());

        revoke_consent(&conn, "synthesis", 2_000_000).unwrap();

        // has_consent false tras revocar
        assert!(!has_consent(&conn, "synthesis", "synthesis_v1").unwrap());

        // Fila sigue existiendo (audit trail)
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM consent_log", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "la fila no debe borrarse");

        // revoked_at no es NULL
        let revoked_at: Option<i64> = conn
            .query_row(
                "SELECT revoked_at FROM consent_log WHERE consent_type = 'synthesis'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(revoked_at.is_some(), "revoked_at debe estar presente");
    }

    #[test]
    fn test_revoke_idempotent() {
        let conn = open_mem();
        record_consent(&conn, "synthesis", "synthesis_v1", 1_000).unwrap();
        revoke_consent(&conn, "synthesis", 2_000).unwrap();
        revoke_consent(&conn, "synthesis", 3_000).unwrap(); // segunda revocación — no falla
        assert!(!has_consent(&conn, "synthesis", "synthesis_v1").unwrap());
    }

    #[test]
    fn test_schema_idempotent() {
        let conn = open_mem();
        ensure_schema(&conn).expect("segunda llamada a ensure_schema debe pasar");
    }
}
