//! Phase 2.3 — cross-language flat-naming parity tests (Rust side).
//!
//! Loads tests/fixtures/cross_lang_naming.json and asserts that the helpers in
//! `flowweaver_lib::drive_relay` produce byte-identical names to the Kotlin
//! `RelayNaming` object (see app/src/test/.../RelayNamingTest.kt).
//!
//! INC-002 lesson: never let each side validate against itself.

#![cfg(not(target_os = "android"))]

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use flowweaver_lib::drive_relay::{
    android_acked, android_pending_prefix, desktop_acked, desktop_acked_prefix, desktop_pending,
};

#[derive(Debug, Deserialize)]
struct Case {
    case_id: String,
    android_device_id: String,
    desktop_device_id: String,
    event_id: String,
    expected_android_pending_name: String,
    expected_android_acked_name: String,
    expected_android_pending_prefix: String,
    expected_android_acked_prefix: String,
    expected_desktop_pending_name: String,
    expected_desktop_acked_name: String,
    expected_desktop_pending_prefix: String,
    expected_desktop_acked_prefix: String,
}

#[derive(Debug, Deserialize)]
struct Table {
    cases: Vec<Case>,
}

fn load_table() -> Table {
    let path: PathBuf = [
        env!("CARGO_MANIFEST_DIR"),
        "tests",
        "fixtures",
        "cross_lang_naming.json",
    ]
    .iter()
    .collect();
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read fixture {path:?}: {e}"));
    serde_json::from_str(&raw).expect("parse cross_lang_naming.json")
}

// ── Test B.1 — full names + canonical prefixes ──────────────────────────────
//
// Covers everything except `desktop_acked_prefix`, which is currently unused
// in production (Bug #5). These tests run unconditionally — they must stay
// green or the relay protocol is broken.

#[test]
fn naming_helpers_match_fixture_for_all_cases() {
    let table = load_table();
    assert!(!table.cases.is_empty(), "fixture must contain at least one case");
    for c in &table.cases {
        assert_eq!(
            android_acked(&c.android_device_id, &c.event_id),
            c.expected_android_acked_name,
            "case={}: android_acked", c.case_id
        );
        assert_eq!(
            android_pending_prefix(&c.android_device_id),
            c.expected_android_pending_prefix,
            "case={}: android_pending_prefix", c.case_id
        );
        assert_eq!(
            desktop_pending(&c.desktop_device_id, &c.event_id),
            c.expected_desktop_pending_name,
            "case={}: desktop_pending", c.case_id
        );
        assert_eq!(
            desktop_acked(&c.desktop_device_id, &c.event_id),
            c.expected_desktop_acked_name,
            "case={}: desktop_acked", c.case_id
        );

        // The desktop emits its own pending names, but the wire-level *prefix*
        // for listing them is exercised on the Kotlin side (Worker reads
        // desktop pending). We pin the canonical desktop pending prefix here
        // anyway because Rust currently has no helper named for it; the test
        // catches drift in `format!` strings if anyone introduces one.
        let desktop_pending_prefix_actual =
            format!("fw-{}-pending-", c.desktop_device_id);
        assert_eq!(
            desktop_pending_prefix_actual, c.expected_desktop_pending_prefix,
            "case={}: desktop_pending_prefix template", c.case_id
        );

        // Symmetric: Android-side acked prefix (Worker reads own ACKs).
        // Same rationale as above — pin the template, not the helper.
        let android_acked_prefix_actual =
            format!("fw-{}-acked-", c.android_device_id);
        assert_eq!(
            android_acked_prefix_actual, c.expected_android_acked_prefix,
            "case={}: android_acked_prefix template", c.case_id
        );

        // Android pending NAME pinned via the helper that the desktop reads
        // is `android_pending_prefix(...) + event_id + ".json"` — assert that
        // composition matches the canonical full name.
        let composed_android_pending = format!(
            "{}{}.json",
            android_pending_prefix(&c.android_device_id),
            c.event_id
        );
        assert_eq!(
            composed_android_pending, c.expected_android_pending_name,
            "case={}: android_pending_prefix + event_id composition", c.case_id
        );
    }
}

// ── Test B.2 — Bug #5 expected post-fix behavior (IGNORED) ──────────────────

/// Expected behavior post-fix of Bug #5. See INC-002 / SESSION-2026-04-29-state-update-1.md.
/// Currently ignored because `src-tauri/src/drive_relay.rs:314` constructs the
/// ACK-listing prefix via `desktop_acked(id, "")`, yielding the broken substring
/// `"fw-<id>-acked-.json"` that never matches real ACK names. Activate this test
/// (delete `#[ignore]` AND delete the characterization test below) when Bug #5
/// is fixed by replacing line 314 with `desktop_acked_prefix(&config.device_id)`.
#[test]
fn desktop_acked_prefix_matches_fixture_post_bug5_fix() {
    let table = load_table();
    for c in &table.cases {
        assert_eq!(
            desktop_acked_prefix(&c.desktop_device_id),
            c.expected_desktop_acked_prefix,
            "case={}: desktop_acked_prefix (post-fix expected)", c.case_id
        );
    }
}

// ── Test B.3 — Characterization of Bug #5 (NOT IGNORED) ─────────────────────

/// Characterization test pinning Bug #5 (current broken behavior of
/// `drive_relay.rs:314`). When Bug #5 is fixed, THIS test must FAIL — at that
/// point, delete this test and unignore `desktop_acked_prefix_matches_fixture_post_bug5_fix`
/// above. See INC-002 / SESSION-2026-04-29-state-update-1.md.
///
/// Documents what the production code at line 314 ACTUALLY produces today:
/// `desktop_acked(id, "")` = `"fw-{id}-acked-.json"`, which Drive's
/// `name contains` operator never matches against real ACK names of the form
/// `fw-{id}-acked-{event_id}.json` (because `acked-.json` substring is absent
/// when `event_id` is non-empty).
#[test]
fn characterization_bug5_desktop_acked_with_empty_event_id_yields_broken_prefix() {
    let table = load_table();
    for c in &table.cases {
        let actual = desktop_acked(&c.desktop_device_id, "");
        let broken_expected = format!("fw-{}-acked-.json", c.desktop_device_id);
        assert_eq!(
            actual, broken_expected,
            "case={}: characterization of Bug #5 — if this changes, the bug is \
             likely fixed; delete this test and activate desktop_acked_prefix_matches_fixture_post_bug5_fix.",
            c.case_id
        );
        // Also assert that the broken prefix is NOT a substring of any real ACK name —
        // pinning the consequence (Drive `contains` query never matches).
        assert!(
            !c.expected_desktop_acked_name.contains(&broken_expected),
            "case={}: real ACK name unexpectedly contains the broken prefix — the \
             bug semantics may have shifted. Investigate before changing this test.",
            c.case_id
        );
    }
}
