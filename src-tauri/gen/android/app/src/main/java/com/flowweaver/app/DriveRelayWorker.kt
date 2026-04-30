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
import okhttp3.HttpUrl.Companion.toHttpUrl
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
        // Bug #1 fix (sesión 2026-04-30): paired desktop device_id ("desktop-<uuid>")
        // necesario para leer pending/acked del desktop con naming flat compatible con
        // drive_relay.rs::desktop_pending / android_acked.
        private const val PREF_PAIRED_DESKTOP_ID = "paired_desktop_id"

        // R16 (2026-04-29): refresh_token + client credentials persisted on Android
        // so the worker can renew the access_token autonomously when it expires.
        // Without these, sync silently breaks 1h after setup.
        private const val PREF_CLIENT_ID         = "drive_client_id"
        private const val PREF_CLIENT_SECRET     = "drive_client_secret"
        private const val PREF_REFRESH_TOKEN     = "drive_refresh_token"
        private const val PREF_TOKEN_EXPIRES_AT  = "drive_token_expires_at"  // Unix seconds
        // R16 hardening: sticky flag for permanent OAuth failures (invalid_grant
        // = refresh revoked / expired / password changed). When set, the UI must
        // prompt the user to reconnect Drive — the worker stops retrying on its own.
        private const val PREF_OAUTH_STATE       = "drive_oauth_state"        // "" | "invalid_grant"
        private const val OAUTH_STATE_INVALID    = "invalid_grant"

        // Google Drive AppData folder — access restricted to this app only.
        private const val DRIVE_UPLOAD_URL =
            "https://www.googleapis.com/upload/drive/v3/files?uploadType=multipart"
        private const val DRIVE_FILES_URL  =
            "https://www.googleapis.com/drive/v3/files"
        private const val OAUTH_TOKEN_URL  =
            "https://oauth2.googleapis.com/token"

        // Timeout for events without ACK — after this they are removed from the queue.
        private const val ACK_TIMEOUT_MS   = 7L * 24 * 60 * 60 * 1000 // 7 days

        // Refresh access_token if less than this many seconds remain. 60s buffer
        // avoids racing with token expiration mid-request.
        private const val TOKEN_REFRESH_BUFFER_S = 60L
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

        // R16: refresh access_token if needed; distinguish transient vs permanent failures.
        // Permanent failure (invalid_grant) → Result.failure() so WorkManager stops the
        // backoff loop. Transient (5xx, network) → Result.retry() to back off and retry.
        val accessToken  = when (val tk = ensureValidAccessToken()) {
            is TokenResult.Valid         -> tk.token
            is TokenResult.RetryLater    -> return@withContext Result.retry()
            is TokenResult.Unrecoverable -> {
                Log.w(TAG, "OAuth permanently failed (${tk.reason}) — user must reconnect Drive")
                return@withContext Result.failure()
            }
        }
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
        // Bug #1 fix: requiere paired_desktop_id ("desktop-<uuid>") para resolver
        // los nombres flat del desktop (drive_relay.rs convention).
        val pairedDesktopId = prefs.getString(PREF_PAIRED_DESKTOP_ID, null)
        if (pairingKey != null && pairedDesktopId != null) {
            try {
                downloadDesktopEvents(deviceId, pairedDesktopId, accessToken, pairingKey, fieldKey)
            } catch (e: Exception) {
                Log.w(TAG, "Download Desktop→Android failed: ${e.message}")
                return@withContext Result.retry()
            }
        } else {
            Log.d(TAG, "No pairing key or paired_desktop_id — skipping desktop→android direction")
        }

        Result.success()
    }

    // ── R16: OAuth access_token lifecycle ────────────────────────────────────────

    /**
     * Outcome of an attempt to obtain a valid access_token.
     *
     *   Valid          — usable access_token, fresh or just-refreshed.
     *   RetryLater     — transient failure (5xx, network, timeout, missing credentials
     *                    on first run). The Worker should retry with WorkManager backoff.
     *   Unrecoverable  — permanent failure (4xx with error=invalid_grant: refresh_token
     *                    revoked / expired / password changed / invalid_client). Retrying
     *                    will not help — the user must reconnect Drive. WorkManager must
     *                    stop retrying so we don't loop silently burning battery.
     */
    private sealed class TokenResult {
        data class Valid(val token: String) : TokenResult()
        object RetryLater : TokenResult()
        data class Unrecoverable(val reason: String) : TokenResult()
    }

    /**
     * Returns a non-expired access_token for Drive API calls.
     *
     * R16 hardening: distinguishes transient OAuth failures from permanent ones.
     *   - Valid current token         → Valid
     *   - Token expired, refresh OK   → Valid (with new token persisted)
     *   - Network/5xx/missing creds   → RetryLater
     *   - 4xx invalid_grant / 401     → Unrecoverable (sticky pref set)
     *
     * Detected during OAuth setup on 2026-04-29 (HO-024).
     */
    private fun ensureValidAccessToken(): TokenResult {
        // Sticky permanent-failure flag — once set, the worker stops trying until
        // the user reconnects Drive (which clears this pref on the desktop side).
        val oauthState = prefs.getString(PREF_OAUTH_STATE, "") ?: ""
        if (oauthState == OAUTH_STATE_INVALID) {
            return TokenResult.Unrecoverable("oauth_state=$oauthState (sticky)")
        }

        val current      = prefs.getString(PREF_DRIVE_TOKEN, null)
        val expiresAt    = prefs.getLong(PREF_TOKEN_EXPIRES_AT, 0L)
        val nowSec       = System.currentTimeMillis() / 1000

        if (current != null && nowSec < expiresAt - TOKEN_REFRESH_BUFFER_S) {
            return TokenResult.Valid(current)
        }

        val clientId     = prefs.getString(PREF_CLIENT_ID, null)
        val clientSecret = prefs.getString(PREF_CLIENT_SECRET, null)
        val refreshToken = prefs.getString(PREF_REFRESH_TOKEN, null)

        if (clientId.isNullOrBlank() || clientSecret.isNullOrBlank() || refreshToken.isNullOrBlank()) {
            // First run before configure_drive completes, or partial wipe. Not permanent.
            Log.w(TAG, "No refresh credentials persisted — cannot renew access_token (R16)")
            return TokenResult.RetryLater
        }

        Log.i(TAG, "Refreshing access_token via refresh_token (R16 mitigation)")

        return try {
            val formBody = ("grant_type=refresh_token" +
                    "&client_id=" + java.net.URLEncoder.encode(clientId, "UTF-8") +
                    "&client_secret=" + java.net.URLEncoder.encode(clientSecret, "UTF-8") +
                    "&refresh_token=" + java.net.URLEncoder.encode(refreshToken, "UTF-8"))
                .toByteArray(Charsets.UTF_8)

            val req = Request.Builder()
                .url(OAUTH_TOKEN_URL)
                .post(formBody.toRequestBody("application/x-www-form-urlencoded".toMediaType()))
                .build()

            http.newCall(req).execute().use { resp ->
                val body = resp.body?.string().orEmpty()

                if (resp.isSuccessful) {
                    val json     = JSONObject(body)
                    val newToken = json.optString("access_token").takeIf { it.isNotBlank() }
                        ?: return TokenResult.RetryLater
                    val expiresIn    = json.optLong("expires_in", 3600L)
                    val newExpiresAt = nowSec + expiresIn

                    prefs.edit()
                        .putString(PREF_DRIVE_TOKEN, newToken)
                        .putLong(PREF_TOKEN_EXPIRES_AT, newExpiresAt)
                        .apply()

                    Log.i(TAG, "access_token refreshed; expires_at=$newExpiresAt")
                    return TokenResult.Valid(newToken)
                }

                // 4xx — possibly permanent. Inspect the error code.
                if (resp.code in 400..499) {
                    val errorCode = try {
                        JSONObject(body).optString("error", "")
                    } catch (_: Exception) { "" }

                    // invalid_grant → refresh_token revoked / expired / password changed.
                    // invalid_client → client_id/client_secret bad.
                    // Both are permanent for this Worker; user must reconnect Drive.
                    val permanent = errorCode == "invalid_grant" ||
                                    errorCode == "invalid_client" ||
                                    resp.code == 400 || resp.code == 401
                    if (permanent) {
                        Log.w(TAG, "Token refresh permanently failed: code=${resp.code} error=$errorCode")
                        // Set sticky flag so subsequent Worker runs short-circuit.
                        prefs.edit()
                            .putString(PREF_OAUTH_STATE, OAUTH_STATE_INVALID)
                            .apply()
                        return TokenResult.Unrecoverable("http=${resp.code} error=$errorCode")
                    }

                    // Other 4xx (rate limit on the token endpoint, transient policy reject).
                    Log.w(TAG, "Token refresh transient 4xx: code=${resp.code} error=$errorCode")
                    return TokenResult.RetryLater
                }

                // 5xx → transient.
                Log.w(TAG, "Token refresh server error: code=${resp.code}")
                return TokenResult.RetryLater
            }
        } catch (e: Exception) {
            // Network exception, timeout, DNS failure — transient.
            Log.w(TAG, "Token refresh exception: ${e.message}")
            TokenResult.RetryLater
        }
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

            // Bug #1 fix: naming flat compatible con drive_relay.rs::android_pending_prefix.
            // Construcción delegada a RelayNaming (Phase 2.3 — gate test cross-lang).
            val remoteName = RelayNaming.androidPending(deviceId, eventId)
            driveUploadOrUpdate(token, remoteName, eventJson)
            Log.d(TAG, "Uploaded Android event $eventId as $remoteName")
        }
    }

    // ── Direction 2: Read ACKs from desktop, clean local queue ──────────────────

    private fun readAndroidAcks(deviceId: String, token: String) {
        // Bug #1 fix: prefix flat. ACK name = "fw-<android_id>-acked-<event_id>.json".
        val ackedPrefix = RelayNaming.androidAckedPrefix(deviceId)
        val files       = driveListFilesByPrefix(token, ackedPrefix) ?: return
        val queueDir    = pendingQueueDir() ?: return

        for (fileId in files) {
            val meta = driveGetFileMeta(token, fileId) ?: continue
            val name = meta.optString("name")
            val eventId = name.removePrefix(ackedPrefix).removeSuffix(".json")
            if (eventId.isBlank()) continue
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
        pairedDesktopId: String,
        token: String,
        pairingKeyHex: String,
        fieldKey: ByteArray
    ) {
        // Bug #1 fix: naming flat. Desktop emite con su propio device_id ("desktop-<uuid>").
        val pendingPrefix = RelayNaming.desktopPendingPrefix(pairedDesktopId)
        val driveFiles    = driveListFilesByPrefix(token, pendingPrefix) ?: return

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
                writeDesktopAck(token, pairedDesktopId, eventId)
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
            writeDesktopAck(token, pairedDesktopId, eventId)
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
            ?: "otro"
    }

    private fun exactLookup(d: String): String? = when (d) {
        "github.com", "gitlab.com", "bitbucket.org", "stackoverflow.com",
        "stackexchange.com", "npmjs.com", "crates.io", "pypi.org",
        "docs.rs", "pkg.go.dev", "codepen.io", "replit.com",
        "jsfiddle.net", "codesandbox.io", "leetcode.com",
        "hackerrank.com", "codewars.com", "rust-lang.org",
        "golang.org", "python.org", "developer.apple.com",
        "developer.android.com", "developer.mozilla.org",
        "hub.docker.com", "registry.hub.docker.com" -> "desarrollo"

        "notion.so", "notionhq.com", "obsidian.md", "roamresearch.com",
        "craft.do", "evernote.com", "onenote.com", "bear.app",
        "logseq.com", "remnote.com", "workflowy.com" -> "notas"

        "figma.com", "dribbble.com", "behance.net", "sketch.com",
        "invisionapp.com", "zeplin.io", "canva.com", "adobe.com",
        "framer.com", "webflow.com", "storybook.js.org",
        "coolors.co", "fontawesome.com", "fonts.google.com" -> "diseño"

        "wistia.com", "loom.com", "screencast.com" -> "vídeo"

        "imdb.com", "youtube.com", "youtu.be", "netflix.com",
        "vimeo.com", "dailymotion.com", "hulu.com", "primevideo.com",
        "disneyplus.com", "hbomax.com", "max.com", "appletv.com",
        "crunchyroll.com", "funimation.com", "rottentomatoes.com",
        "letterboxd.com", "themoviedb.org" -> "entretenimiento"

        "store.steampowered.com", "steampowered.com", "twitch.tv",
        "itch.io", "epicgames.com", "gog.com", "origin.com",
        "xbox.com", "playstation.com", "nintendo.com",
        "gamespot.com", "ign.com", "kotaku.com" -> "gaming"

        "bbc.com", "bbc.co.uk", "elpais.com", "elmundo.es",
        "reuters.com", "apnews.com", "theguardian.com",
        "nytimes.com", "washingtonpost.com", "lemonde.fr",
        "spiegel.de", "publico.es", "elconfidencial.com",
        "lavanguardia.com", "20minutos.es" -> "noticias"

        "coursera.org", "udemy.com", "edx.org", "khanacademy.org",
        "pluralsight.com", "skillshare.com", "lynda.com",
        "udacity.com", "freecodecamp.org",
        "codecademy.com", "brilliant.org", "duolingo.com" -> "educación"

        "spotify.com", "soundcloud.com", "bandcamp.com",
        "music.apple.com", "tidal.com", "deezer.com",
        "last.fm", "genius.com", "musixmatch.com",
        "audiomack.com", "mixcloud.com" -> "música"

        "google.com", "airtable.com", "trello.com", "asana.com",
        "monday.com", "linear.app", "atlassian.com", "slack.com",
        "discord.com", "zoom.us", "microsoft.com", "office.com",
        "outlook.com", "clickup.com", "basecamp.com",
        "todoist.com", "ticktick.com" -> "productividad"

        "medium.com", "substack.com", "dev.to", "hashnode.com",
        "hackernoon.com", "techcrunch.com", "theverge.com",
        "wired.com", "news.ycombinator.com", "lobste.rs",
        "indiehackers.com", "smashingmagazine.com", "css-tricks.com",
        "alistapart.com", "increment.com" -> "artículos"

        "twitter.com", "x.com", "linkedin.com", "reddit.com",
        "facebook.com", "instagram.com", "pinterest.com",
        "mastodon.social", "threads.net", "bsky.app" -> "social"

        "amazon.com", "gumroad.com", "stripe.com", "shopify.com",
        "etsy.com", "ebay.com", "paypal.com", "paddle.com",
        "lemonsqueezy.com", "revenuecat.com", "fastspring.com" -> "comercio"

        "arxiv.org", "scholar.google.com", "pubmed.ncbi.nlm.nih.gov",
        "semanticscholar.org", "researchgate.net", "jstor.org",
        "ncbi.nlm.nih.gov", "nature.com", "science.org",
        "acm.org", "ieee.org", "springer.com", "wiley.com",
        "sciencedirect.com", "plos.org" -> "investigación"

        else -> null
    }

    // ── Desktop field decryption (fw1a — pairing key) ────────────────────────────

    /**
     * Decrypt a fw1a (AES-256-GCM) field from a desktop raw_event using the shared
     * pairing key (hex). Delegates to RelayCrypto so the same code is exercised by
     * cross-language JVM unit tests (Phase 2.2).
     */
    private fun decryptDesktopField(hexField: String?, keyHex: String): String? {
        val plain = RelayCrypto.decryptFw1a(hexField, keyHex)
        if (plain == null && !hexField.isNullOrBlank()) {
            Log.w(TAG, "AES-GCM decrypt failed for desktop field (magic/auth/malformed)")
        }
        return plain
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

    private fun writeDesktopAck(token: String, pairedDesktopId: String, eventId: String) {
        // Bug #1 fix: naming flat. Match desktop_acked() en drive_relay.rs.
        // Bug #6 fix: check if ACK already exists before uploading to avoid duplicates.
        // driveGetFileId now works correctly (Fix A), so this check is reliable.
        val ackName = RelayNaming.desktopAcked(pairedDesktopId, eventId)
        try {
            if (driveGetFileId(token, ackName) != null) {
                Log.d(TAG, "ACK already exists for desktop event $eventId — skipping upload")
                return
            }
            val ackBody = """{"event_id":"$eventId","acked_at":${System.currentTimeMillis()}}"""
            driveUploadOrUpdate(token, ackName, ackBody)
            Log.d(TAG, "ACK written for desktop event $eventId as $ackName")
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
     * Upload or update a file in Drive AppData. Naming is flat (Bug #1 fix):
     * `remoteName` is the full file name (no slashes). Match drive_relay.rs convention.
     * If a file with [remoteName] already exists, its content is replaced (idempotent).
     */
    private fun driveUploadOrUpdate(token: String, remoteName: String, content: String) {
        // Check if file already exists so we can PATCH instead of POST.
        val existingId = driveGetFileId(token, remoteName)

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
            // POST new file with flat name in appDataFolder (no nested folders).
            val metadata = JSONObject().apply {
                put("name", remoteName)
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

    /** Return the Drive file ID for a given flat name in AppData, or null if not found. */
    private fun driveGetFileId(token: String, name: String): String? {
        // Bug #6 fix: build q with HttpUrl.Builder so addQueryParameter URL-encodes
        // the value. Previous raw-string construction produced "name 'x'" (no operator),
        // which Drive API rejected — causing driveUploadOrUpdate to always POST new files.
        val url = DRIVE_FILES_URL.toHttpUrl().newBuilder()
            .addQueryParameter("spaces", "appDataFolder")
            .addQueryParameter("fields", "files(id,name)")
            .addQueryParameter("q", "name = '$name' and trashed = false")
            .build()
        val req = Request.Builder()
            .url(url)
            .addHeader("Authorization", "Bearer $token")
            .get()
            .build()
        return try {
            http.newCall(req).execute().use { resp ->
                if (!resp.isSuccessful) return null
                val json  = JSONObject(resp.body!!.string())
                val files = json.optJSONArray("files") ?: return null
                if (files.length() > 0) files.getJSONObject(0).optString("id") else null
            }
        } catch (e: Exception) {
            Log.w(TAG, "driveGetFileId failed: ${e.message}")
            null
        }
    }

    /**
     * List file IDs whose name starts with [prefix] (flat naming, Bug #1).
     * Returns null on network error.
     */
    private fun driveListFilesByPrefix(token: String, prefix: String): List<String>? {
        val url    = "$DRIVE_FILES_URL?spaces=appDataFolder&fields=files(id,name)&q=name+contains+'$prefix'+and+trashed=false"
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
            Log.w(TAG, "driveListFilesByPrefix failed: ${e.message}")
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

}
