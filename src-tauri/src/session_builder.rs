/// Session Builder — Phase 0b.
/// Groups resources into temporal windows of at most 24 h.
/// A new session begins when the gap between consecutive captures exceeds GAP_SECS.
/// Resources with captured_at == 0 (bootstrap imports) form a dedicated cold-start session.
/// Independent module. Does NOT extend grouper.rs (R12 active).

use serde::Serialize;
use uuid::Uuid;

use crate::{crypto, storage::Db};

/// Gap between consecutive captures that triggers a new session (3 hours).
const GAP_SECS: i64 = 10_800;
/// Maximum duration of a single session (24 hours).
const MAX_WINDOW_SECS: i64 = 86_400;

/// A resource as seen by the Session Builder.
/// Intentionally separate from grouper::ClusterResource (R12).
#[derive(Debug, Serialize, Clone)]
pub struct SessionResource {
    pub uuid: String,
    pub title: String,       // decrypted
    pub domain: String,
    pub category: String,
    pub captured_at: i64,   // Unix seconds; 0 = bootstrap
    /// Decrypted URL — never serialized to frontend (D1). Used only for in-process tokenization.
    #[serde(skip)]
    pub url: String,
}

/// A session: a coherent burst of resource captures within a time window.
#[derive(Debug, Serialize)]
pub struct Session {
    pub session_id: String,
    pub window_start: i64,
    pub window_end: i64,
    pub is_bootstrap: bool,
    pub resources: Vec<SessionResource>,
}

/// Build sessions from the current database state. Pure read — no DB writes.
pub fn build_sessions(db: &Db, key: &str) -> Result<Vec<Session>, String> {
    let rows = db.all_resources().map_err(|e| e.to_string())?;

    // Decrypt titles; fall back to domain on decryption failure.
    let mut resources: Vec<SessionResource> = rows
        .into_iter()
        .map(|r| SessionResource {
            uuid: r.uuid,
            title: crypto::decrypt(&r.title, key).unwrap_or_else(|| r.domain.clone()),
            url: crypto::decrypt_any(&r.url, key).unwrap_or_default(),
            domain: r.domain,
            category: r.category,
            captured_at: r.captured_at,
        })
        .collect();

    // Already ordered by captured_at, id from the DB query.
    // Partition into bootstrap (no timestamp) and timestamped.
    let (bootstrap, timestamped): (Vec<_>, Vec<_>) =
        resources.drain(..).partition(|r| r.captured_at == 0);

    let mut sessions: Vec<Session> = Vec::new();

    // Bootstrap session — all cold-start bookmarks with no timestamp.
    if !bootstrap.is_empty() {
        sessions.push(Session {
            session_id: Uuid::new_v4().to_string(),
            window_start: 0,
            window_end: 0,
            is_bootstrap: true,
            resources: bootstrap,
        });
    }

    // Timestamped sessions: split by gap or 24 h window overflow.
    if !timestamped.is_empty() {
        let mut current: Vec<SessionResource> = Vec::new();
        let mut session_start = timestamped[0].captured_at;
        let mut prev_ts = timestamped[0].captured_at;

        for res in timestamped {
            let ts = res.captured_at;
            let gap = ts - prev_ts;
            let span = ts - session_start;

            if gap > GAP_SECS || span >= MAX_WINDOW_SECS {
                flush_session(&mut sessions, &mut current, session_start);
                session_start = ts;
            }

            prev_ts = ts;
            current.push(res);
        }
        flush_session(&mut sessions, &mut current, session_start);
    }

    Ok(sessions)
}

fn flush_session(sessions: &mut Vec<Session>, current: &mut Vec<SessionResource>, start: i64) {
    if current.is_empty() {
        return;
    }
    let end = current.last().map(|r| r.captured_at).unwrap_or(start);
    sessions.push(Session {
        session_id: Uuid::new_v4().to_string(),
        window_start: start,
        window_end: end,
        is_bootstrap: false,
        resources: std::mem::take(current),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_res(ts: i64) -> SessionResource {
        SessionResource {
            uuid: Uuid::new_v4().to_string(),
            title: format!("Resource {ts}"),
            domain: "example.com".into(),
            category: "desarrollo".into(),
            url: String::new(),
            captured_at: ts,
        }
    }

    #[test]
    fn bootstrap_session_created_for_zero_timestamps() {
        let resources = vec![make_res(0), make_res(0)];
        // Simulate partition logic directly
        let (boot, ts): (Vec<_>, Vec<_>) = resources.into_iter().partition(|r| r.captured_at == 0);
        assert_eq!(boot.len(), 2);
        assert!(ts.is_empty());
    }

    #[test]
    fn gap_triggers_new_session() {
        // Gap is measured from prev_ts, not from base.
        // r2 is at base+100; r3 must be > GAP_SECS after r2 to start a new session.
        let base: i64 = 1_700_000_000;
        let r1 = make_res(base);
        let r2 = make_res(base + 100);
        let r3 = make_res(base + 100 + GAP_SECS + 1); // gap from r2 > GAP_SECS
        let r4 = make_res(base + 100 + GAP_SECS + 200);

        let mut all = vec![r1, r2, r3, r4];
        all.sort_by_key(|r| r.captured_at);

        let mut sessions: Vec<Session> = Vec::new();
        let mut current: Vec<SessionResource> = Vec::new();
        let mut session_start = all[0].captured_at;
        let mut prev_ts = all[0].captured_at;

        for res in all {
            let ts = res.captured_at;
            if ts - prev_ts > GAP_SECS || ts - session_start >= MAX_WINDOW_SECS {
                flush_session(&mut sessions, &mut current, session_start);
                session_start = ts;
            }
            prev_ts = ts;
            current.push(res);
        }
        flush_session(&mut sessions, &mut current, session_start);

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].resources.len(), 2);
        assert_eq!(sessions[1].resources.len(), 2);
    }
}
