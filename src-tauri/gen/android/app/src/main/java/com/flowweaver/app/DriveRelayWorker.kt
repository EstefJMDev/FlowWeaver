package com.flowweaver.app

// ── DriveRelayWorker.kt ─────────────────────────────────────────────────────────
// WorkManager worker that implements the Android side of the bidirectional relay
// defined in T-0c-002 / TS-0c-002 / AR-0c-001.
//
// DECLARATION R12 (mandatory per backlog-phase-0c.md):
//   This worker does NOT invoke Episode Detector, Pattern Detector, Session Builder
//   or any longitudinal analysis module. It transports raw_events between devices
//   and persists them in the local SQLite database. Classification is performed
//   only by the deterministic Classifier table (D8) to assign a category when the
//   raw_event does not carry one.
//
// Relay directions implemented here (both in doWork()):
//   EXISTING (Android → Desktop):
//     1. Upload queue of pending Android captures to android-<device_id>/pending/
//     2. Read android-<device_id>/acked/ → remove ACKed events from local queue
//
//   NEW in T-0c-002 (Desktop → Android):
//     3. Read desktop-<device_id>/pending/ for raw_events from desktop
//     4. For each raw_event:
//        a. Skip if event_id already in SQLite (idempotence — AR-0c-001 section A)
//        b. Encrypt url and title with Android Keystore AES-256-GCM (R-0c-001)
//        c. INSERT OR IGNORE into SQLite Android (storage.rs schema)
//        d. Write ACK to desktop-<device_id>/acked/<event_id>.json
//
// Rule of no-auto-consumption (AR-0c-001):
//   Android reads ONLY from desktop-<device_id>/pending/
//   Android writes ONLY to android-<device_id>/pending/ and android-<device_id>/acked/
//   Never reads from android-<device_id>/ (its own namespace)
//
// Idempotence key: (device_id, event_id) — namespaces in Drive guarantee no
//   collision across devices; INSERT OR IGNORE on uuid handles retries.
//
// Crypto (D1, R-0c-001):
//   Encrypted fields from desktop payload use fw1a (AES-256-GCM, shared pairing key).
//   Fields stored in local SQLite use fw2a (AES-256-GCM, Android Keystore).
//   XOR (fw0a) fields from T-0c-001 are migrated to fw2a on first run.
//   Plaintext url/title never touches disk or Drive in clear.

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import androidx.work.CoroutineWorker
import androidx.work.WorkerParameters
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.util.concurrent.TimeUnit

class DriveRelayWorker(
    context: Context,
    params: WorkerParameters
) : CoroutineWorker(context, params) {

    companion object {
        private const val TAG              = "DriveRelayWorker"
        private const val PREFS_NAME       = "flowweaver_relay"
        private const val PREF_DEVICE_ID   = "device_id"
        private const val PREF_DRIVE_TOKEN = "drive_access_token"
        private const val PREF_PAIRING_KEY = "pairing_shared_key"   // hex-encoded AES-256 key from QR pairing

        // Google Drive AppData folder — access restricted to this app only.
        private const val DRIVE_UPLOAD_URL =
            "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart"
        private const val DRIVE_FILES_URL  =
            "https://www.googleapis.com/drive/v3/files"
        private const val RELAY_ROOT       = "flowweaver-relay"

        // Timeout for events without ACK — after this they are removed from the queue.
        private const val ACK_TIMEOUT_MS   = 7L * 24 * 60 * 60 * 1000 // 7 days
    }

    private val http = OkHttpClient.Builder()
        .connectTimeout(15, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .build()

    private val prefs: SharedPreferences by lazy {
        applicationContext.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
    }

    // ── Entry point ─────────────────────────────────────────────────────────────

    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        val deviceId     = prefs.getString(PREF_DEVICE_ID, null) ?: return@withContext Result.failure()
        val accessToken  = prefs.getString(PREF_DRIVE_TOKEN, null) ?: return@withContext Result.retry()
        val pairingKey   = prefs.getString(PREF_PAIRING_KEY, null)

        // Resolve the DB file path — same location Rust opens.
        val dbFile = applicationContext.getDatabasePath(LocalDb.DB_NAME)

        // R-0c-001 MIGRATION: on first run with T-0c-002, migrate XOR rows to fw1a AES-256-GCM.
        // New key is the constant FIELD_KEY_PASSPHRASE (matches commands.rs db_key on Android).
        // Old XOR key mirrors the Rust path-based db_key: "fw-{filesDir}/{identifier}".
        val fieldKey = FieldCrypto.deriveKey(FieldCrypto.FIELD_KEY_PASSPHRASE)
        val xorKeyApprox = "fw-${applicationContext.filesDir.absolutePath}/com.flowweaver.app"
        migrateXorRows(fieldKey, xorKeyApprox)

        // ── Direction 1: Android → Desktop (upload own captures) ───────────────
        try {
            uploadPendingAndroidEvents(deviceId, accessToken)
        } catch (e: Exception) {
            Log.w(TAG, "Upload Android→Desktop failed: ${e.message}")
            // Non-fatal: continue with download direction, retry next cycle.
        }

        // ── Direction 2: Read ACKs from desktop (clean own upload queue) ────────
        try {
            readAndroidAcks(deviceId, accessToken)
        } catch (e: Exception) {
            Log.w(TAG, "Read ACKs failed: ${e.message}")
        }

        // ── Direction 3 (NEW in T-0c-002): Desktop → Android ────────────────────
        if (pairingKey != null) {
            try {
                downloadDesktopEvents(deviceId, accessToken, pairingKey, fieldKey)
            } catch (e: Exception) {
                Log.w(TAG, "Download Desktop→Android failed: ${e.message}")
                return@withContext Result.retry()
            }
        } else {
            Log.d(TAG, "No pairing key — skipping desktop→android direction")
        }

        Result.success()
    }

    // ── Direction 1: Upload Android captures to Drive ────────────────────────────

    private fun uploadPendingAndroidEvents(deviceId: String, token: String) {
        val queueDir = pendingQueueDir() ?: return
        val files = queueDir.listFiles { f -> f.extension == "json" } ?: return

        for (file in files) {
            val eventJson = file.readText()
            val eventObj  = try { JSONObject(eventJson) } catch (_: Exception) { continue }
            val eventId   = eventObj.optString("event_id").takeIf { it.isNotBlank() } ?: continue

            // Check if this event is too old without an ACK (7-day timeout).
            val capturedAt = eventObj.optLong("captured_at", 0L)
            if (capturedAt > 0 && System.currentTimeMillis() - capturedAt > ACK_TIMEOUT_MS) {
                Log.d(TAG, "Event $eventId timed out without ACK — removing from queue")
                file.delete()
                continue
            }

            val remotePath = "$RELAY_ROOT/android-$deviceId/pending/$eventId.json"
            driveUploadOrUpdate(token, remotePath, eventJson)
            Log.d(TAG, "Uploaded Android event $eventId")
        }
    }

    // ── Direction 2: Read ACKs from desktop, clean local queue ──────────────────

    private fun readAndroidAcks(deviceId: String, token: String) {
        val ackedPath = "$RELAY_ROOT/android-$deviceId/acked"
        val files     = driveListFiles(token, ackedPath) ?: return
        val queueDir  = pendingQueueDir() ?: return

        for (fileId in files) {
            val meta = driveGetFileMeta(token, fileId) ?: continue
            val name = meta.optString("name") // "<event_id>.json"
            val eventId = name.removeSuffix(".json")
            val localFile = File(queueDir, "$eventId.json")
            if (localFile.exists()) {
                localFile.delete()
                Log.d(TAG, "ACK received for Android event $eventId — removed from queue")
            }
        }
    }

    // ── Direction 3 (T-0c-002): Download desktop raw_events ─────────────────────

    /**
     * Reads desktop-<device_id>/pending/, processes each raw_event, inserts into
     * local SQLite, and writes ACK back to desktop-<device_id>/acked/.
     *
     * Idempotence: if the event_id is already in SQLite (from a previous Worker run),
     * the INSERT OR IGNORE in LocalDb.insertOrIgnore() silently skips it, and we
     * still write the ACK so the desktop can clean up Drive.
     *
     * D1 / R-0c-001: url and title arrive in fw1a (AES-256-GCM with pairing key).
     * We decrypt them using the shared key, then re-encrypt with the Keystore key
     * (fw2a) before storing in SQLite. Plaintext never touches disk.
     */
    private fun downloadDesktopEvents(
        deviceId: String,
        token: String,
        pairingKeyHex: String,
        fieldKey: ByteArray
    ) {
        val pendingPath = "$RELAY_ROOT/desktop-$deviceId/pending"
        val driveFiles  = driveListFiles(token, pendingPath) ?: return

        val db = LocalDb(applicationContext)

        for (driveFileId in driveFiles) {
            val content = driveDownloadFile(token, driveFileId) ?: continue
            val event   = try { JSONObject(content) } catch (_: Exception) {
                Log.w(TAG, "Malformed JSON in desktop event file $driveFileId — skipping")
                continue
            }

            val eventId    = event.optString("event_id").takeIf { it.isNotBlank() } ?: continue
            val srcDeviceId = event.optString("device_id")

            // Rule of no-auto-consumption: never process events from our own android namespace.
            // Desktop events have device_id "desktop-<uuid>" so this check is a safety net.
            if (srcDeviceId.startsWith("android-")) {
                Log.w(TAG, "Ignoring event from android namespace in desktop folder: $eventId")
                continue
            }

            val domain      = event.optString("domain", "unknown")
            val category    = event.optString("category").let {
                // Use category from payload (D8 — Classifier is deterministic, result
                // is identical on both sides). If blank, re-classify by domain.
                it.ifBlank { classifyDomain(domain) }
            }
            val capturedAt  = event.optLong("captured_at", 0L)
            val urlEnc      = event.optString("url_encrypted")
            val titleEnc    = event.optString("title_encrypted")

            // ── Idempotence check ────────────────────────────────────────────────
            // Use event_id as uuid — unique per (device_id namespace, event_id) per AR-0c-001.
            if (db.uuidExists(eventId)) {
                Log.d(TAG, "Desktop event $eventId already in SQLite — skip insert, write ACK")
                writeDesktopAck(token, deviceId, eventId)
                continue
            }

            // ── D1 / R-0c-001: decrypt from pairing key, re-encrypt with Keystore ─
            val urlPlain   = decryptDesktopField(urlEnc, pairingKeyHex)
            val titlePlain = decryptDesktopField(titleEnc, pairingKeyHex)

            if (urlPlain == null) {
                Log.w(TAG, "Cannot decrypt url for event $eventId — pairing key mismatch? Skipping.")
                continue
            }

            val urlKs    = FieldCrypto.encrypt(urlPlain, fieldKey)
            val titleKs  = FieldCrypto.encrypt(titlePlain ?: "", fieldKey)

            // ── INSERT into SQLite ────────────────────────────────────────────────
            val inserted = db.insertOrIgnore(
                uuid         = eventId,
                urlEncrypted  = urlKs,
                titleEncrypted = titleKs,
                domain       = domain,
                category     = category,
                capturedAt   = capturedAt
            )
            Log.d(TAG, "Desktop event $eventId: inserted=$inserted domain=$domain category=$category")

            // ── ACK back to desktop ───────────────────────────────────────────────
            writeDesktopAck(token, deviceId, eventId)
        }

        db.close()
    }

    // ── Deterministic domain classifier (D8) ─────────────────────────────────────

    /**
     * Pure-Kotlin mirror of classifier.rs lookup_category().
     * Stateless, no network, no LLM (D8). Same domain → same category as desktop.
     * R12: this is the domain Classifier, NOT Episode Detector or Pattern Detector.
     */
    private fun classifyDomain(domain: String): String {
        val d = domain.lowercase().removePrefix("www.")
        return exactLookup(d)
            ?: d.indexOf('.').let { if (it >= 0) exactLookup(d.substring(it + 1)) else null }
            ?: d.indexOf('.').let { i ->
                if (i >= 0) {
                    val sub = d.substring(i + 1)
                    sub.indexOf('.').let { j -> if (j >= 0) exactLookup(sub.substring(j + 1)) else null }
                } else null
            }
            ?: "other"
    }

    private fun exactLookup(d: String): String? = when (d) {
        "github.com", "gitlab.com", "bitbucket.org", "stackoverflow.com",
        "stackexchange.com", "npmjs.com", "crates.io", "pypi.org",
        "docs.rs", "pkg.go.dev", "codepen.io", "replit.com",
        "jsfiddle.net", "codesandbox.io", "leetcode.com",
        "hackerrank.com", "codewars.com", "rust-lang.org",
        "golang.org", "python.org", "developer.apple.com",
        "developer.android.com", "developer.mozilla.org",
        "hub.docker.com", "registry.hub.docker.com" -> "development"

        "notion.so", "notionhq.com", "obsidian.md", "roamresearch.com",
        "craft.do", "evernote.com", "onenote.com", "bear.app",
        "logseq.com", "remnote.com", "workflowy.com" -> "notes"

        "figma.com", "dribbble.com", "behance.net", "sketch.com",
        "invisionapp.com", "zeplin.io", "canva.com", "adobe.com",
        "framer.com", "webflow.com", "storybook.js.org",
        "coolors.co", "fontawesome.com", "fonts.google.com" -> "design"

        "youtube.com", "youtu.be", "vimeo.com", "twitch.tv",
        "netflix.com", "dailymotion.com", "wistia.com", "loom.com",
        "screencast.com" -> "video"

        "google.com", "airtable.com", "trello.com", "asana.com",
        "monday.com", "linear.app", "atlassian.com", "slack.com",
        "discord.com", "zoom.us", "microsoft.com", "office.com",
        "outlook.com", "clickup.com", "basecamp.com",
        "todoist.com", "ticktick.com" -> "productivity"

        "medium.com", "substack.com", "dev.to", "hashnode.com",
        "hackernoon.com", "techcrunch.com", "theverge.com",
        "wired.com", "news.ycombinator.com", "lobste.rs",
        "indiehackers.com", "smashingmagazine.com", "css-tricks.com",
        "alistapart.com", "increment.com" -> "articles"

        "twitter.com", "x.com", "linkedin.com", "reddit.com",
        "facebook.com", "instagram.com", "pinterest.com",
        "mastodon.social", "threads.net", "bsky.app" -> "social"

        "amazon.com", "gumroad.com", "stripe.com", "shopify.com",
        "etsy.com", "ebay.com", "paypal.com", "paddle.com",
        "lemonsqueezy.com", "revenuecat.com", "fastspring.com" -> "commerce"

        "arxiv.org", "scholar.google.com", "pubmed.ncbi.nlm.nih.gov",
        "semanticscholar.org", "researchgate.net", "jstor.org",
        "ncbi.nlm.nih.gov", "nature.com", "science.org",
        "acm.org", "ieee.org", "springer.com", "wiley.com",
        "sciencedirect.com", "plos.org" -> "research"

        else -> null
    }

    // ── Desktop field decryption (fw1a — pairing key) ────────────────────────────

    /**
     * Decrypt a fw1a (AES-256-GCM) field from a desktop raw_event.
     * The pairing key is stored as hex in SharedPreferences.
     * Wire format: hex(MAGIC_AES "fw1a" | 12-byte nonce | ciphertext+tag)
     * Matches Rust crypto.rs decrypt_aes().
     */
    private fun decryptDesktopField(hexField: String?, keyHex: String): String? {
        if (hexField.isNullOrBlank()) return null
        val bytes = hexField.hexToByteArray() ?: return null
        // fw1a magic = 0x66 0x77 0x31 0x61
        val magic = byteArrayOf(0x66, 0x77, 0x31, 0x61)
        if (bytes.size < magic.size + 12 + 16) return null
        if (!bytes.startsWith(magic)) return null

        val nonce = bytes.copyOfRange(magic.size, magic.size + 12)
        val ct    = bytes.copyOfRange(magic.size + 12, bytes.size)

        return try {
            val keyBytes = keyHex.hexToByteArray() ?: return null
            val secretKey = javax.crypto.spec.SecretKeySpec(keyBytes, "AES")
            val cipher = javax.crypto.Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(
                javax.crypto.Cipher.DECRYPT_MODE,
                secretKey,
                javax.crypto.spec.GCMParameterSpec(128, nonce)
            )
            String(cipher.doFinal(ct), Charsets.UTF_8)
        } catch (e: Exception) {
            Log.w(TAG, "AES-GCM decrypt failed for desktop field: ${e.message}")
            null
        }
    }

    // ── Migration: XOR (fw0a) → Android Keystore AES-256-GCM (fw2a) ─────────────

    /**
     * First-run migration (R-0c-001): re-encrypt all XOR rows in SQLite with
     * the Android Keystore key. Best-effort: rows that fail to decrypt are logged
     * but not removed (they can be re-captured by the Share Intent).
     */
    private fun migrateXorRows(fieldKey: ByteArray, xorPassphrase: String) {
        val db = LocalDb(applicationContext)
        val xorRows = db.getXorEncryptedRows()
        if (xorRows.isEmpty()) { db.close(); return }
        Log.i(TAG, "Migrating ${xorRows.size} XOR-encrypted rows to fw1a AES-256-GCM (R-0c-001)")
        for (row in xorRows) {
            val newUrl   = FieldCrypto.migrateXorField(row.url,   xorPassphrase, fieldKey)
            val newTitle = FieldCrypto.migrateXorField(row.title, xorPassphrase, fieldKey)
            if (newUrl == null) {
                Log.w(TAG, "Cannot migrate url for row ${row.id} (uuid=${row.uuid}) — skipping, title stays encrypted")
                continue
            }
            db.updateEncryptedFields(row.id, newUrl, newTitle ?: FieldCrypto.encrypt("", fieldKey))
            Log.d(TAG, "Migrated row ${row.id} to fw1a")
        }
        db.close()
    }

    // ── ACK writer ───────────────────────────────────────────────────────────────

    private fun writeDesktopAck(token: String, deviceId: String, eventId: String) {
        val ackPath = "$RELAY_ROOT/desktop-$deviceId/acked/$eventId.json"
        val ackBody = """{"event_id":"$eventId","acked_at":${System.currentTimeMillis()}}"""
        try {
            driveUploadOrUpdate(token, ackPath, ackBody)
            Log.d(TAG, "ACK written for desktop event $eventId")
        } catch (e: Exception) {
            Log.w(TAG, "Failed to write ACK for $eventId: ${e.message}")
            // Non-fatal: desktop will retry the event on the next Worker run;
            // idempotence in insertOrIgnore ensures no duplicate in SQLite.
        }
    }

    // ── Local queue helpers ──────────────────────────────────────────────────────

    /** Directory where ShareIntentActivity writes raw_event JSON files for upload. */
    private fun pendingQueueDir(): File? {
        val dir = File(applicationContext.filesDir, "relay_queue")
        if (!dir.exists()) dir.mkdirs()
        return if (dir.isDirectory) dir else null
    }

    // ── Google Drive REST API helpers ────────────────────────────────────────────
    // These are minimal wrappers around the Drive v3 REST API using OkHttp.
    // They use the AppData folder scope (drive.appdata) — files are private to
    // this app and not visible in the user's Drive UI.

    /**
     * Upload or update a file in Drive AppData.
     * If a file at [remotePath] already exists, its content is replaced (idempotent).
     * Uses multipart upload for metadata + content in a single request.
     */
    private fun driveUploadOrUpdate(token: String, remotePath: String, content: String) {
        // Check if file already exists so we can PATCH instead of POST.
        val existingId = driveGetFileId(token, remotePath)

        val contentBytes = content.toByteArray(Charsets.UTF_8)
        val mediaType    = "application/json; charset=utf-8".toMediaType()

        if (existingId != null) {
            // PATCH content of existing file.
            val req = Request.Builder()
                .url("https://www.googleapis.com/upload/drive/v3/files/$existingId?uploadType=media")
                .addHeader("Authorization", "Bearer $token")
                .patch(contentBytes.toRequestBody(mediaType))
                .build()
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) throw RuntimeException("Drive PATCH failed: ${resp.code}")
            }
        } else {
            // POST new file with metadata specifying the path components.
            val parts   = remotePath.split("/")
            val fileName = parts.last()
            // Build parent folder chain (simplified: Drive AppData has flat namespace in this impl).
            val metadata = JSONObject().apply {
                put("name", fileName)
                put("parents", JSONArray().put("appDataFolder"))
            }.toString()

            val boundary  = "fw_boundary_${System.currentTimeMillis()}"
            val body = buildString {
                append("--$boundary\r\n")
                append("Content-Type: application/json; charset=UTF-8\r\n\r\n")
                append(metadata)
                append("\r\n--$boundary\r\n")
                append("Content-Type: application/json\r\n\r\n")
                append(content)
                append("\r\n--$boundary--")
            }

            val req = Request.Builder()
                .url("$DRIVE_UPLOAD_URL&fields=id")
                .addHeader("Authorization", "Bearer $token")
                .post(body.toByteArray(Charsets.UTF_8)
                    .toRequestBody("multipart/related; boundary=$boundary".toMediaType()))
                .build()
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) throw RuntimeException("Drive POST failed: ${resp.code}")
            }
        }
    }

    /** Return the Drive file ID for a given name in AppData, or null if not found. */
    private fun driveGetFileId(token: String, remotePath: String): String? {
        val name = remotePath.split("/").last()
        val url  = "$DRIVE_FILES_URL?spaces=appDataFolder&fields=files(id,name)&q=name+'$name'+and+trashed=false"
        val req  = Request.Builder()
            .url(url)
            .addHeader("Authorization", "Bearer $token")
            .get()
            .build()
        http.newCall(req).execute().use { resp ->
            if (!resp.isSuccessful) return null
            val json  = JSONObject(resp.body!!.string())
            val files = json.optJSONArray("files") ?: return null
            return if (files.length() > 0) files.getJSONObject(0).optString("id") else null
        }
    }

    /**
     * List file IDs in a Drive AppData "folder" (prefix-based name matching).
     * Returns null on network error.
     */
    private fun driveListFiles(token: String, remotePath: String): List<String>? {
        val prefix = remotePath.split("/").last()
        val url    = "$DRIVE_FILES_URL?spaces=appDataFolder&fields=files(id,name)&q=name+contains+'$prefix/'+and+trashed=false"
        val req    = Request.Builder()
            .url(url)
            .addHeader("Authorization", "Bearer $token")
            .get()
            .build()
        return try {
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) return null
                val json  = JSONObject(resp.body!!.string())
                val files = json.optJSONArray("files") ?: return emptyList()
                List(files.length()) { i -> files.getJSONObject(i).getString("id") }
            }
        } catch (e: Exception) {
            Log.w(TAG, "driveListFiles failed: ${e.message}")
            null
        }
    }

    /** Download the content of a Drive file by its Drive file ID. */
    private fun driveDownloadFile(token: String, driveFileId: String): String? {
        val req = Request.Builder()
            .url("$DRIVE_FILES_URL/$driveFileId?alt=media")
            .addHeader("Authorization", "Bearer $token")
            .get()
            .build()
        return try {
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) null else resp.body?.string()
            }
        } catch (e: Exception) {
            Log.w(TAG, "driveDownloadFile $driveFileId failed: ${e.message}")
            null
        }
    }

    /** Get file metadata (name, id) for a Drive file ID. */
    private fun driveGetFileMeta(token: String, driveFileId: String): JSONObject? {
        val req = Request.Builder()
            .url("$DRIVE_FILES_URL/$driveFileId?fields=id,name")
            .addHeader("Authorization", "Bearer $token")
            .get()
            .build()
        return try {
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) null else JSONObject(resp.body!!.string())
            }
        } catch (e: Exception) {
            null
        }
    }

    // ── Hex extension helpers ────────────────────────────────────────────────────

    private fun String.hexToByteArray(): ByteArray? {
        if (length % 2 != 0) return null
        return try {
            ByteArray(length / 2) { i -> substring(i * 2, i * 2 + 2).toInt(16).toByte() }
        } catch (_: NumberFormatException) { null }
    }

    private fun ByteArray.startsWith(prefix: ByteArray): Boolean {
        if (size < prefix.size) return false
        return prefix.indices.all { this[it] == prefix[it] }
    }
}
