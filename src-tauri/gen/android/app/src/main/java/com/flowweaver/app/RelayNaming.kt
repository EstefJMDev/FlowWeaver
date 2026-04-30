package com.flowweaver.app

// ── RelayNaming.kt ────────────────────────────────────────────────────────────
// Single source of truth for Drive AppData flat-naming used by the relay
// (Bug #1 fix, sesión 2026-04-30). Mirrors the Rust helpers in
// src-tauri/src/drive_relay.rs (desktop_pending / desktop_acked /
// android_pending_prefix / android_acked / desktop_acked_prefix).
//
// DriveRelayWorker delegates ALL Drive name construction here so the JVM unit
// test in app/src/test/.../RelayNamingTest.kt can assert byte-for-byte parity
// against the same fixture (src-tauri/tests/fixtures/cross_lang_naming.json)
// that the Rust integration test reads. INC-002 lesson: no inline templating
// outside this object.

object RelayNaming {

    /** Android pending upload — `fw-<android_id>-pending-<event_id>.json`. */
    fun androidPending(androidId: String, eventId: String): String =
        "fw-$androidId-pending-$eventId.json"

    /** Drive query prefix to read OWN Android-side ACKs. */
    fun androidAckedPrefix(androidId: String): String =
        "fw-$androidId-acked-"

    /** Drive query prefix to read Desktop-emitted pending events. */
    fun desktopPendingPrefix(desktopId: String): String =
        "fw-$desktopId-pending-"

    /** ACK file written back to desktop — `fw-<desktop_id>-acked-<event_id>.json`. */
    fun desktopAcked(desktopId: String, eventId: String): String =
        "fw-$desktopId-acked-$eventId.json"
}
