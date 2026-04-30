package com.flowweaver.app

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Ignore
import org.junit.Test
import java.io.File

/**
 * Phase 2.3 — cross-language flat-naming parity tests (Kotlin side).
 *
 * Loads the SAME fixture as the Rust integration test
 * (src-tauri/tests/relay_naming_convention.rs) via the absolute path injected
 * by gradle as `fw.fixtures.cross_lang_naming` (see app/build.gradle.kts).
 *
 * INC-002 lesson: edit the fixture in one place; both languages assert against it.
 */
class RelayNamingTest {

    private data class Case(
        val caseId: String,
        val androidDeviceId: String,
        val desktopDeviceId: String,
        val eventId: String,
        val expectedAndroidPendingName: String,
        val expectedAndroidAckedName: String,
        val expectedAndroidPendingPrefix: String,
        val expectedAndroidAckedPrefix: String,
        val expectedDesktopPendingName: String,
        val expectedDesktopAckedName: String,
        val expectedDesktopPendingPrefix: String,
        val expectedDesktopAckedPrefix: String
    )

    private fun loadTable(): List<Case> {
        val path = System.getProperty("fw.fixtures.cross_lang_naming")
            ?: error("Missing system property fw.fixtures.cross_lang_naming. " +
                "Gradle must inject the absolute path — see app/build.gradle.kts.")
        val file = File(path)
        require(file.exists()) { "Fixture not found at $path" }
        val root = JSONObject(file.readText(Charsets.UTF_8))
        val arr = root.getJSONArray("cases")
        return List(arr.length()) { i ->
            val o = arr.getJSONObject(i)
            Case(
                caseId                       = o.getString("case_id"),
                androidDeviceId              = o.getString("android_device_id"),
                desktopDeviceId              = o.getString("desktop_device_id"),
                eventId                      = o.getString("event_id"),
                expectedAndroidPendingName   = o.getString("expected_android_pending_name"),
                expectedAndroidAckedName     = o.getString("expected_android_acked_name"),
                expectedAndroidPendingPrefix = o.getString("expected_android_pending_prefix"),
                expectedAndroidAckedPrefix   = o.getString("expected_android_acked_prefix"),
                expectedDesktopPendingName   = o.getString("expected_desktop_pending_name"),
                expectedDesktopAckedName     = o.getString("expected_desktop_acked_name"),
                expectedDesktopPendingPrefix = o.getString("expected_desktop_pending_prefix"),
                expectedDesktopAckedPrefix   = o.getString("expected_desktop_acked_prefix")
            )
        }
    }

    // ── Test B.1 — RelayNaming output matches fixture for every case ────────

    @Test
    fun relay_naming_matches_fixture_for_all_cases() {
        val cases = loadTable()
        assertNotNull(cases)
        check(cases.isNotEmpty()) { "fixture must contain at least one case" }
        for (c in cases) {
            assertEquals(
                "case=${c.caseId}: androidPending",
                c.expectedAndroidPendingName,
                RelayNaming.androidPending(c.androidDeviceId, c.eventId)
            )
            assertEquals(
                "case=${c.caseId}: androidAckedPrefix",
                c.expectedAndroidAckedPrefix,
                RelayNaming.androidAckedPrefix(c.androidDeviceId)
            )
            assertEquals(
                "case=${c.caseId}: desktopPendingPrefix",
                c.expectedDesktopPendingPrefix,
                RelayNaming.desktopPendingPrefix(c.desktopDeviceId)
            )
            assertEquals(
                "case=${c.caseId}: desktopAcked",
                c.expectedDesktopAckedName,
                RelayNaming.desktopAcked(c.desktopDeviceId, c.eventId)
            )
            // Sanity: the canonical full ACK name must START WITH the canonical prefix.
            // Closes the gap that Bug #5 exploits on the Rust side: real ACK names
            // begin with `fw-{id}-acked-`, NOT `fw-{id}-acked-.json`.
            val canonicalAckPrefix = "fw-${c.desktopDeviceId}-acked-"
            assertEquals(
                "case=${c.caseId}: expected_desktop_acked_prefix template",
                c.expectedDesktopAckedPrefix,
                canonicalAckPrefix
            )
            assert(c.expectedDesktopAckedName.startsWith(canonicalAckPrefix)) {
                "case=${c.caseId}: real ACK name must start with canonical prefix"
            }

            val canonicalAndroidAckPrefix = "fw-${c.androidDeviceId}-acked-"
            assertEquals(
                "case=${c.caseId}: expected_android_acked_prefix template",
                c.expectedAndroidAckedPrefix,
                canonicalAndroidAckPrefix
            )
            assert(c.expectedAndroidAckedName.startsWith(canonicalAndroidAckPrefix))
        }
    }

    // ── Test B.2 — Bug #5 cross-checks (Kotlin side, defensive) ─────────────
    //
    // Bug #5 lives on the Rust desktop side (drive_relay.rs:314). The Kotlin
    // Worker is unaffected. We still mirror the assertion that the canonical
    // ACK prefix is NEVER a substring containing `.json`, so any future drift
    // toward the broken shape is caught on this side too.

    @Test
    fun bug5_canonical_acked_prefix_does_not_contain_dot_json() {
        for (c in loadTable()) {
            assertFalse(
                "case=${c.caseId}: canonical ACK prefix must not contain '.json' — " +
                    "if this asserts, someone reintroduced the Bug #5 shape on the Kotlin side. " +
                    "See SESSION-2026-04-29-state-update-1.md.",
                c.expectedDesktopAckedPrefix.contains(".json")
            )
            assertFalse(
                "case=${c.caseId}: Android ACK prefix must not contain '.json'",
                c.expectedAndroidAckedPrefix.contains(".json")
            )
        }
    }

    // ── Test B.3 — Bug #5 expected post-fix Kotlin counterpart (IGNORED) ────
    //
    // Symmetric @Ignore so the cross-language gate trips on BOTH sides if
    // Bug #5 is reintroduced after fix. Activate (remove @Ignore) at the same
    // time as the Rust counterpart `desktop_acked_prefix_matches_fixture_post_bug5_fix`.

    @Test
    @Ignore("Expected behavior post-fix of Bug #5. " +
        "Currently ignored because the Kotlin side does not yet need a desktopAckedPrefix " +
        "helper (Worker only writes ACKs, never lists them). When Bug #5 is fixed in " +
        "drive_relay.rs:314 and a parallel listing path is added on Kotlin, mirror it here. " +
        "See INC-002 / SESSION-2026-04-29-state-update-1.md.")
    fun desktop_acked_prefix_post_bug5_fix_kotlin_counterpart() {
        // Placeholder: when Bug #5 is fixed, add `RelayNaming.desktopAckedPrefix(id)` and
        // assert it equals each `c.expectedDesktopAckedPrefix` from the fixture.
    }
}
