package com.flowweaver.app

// ── RelayCrypto.kt ────────────────────────────────────────────────────────────
// Stateless crypto helpers for the Drive relay protocol (fw1a wire format).
// Pure Kotlin / JVM — no Android imports — so JVM unit tests in app/src/test
// can exercise the exact same code that DriveRelayWorker and ShareIntentActivity
// run in production.
//
// Wire format (mirror of crypto.rs::encrypt_aes / decrypt_aes):
//   hex(MAGIC_AES "fw1a" | 12-byte random nonce | ciphertext+tag)
//
// Key derivation: SHA-256(keyHex.utf8) → 32 bytes (matches crypto.rs::derive_key_aes).
// AES-256-GCM with 128-bit auth tag.
//
// Security — nonce handling:
//   The production API `encryptFw1a` ALWAYS uses a fresh SecureRandom nonce.
//   The test-only API `encryptFw1aForTestWithExplicitNonce` exists ONLY for
//   cross-language byte-for-byte parity tests (Phase 2.2). Reusing a nonce in
//   production with the same key breaks AES-GCM confidentiality and integrity.
//   The two APIs are separate fns by design: there is no path from production
//   code to the explicit-nonce variant.

import androidx.annotation.VisibleForTesting
import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

object RelayCrypto {

    /** fw1a magic bytes — must match crypto.rs MAGIC_AES. */
    private val MAGIC_AES = byteArrayOf(0x66, 0x77, 0x31, 0x61)

    /** SHA-256(keyHex.utf8) → 32-byte AES-256 key. Matches crypto.rs::derive_key_aes. */
    fun deriveKey(keyHex: String): ByteArray =
        MessageDigest.getInstance("SHA-256").digest(keyHex.toByteArray(Charsets.UTF_8))

    /**
     * Production encrypt. Generates a fresh 12-byte nonce via SecureRandom on every
     * call — DO NOT add a nonce parameter here. Tests that need deterministic output
     * must use [encryptFw1aForTestWithExplicitNonce].
     */
    fun encryptFw1a(plaintext: String, keyHex: String): String {
        val nonce = ByteArray(12).also { SecureRandom().nextBytes(it) }
        return encryptFw1aInternal(plaintext, deriveKey(keyHex), nonce)
    }

    /**
     * Test-only variant with caller-supplied nonce, exposed for cross-language
     * golden-vector parity tests. NEVER call from production code: reusing a nonce
     * with the same key in AES-GCM breaks confidentiality and authentication.
     *
     * Marked `internal` so it cannot be referenced from outside this module
     * (DriveRelayWorker, ShareIntentActivity, etc. cannot see it). Marked
     * `@VisibleForTesting` so static analysis flags any accidental misuse.
     */
    @VisibleForTesting
    internal fun encryptFw1aForTestWithExplicitNonce(
        plaintext: String,
        keyHex: String,
        nonce: ByteArray
    ): String {
        require(nonce.size == 12) { "AES-GCM nonce must be 12 bytes" }
        return encryptFw1aInternal(plaintext, deriveKey(keyHex), nonce)
    }

    private fun encryptFw1aInternal(plaintext: String, key32: ByteArray, nonce: ByteArray): String {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(
            Cipher.ENCRYPT_MODE,
            SecretKeySpec(key32, "AES"),
            GCMParameterSpec(128, nonce)
        )
        val ct = cipher.doFinal(plaintext.toByteArray(Charsets.UTF_8))
        val out = ByteArray(MAGIC_AES.size + nonce.size + ct.size)
        System.arraycopy(MAGIC_AES, 0, out, 0, MAGIC_AES.size)
        System.arraycopy(nonce,     0, out, MAGIC_AES.size, nonce.size)
        System.arraycopy(ct,        0, out, MAGIC_AES.size + nonce.size, ct.size)
        return out.toLowerHex()
    }

    /** Decrypt fw1a hex ciphertext. Returns null on magic mismatch / auth failure / malformed input. */
    fun decryptFw1a(hexField: String?, keyHex: String): String? {
        if (hexField.isNullOrBlank()) return null
        val bytes = hexField.hexToBytesOrNull() ?: return null
        if (bytes.size < MAGIC_AES.size + 12 + 16) return null
        for (i in MAGIC_AES.indices) if (bytes[i] != MAGIC_AES[i]) return null

        val nonce = bytes.copyOfRange(MAGIC_AES.size, MAGIC_AES.size + 12)
        val ct    = bytes.copyOfRange(MAGIC_AES.size + 12, bytes.size)

        return try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(
                Cipher.DECRYPT_MODE,
                SecretKeySpec(deriveKey(keyHex), "AES"),
                GCMParameterSpec(128, nonce)
            )
            String(cipher.doFinal(ct), Charsets.UTF_8)
        } catch (_: Exception) {
            null
        }
    }

    private fun ByteArray.toLowerHex(): String =
        joinToString("") { "%02x".format(it) }

    private fun String.hexToBytesOrNull(): ByteArray? {
        if (length % 2 != 0) return null
        return try {
            ByteArray(length / 2) { i -> substring(i * 2, i * 2 + 2).toInt(16).toByte() }
        } catch (_: NumberFormatException) { null }
    }
}
