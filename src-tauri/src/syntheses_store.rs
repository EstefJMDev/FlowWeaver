// syntheses_store.rs — Fase 3 (T-3-009)
// Propósito: persistir síntesis cifradas en SQLCipher (tabla syntheses).
// Consultado por synthesis_engine.rs para save/get. No decide transiciones (D4).
// Constraints: D1 (content_encrypted — nunca texto en claro), D8 (idempotente).

use rusqlite::Connection;

#[derive(Debug, Clone)]
pub(crate) struct SynthesisEntry {
    pub anchor_key:        String,
    pub anchor_type:       String,
    pub category:          String,
    pub synthesis_type:    String,
    pub content_encrypted: String,
    pub generated_at:      i64,
}

pub(crate) fn ensure_schema(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS syntheses (
            id                INTEGER PRIMARY KEY,
            anchor_key        TEXT NOT NULL,
            anchor_type       TEXT NOT NULL,
            category          TEXT NOT NULL,
            synthesis_type    TEXT NOT NULL,
            content_encrypted TEXT NOT NULL,
            generated_at      INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_syntheses_anchor ON syntheses(anchor_key);",
    )?;
    Ok(())
}

pub(crate) fn save(conn: &Connection, entry: &SynthesisEntry) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO syntheses
            (anchor_key, anchor_type, category, synthesis_type, content_encrypted, generated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            entry.anchor_key,
            entry.anchor_type,
            entry.category,
            entry.synthesis_type,
            entry.content_encrypted,
            entry.generated_at,
        ],
    )?;
    Ok(())
}

pub(crate) fn get_by_anchor(
    conn: &Connection,
    anchor_key: &str,
) -> Result<Option<SynthesisEntry>, rusqlite::Error> {
    let result = conn.query_row(
        "SELECT anchor_key, anchor_type, category, synthesis_type, content_encrypted, generated_at
         FROM syntheses
         WHERE anchor_key = ?1
         ORDER BY generated_at DESC
         LIMIT 1",
        rusqlite::params![anchor_key],
        |r| {
            Ok(SynthesisEntry {
                anchor_key:        r.get(0)?,
                anchor_type:       r.get(1)?,
                category:          r.get(2)?,
                synthesis_type:    r.get(3)?,
                content_encrypted: r.get(4)?,
                generated_at:      r.get(5)?,
            })
        },
    );
    match result {
        Ok(e) => Ok(Some(e)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

pub(crate) fn list_recent(
    conn: &Connection,
    limit: usize,
) -> Result<Vec<SynthesisEntry>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT anchor_key, anchor_type, category, synthesis_type, content_encrypted, generated_at
         FROM syntheses
         ORDER BY generated_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(rusqlite::params![limit as i64], |r| {
        Ok(SynthesisEntry {
            anchor_key:        r.get(0)?,
            anchor_type:       r.get(1)?,
            category:          r.get(2)?,
            synthesis_type:    r.get(3)?,
            content_encrypted: r.get(4)?,
            generated_at:      r.get(5)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
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
    fn test_save_and_get_by_anchor() {
        let conn = open_mem();
        let entry = SynthesisEntry {
            anchor_key:        "anchor-1".to_string(),
            anchor_type:       "session".to_string(),
            category:          "cocina".to_string(),
            synthesis_type:    "cocina".to_string(),
            content_encrypted: "ciphertext-abc".to_string(),
            generated_at:      1_000_000,
        };
        save(&conn, &entry).unwrap();
        let stored = get_by_anchor(&conn, "anchor-1").unwrap().unwrap();
        assert_eq!(stored.anchor_key, "anchor-1");
        assert_eq!(stored.content_encrypted, "ciphertext-abc");
    }

    #[test]
    fn test_get_by_anchor_returns_latest() {
        let conn = open_mem();
        let e1 = SynthesisEntry {
            anchor_key: "k".to_string(), anchor_type: "s".to_string(),
            category: "c".to_string(), synthesis_type: "c".to_string(),
            content_encrypted: "old".to_string(), generated_at: 1_000,
        };
        let e2 = SynthesisEntry { content_encrypted: "new".to_string(), generated_at: 2_000, ..e1.clone() };
        save(&conn, &e1).unwrap();
        save(&conn, &e2).unwrap();
        let stored = get_by_anchor(&conn, "k").unwrap().unwrap();
        assert_eq!(stored.content_encrypted, "new");
    }

    #[test]
    fn test_list_recent() {
        let conn = open_mem();
        for i in 0..3u32 {
            save(&conn, &SynthesisEntry {
                anchor_key: format!("a{i}"), anchor_type: "s".to_string(),
                category: "c".to_string(), synthesis_type: "c".to_string(),
                content_encrypted: format!("ct{i}"), generated_at: i as i64 * 1000,
            }).unwrap();
        }
        let recent = list_recent(&conn, 2).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].generated_at, 2000);
    }
}
