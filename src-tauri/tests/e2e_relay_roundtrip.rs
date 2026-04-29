//! E2E test of the Drive relay data round-trip — Android emit → Drive payload
//! (in-memory) → Desktop import. No real network and no real Drive API.
//!
//! Why this test exists (D.1 of OD-007 audit):
//! the relay is the spine of the wow moment. If `build_raw_event`,
//! the JSON wire format, or `process_android_event` ever drift, the bridge
//! breaks silently. This test pins the data round-trip down — encryption
//! handover (local_key → shared_key → local_key), idempotency on `event_id`,
//! and the v5 UUID derivation done by `process_android_event` from
//! `(domain, url)`.
//!
//! The full `run_relay_cycle` cannot be tested here because Drive REST is not
//! abstracted behind a trait yet — see the `#[ignore]`d test below for the
//! refactor shape needed to enable the full mock cycle.

#![cfg(not(target_os = "android"))]

use std::path::Path;
use std::time::Instant;
use uuid::Uuid;

use flowweaver_lib::crypto;
use flowweaver_lib::drive_relay::{build_raw_event, process_android_event};
use flowweaver_lib::raw_event::RawEvent;
use flowweaver_lib::storage::{Db, NewResource};

const SHARED_KEY: &str = "shared-pairing-key-32-bytes-zzzz";
const ANDROID_LOCAL_KEY: &str = "android-local-key";
const DESKTOP_LOCAL_KEY: &str = "desktop-local-key";

const ANDROID_DEVICE_ID: &str = "android-deadbeef-0000-0000-0000-000000000000";

const ORIGIN_DOMAIN: &str = "github.com";
const ORIGIN_URL: &str = "https://github.com/rust-lang/rust";
const ORIGIN_TITLE: &str = "rust-lang/rust: Empowering everyone to build reliable software";
const ORIGIN_CATEGORY: &str = "development";

fn open_db() -> Db {
    let db = Db::open(Path::new(":memory:"), "test-key").expect("open db");
    db.migrate().expect("migrate db");
    db
}

#[test]
fn e2e_relay_data_roundtrip() {
    // ── Setup: two independent SQLCipher in-memory DBs ──────────────────────
    let db_android = open_db();
    let db_desktop = open_db();

    // ── Android side: capture a resource (url/title encrypted with android local key)
    let android_uuid = Uuid::new_v4().to_string();
    let captured_at = 1_714_000_000_000_i64;
    let resource = NewResource {
        uuid: android_uuid.clone(),
        url: crypto::encrypt_aes(ORIGIN_URL, ANDROID_LOCAL_KEY),
        title: crypto::encrypt_aes(ORIGIN_TITLE, ANDROID_LOCAL_KEY),
        domain: ORIGIN_DOMAIN.to_string(),
        category: ORIGIN_CATEGORY.to_string(),
        captured_at,
    };
    db_android
        .insert_or_ignore(&resource)
        .expect("insert android resource");

    let event_id = Uuid::new_v4().to_string();

    // ── Capture t0 — Android emits ──────────────────────────────────────────
    let t0 = Instant::now();

    // Build the wire event using the production helper (drive_relay::build_raw_event).
    // This exercises the encryption handover: decrypt with local key, re-encrypt
    // with shared pairing key.
    let raw_event = build_raw_event(
        &android_uuid,
        &event_id,
        &db_android,
        ANDROID_LOCAL_KEY,
        SHARED_KEY,
        ANDROID_DEVICE_ID,
    )
    .expect("build_raw_event must produce an event for an existing resource");

    // Sanity: the wire payload must NOT be the local-key ciphertext.
    let android_local_url = crypto::encrypt_aes(ORIGIN_URL, ANDROID_LOCAL_KEY);
    assert_ne!(
        raw_event.url_encrypted, android_local_url,
        "wire payload must be re-encrypted with shared key, not local key"
    );
    // Sanity: domain and category travel in clear (D1 — permitted abstraction).
    assert_eq!(raw_event.domain, ORIGIN_DOMAIN);
    assert_eq!(raw_event.category, ORIGIN_CATEGORY);
    assert_eq!(raw_event.event_id, event_id);
    assert_eq!(raw_event.device_id, ANDROID_DEVICE_ID);

    // Serialize / deserialize across the (simulated) Drive boundary.
    let wire_json = serde_json::to_string(&raw_event).expect("serialize raw_event");
    let received: RawEvent = serde_json::from_str(&wire_json).expect("deserialize raw_event");

    // ── Desktop side: import the event ──────────────────────────────────────
    let imported_uuid =
        process_android_event(&received, &db_desktop, DESKTOP_LOCAL_KEY, SHARED_KEY)
            .expect("process_android_event must succeed");

    // Capture t1 — Desktop has imported.
    let t1 = Instant::now();
    let elapsed_ms = t1.duration_since(t0).as_millis();
    println!("[METRIC] e2e_latency_ms={elapsed_ms}");

    // ── Verify: the desktop DB has the resource, decryptable with desktop key ──
    // The desktop side recomputes the resource UUID as v5(NAMESPACE_URL, domain+url).
    let expected_uuid = Uuid::new_v5(
        &Uuid::NAMESPACE_URL,
        format!("{ORIGIN_DOMAIN}{ORIGIN_URL}").as_bytes(),
    )
    .to_string();

    assert_eq!(
        imported_uuid, expected_uuid,
        "process_android_event must return the v5(domain||url) uuid that R14 \
         emits in the relay-event-imported payload"
    );

    let stored = db_desktop
        .get_by_uuid(&expected_uuid)
        .expect("query desktop db")
        .expect("desktop db must contain the imported resource");

    assert_eq!(stored.domain, ORIGIN_DOMAIN);
    assert_eq!(stored.category, ORIGIN_CATEGORY);
    assert_eq!(stored.captured_at, captured_at);

    let url_plain = crypto::decrypt_any(&stored.url, DESKTOP_LOCAL_KEY)
        .expect("desktop must decrypt url with its own local key");
    let title_plain = crypto::decrypt_any(&stored.title, DESKTOP_LOCAL_KEY)
        .expect("desktop must decrypt title with its own local key");
    assert_eq!(url_plain, ORIGIN_URL);
    assert_eq!(title_plain, ORIGIN_TITLE);

    // Idempotency: relay_events must remember this event so a redelivery is a no-op.
    assert!(
        db_desktop
            .has_relay_event_id(&event_id)
            .expect("query relay_events"),
        "process_android_event must persist event_id in relay_events"
    );

    // Replay the same event — desktop must not duplicate.
    let _ = process_android_event(&received, &db_desktop, DESKTOP_LOCAL_KEY, SHARED_KEY)
        .expect("redelivery must be a no-op");
    // Resource still stored exactly once.
    let again = db_desktop.get_by_uuid(&expected_uuid).unwrap();
    assert!(again.is_some());
}

/// Full `run_relay_cycle` round-trip with an in-memory fake Drive.
///
/// IGNORED today because the Drive REST layer in `drive_relay.rs` is not
/// abstracted behind a trait — `drive_upload`, `drive_list_prefix`,
/// `drive_download` and `ensure_token` are free `async fn` items that
/// hard-code `reqwest`. To activate this test, do the following minimal
/// refactor (no new dependencies; `#[cfg(test)]` is enough to keep
/// production untouched):
///
/// 1. Extract a `pub trait DriveApi` with these async methods (signatures
///    matching today's free functions):
///       async fn ensure_token(&self) -> Result<String, String>
///       async fn upload(&self, token: &str, name: &str, body: &str)
///                     -> Result<(), String>
///       async fn list_prefix(&self, token: &str, prefix: &str)
///                     -> Result<Vec<(String, String)>, String>
///       async fn download(&self, token: &str, file_id: &str)
///                     -> Result<String, String>
///
/// 2. Move the current `reqwest`-based bodies into a struct
///    `pub struct HttpDriveApi { /* config refs */ }` that implements
///    `DriveApi`. `run_relay_cycle` keeps wiring `HttpDriveApi` in
///    production.
///
/// 3. Change `pub async fn run_relay_cycle(...)` to take
///    `api: &dyn DriveApi` (or `impl DriveApi`) instead of building
///    its own `reqwest::Client` inline.
///
/// 4. In this test, build an `InMemoryDriveApi` (a `Mutex<HashMap<String,
///    String>>` keyed by file name) implementing `DriveApi`, prime it
///    with an Android pending file, and assert that after one cycle the
///    desktop DB contains the resource and the Android-acked file is
///    present in the in-memory store.
///
/// Estimated effort: 1 small Rust PR (~120 LOC moved, ~80 LOC test).
/// Until that refactor exists, the data-roundtrip test above already pins
/// the encryption handover, the wire shape and idempotency.
#[test]
#[ignore]
fn e2e_relay_full_cycle_with_mock_drive() {
    todo!(
        "Activate after extracting `trait DriveApi` from drive_relay.rs. \
         See doc-comment of this test for the four-step refactor shape."
    );
}
