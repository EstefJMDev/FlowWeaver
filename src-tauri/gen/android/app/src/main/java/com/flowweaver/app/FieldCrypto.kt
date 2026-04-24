package com.flowweaver.app

// ── FieldCrypto.kt ─────────────────────────────────────────────────────────────
// AES-256-GCM field-level encryption for url and title stored in SQLite Android.
//
// Wire format: hex(MAGIC_AES "fw1a" | 12-byte random nonce | ciphertext+16-byte GCM tag)
// This is IDENTICAL to the format produced by Rust crypto::encrypt_aes() so that
// Rust's decrypt_any() can read records written by this Kotlin layer and vice versa.
//
// Key derivation: SHA-256(FIELD_KEY_PASSPHRASE) → 32-byte AES key.
// FIELD_KEY_PASSPHRASE must match the value returned by commands.rs db_key() on
// the Android target. Changing it invalidates all existing encrypted records.
//
// D1 compliance: url and title never leave this object in plaintext.
// R12: no Episode Detector, Pattern Detector or longitudinal analysis.
//      This class encrypts and decrypts individual field values only.
//
// Previous format (fw0a XOR): handled in migrateXorField() for T-0c-001 migration.

import java.security.MessageDigest
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.spec.GCMParameterSpec
import javax.crypto.spec.SecretKeySpec

object FieldCrypto {

    // Must match db_key() in commands.rs when cfg(target_os = "android").
    // Do NOT change after first deployment — it would invalidate all stored records.
    const val FIELD_KEY_PASSPHRASE = "flowweaver-android-field-key-v1"

    private val MAGIC_AES  = byteArrayOf(0x66, 0x77, 0x31, 0x61) // "fw1a"
    private val MAGIC_XOR  = byteArrayOf(0x66, 0x77, 0x30, 0x61) // "fw0a"
    private const val NONCE_BYTES  = 12
    private const val GCM_TAG_BITS = 128

    // ── Key derivation ───────────────────────────────────────────────────────────

    /** SHA-256(passphrase) → 32-byte AES key. Mirrors Rust's derive_key_aes(). */
    fun deriveKey(passphrase: String): ByteArray =
        MessageDigest.getInstance("SHA-256").digest(passphrase.toByteArray(Charsets.UTF_8))

    // ── Encrypt (fw1a) ───────────────────────────────────────────────────────────

    /**
     * Encrypt [plaintext] with AES-256-GCM using [keyBytes].
     * Returns lowercase hex string: hex(MAGIC_AES | 12-byte nonce | ciphertext+tag).
     * Compatible with Rust crypto::encrypt_aes().
     */
    fun encrypt(plaintext: String, keyBytes: ByteArray): String {
        val nonce = ByteArray(NONCE_BYTES).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, SecretKeySpec(keyBytes, "AES"), GCMParameterSpec(GCM_TAG_BITS, nonce))
        val ct = cipher.doFinal(plaintext.toByteArray(Charsets.UTF_8))
        return (MAGIC_AES + nonce + ct).toHexString()
    }

    // ── Decrypt (fw1a) ───────────────────────────────────────────────────────────

    /**
     * Decrypt a fw1a hex ciphertext produced by [encrypt] or by Rust encrypt_aes().
     * Returns null if magic mismatch, malformed input, or decryption fails.
     */
    fun decrypt(hexStr: String, keyBytes: ByteArray): String? {
        val bytes = hexStr.hexToByteArray() ?: return null
        if (bytes.size < MAGIC_AES.size + NONCE_BYTES + 16) return null
        if (!bytes.startsWith(MAGIC_AES)) return null
        val nonce = bytes.copyOfRange(MAGIC_AES.size, MAGIC_AES.size + NONCE_BYTES)
        val ct    = bytes.copyOfRange(MAGIC_AES.size + NONCE_BYTES, bytes.size)
        return try {
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, SecretKeySpec(keyBytes, "AES"), GCMParameterSpec(GCM_TAG_BITS, nonce))
            String(cipher.doFinal(ct), Charsets.UTF_8)
        } catch (_: Exception) { null }
    }

    // ── Detection ────────────────────────────────────────────────────────────────

    fun isXorEncrypted(hex: String): Boolean =
        (hex.hexToByteArray() ?: return false).startsWith(MAGIC_XOR)

    // ── Migration: XOR (fw0a) → AES-256-GCM (fw1a) ──────────────────────────────

    /**
     * Re-encrypt a single XOR-encrypted field to fw1a.
     * [xorPassphrase] is the key used by the old XOR layer — typically
     *   "fw-${context.filesDir.absolutePath}" (mirrors Rust db_key on Android).
     * [newKeyBytes] is deriveKey(FIELD_KEY_PASSPHRASE) for the new layer.
     * Returns null if XOR decryption fails (record stays unchanged — best-effort).
     */
    fun migrateXorField(xorHex: String, xorPassphrase: String, newKeyBytes: ByteArray): String? {
        val plain = xorDecrypt(xorHex, xorPassphrase) ?: return null
        return encrypt(plain, newKeyBytes)
    }

    // ── XOR legacy decryption ────────────────────────────────────────────────────

    /** Mirror of Rust crypto::decrypt() — exact algorithm port. */
    private fun xorDecrypt(hex: String, passphrase: String): String? {
        val bytes = hex.hexToByteArray() ?: return null
        if (bytes.size < MAGIC_XOR.size || !bytes.startsWith(MAGIC_XOR)) return null
        val cipher   = bytes.copyOfRange(MAGIC_XOR.size, bytes.size)
        val keyBytes = deriveKeyXor(passphrase)
        val plain    = ByteArray(cipher.size) { i -> (cipher[i].toInt() xor keyBytes[i % keyBytes.size].toInt()).toByte() }
        return try { String(plain, Charsets.UTF_8) } catch (_: Exception) { null }
    }

    /** Mirror of Rust fn derive_key_xor: extend passphrase with +0x5c until ≥32, truncate. */
    private fun deriveKeyXor(passphrase: String): ByteArray {
        val key = passphrase.toByteArray(Charsets.UTF_8).toMutableList()
        while (key.size < 32) key.addAll(key.map { (it.toInt() + 0x5c).toByte() })
        return key.take(32).toByteArray()
    }

    // ── Utility extensions ───────────────────────────────────────────────────────

    private fun ByteArray.toHexString(): String = joinToString("") { "%02x".format(it) }

    private fun String.hexToByteArray(): ByteArray? {
        if (length % 2 != 0) return null
        return try { ByteArray(length / 2) { i -> substring(i * 2, i * 2 + 2).toInt(16).toByte() } }
        catch (_: NumberFormatException) { null }
    }

    private fun ByteArray.startsWith(prefix: ByteArray): Boolean {
        if (size < prefix.size) return false
        return prefix.indices.all { this[it] == prefix[it] }
    }

    private operator fun ByteArray.plus(other: ByteArray): ByteArray {
        val r = ByteArray(size + other.size)
        System.arraycopy(this, 0, r, 0, size)
        System.arraycopy(other, 0, r, size, other.size)
        return r
    }
}
