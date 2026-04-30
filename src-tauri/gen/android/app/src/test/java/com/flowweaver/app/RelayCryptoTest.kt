package com.flowweaver.app

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Test
import java.io.File

/**
 * Phase 2.2 — cross-language crypto parity tests (Kotlin side).
 *
 * Loads the SAME golden vector used by the Rust integration test
 * (src-tauri/tests/cross_lang_crypto.rs), via the absolute path injected by
 * gradle as the system property `fw.fixtures.cross_lang_vectors`.
 *
 * INC-002 lesson: avoid each side validating against itself. Edit the fixture
 * once; both languages assert against it.
 */
class RelayCryptoTest {

    private data class Vector(
        val vectorId: String,
        val keyHex: String,
        val plainUtf8: String,
        val nonceHex: String,
        val expectedCiphertextHex: String
    )

    private fun loadVector(): Vector {
        val path = System.getProperty("fw.fixtures.cross_lang_vectors")
            ?: error("Missing system property fw.fixtures.cross_lang_vectors. " +
                "Gradle must inject the absolute path — see app/build.gradle.kts.")
        val file = File(path)
        require(file.exists()) { "Fixture not found at $path" }
        val json = JSONObject(file.readText(Charsets.UTF_8))
        return Vector(
            vectorId              = json.getString("vector_id"),
            keyHex                = json.getString("key_hex"),
            plainUtf8             = json.getString("plain_utf8"),
            nonceHex              = json.getString("nonce_hex"),
            expectedCiphertextHex = json.getString("expected_ciphertext_hex")
        )
    }

    private fun hexToBytes(hex: String): ByteArray =
        ByteArray(hex.length / 2) { i -> hex.substring(i * 2, i * 2 + 2).toInt(16).toByte() }

    // ── Test A.1 — deterministic encrypt matches fixture ────────────────────

    @Test
    fun kotlin_encrypt_with_fixture_nonce_matches_expected_hex() {
        val v = loadVector()
        val nonce = hexToBytes(v.nonceHex)
        check(nonce.size == 12) { "AES-GCM nonce must be 12 bytes" }
        val actual = RelayCrypto.encryptFw1aForTestWithExplicitNonce(v.plainUtf8, v.keyHex, nonce)
        assertEquals(
            "vector_id=${v.vectorId}: Kotlin encrypt produced different bytes than fixture. " +
                "If you intentionally changed RelayCrypto, regenerate the fixture and the Rust " +
                "test in cross_lang_crypto.rs must pass with the new value too.",
            v.expectedCiphertextHex,
            actual
        )
    }

    // ── Test A.2 — decrypt of fixture ciphertext recovers plaintext ─────────

    @Test
    fun kotlin_decrypt_of_fixture_ciphertext_recovers_plaintext() {
        val v = loadVector()
        val plain = RelayCrypto.decryptFw1a(v.expectedCiphertextHex, v.keyHex)
        assertNotNull("decryptFw1a must succeed for golden ciphertext", plain)
        assertEquals("vector_id=${v.vectorId}", v.plainUtf8, plain)
    }

    // ── Test A.3 — production API uses random nonce (refuerzo 1.2) ──────────
    //
    // Guards against future regressions where someone replaces SecureRandom in
    // RelayCrypto.encryptFw1a with a fixed value. Two consecutive encrypts of
    // the same plaintext + key MUST yield different ciphertexts.

    @Test
    fun production_encryptFw1a_uses_random_nonce() {
        val v = loadVector()
        val a = RelayCrypto.encryptFw1a(v.plainUtf8, v.keyHex)
        val b = RelayCrypto.encryptFw1a(v.plainUtf8, v.keyHex)
        assertNotEquals(
            "RelayCrypto.encryptFw1a must use a fresh random nonce on every call. " +
                "Reusing a nonce with the same key in AES-GCM destroys confidentiality and " +
                "authentication. If this assertion ever fires, revert the offending change.",
            a,
            b
        )
        // Sanity: both still round-trip back to plaintext.
        assertEquals(v.plainUtf8, RelayCrypto.decryptFw1a(a, v.keyHex))
        assertEquals(v.plainUtf8, RelayCrypto.decryptFw1a(b, v.keyHex))
    }
}
