// consent_log_store.rs — stub mínimo para T-3-009
// Implementación completa en T-3-012 (consent_dialog).
// Este stub provee solo las funciones que synthesis_engine.rs necesita.

use rusqlite::Connection;

pub(crate) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS consent_log (
            id               INTEGER PRIMARY KEY,
            consent_type     TEXT NOT NULL,
            consent_version  TEXT NOT NULL,
            accepted_at      INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_consent_type
            ON consent_log(consent_type, consent_version);",
    )?;
    Ok(())
}

/// Devuelve true si existe consentimiento activo para el tipo y versión dados.
pub(crate) fn has_consent(
    conn: &Connection,
    consent_type: &str,
    consent_version: &str,
) -> Result<bool, rusqlite::Error> {
    // Defensivo: si la tabla no existe, captura el error y devuelve false.
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM consent_log
             WHERE consent_type = ?1 AND consent_version = ?2",
            rusqlite::params![consent_type, consent_version],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(count > 0)
}
