package com.flowweaver.app

// ── ShareIntentActivity.kt ─────────────────────────────────────────────────────
// Receives ACTION_SEND intents from the Android share sheet (T-0b-android-001).
// Classifies the URL, encrypts url+title (D1), writes the raw_event to the local
// relay queue, and enqueues a one-shot DriveRelayWorker for fast upload.
//
// R12: this activity does NOT invoke Episode Detector, Pattern Detector, Session
//   Builder or any longitudinal analysis. It classifies a single URL and enqueues
//   the raw_event for transport to the desktop.
// D1: url and title are encrypted before any storage (FieldCrypto, AES-256-GCM
//   via Android Keystore). domain and category travel in clear per D1 spec.
// D9: no background observer, no clipboard polling, no accessibility services.
//   Capture is purely explicit — triggered by the user through the share sheet.

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.Log
import android.widget.TextView
import android.widget.Toast
import androidx.work.Constraints
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import org.json.JSONObject
import java.io.File
import java.util.UUID

class ShareIntentActivity : Activity() {

    companion object {
        private const val TAG              = "ShareIntentActivity"
        private const val PREFS_NAME       = "flowweaver_relay"
        private const val PREF_DEVICE_ID   = "device_id"
        private const val SCHEMA_VERSION   = 1

        /** Generate or retrieve the stable device_id for this Android installation. */
        fun getOrCreateDeviceId(context: Context): String {
            val prefs = context.getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
            val existing = prefs.getString(PREF_DEVICE_ID, null)
            if (existing != null) return existing
            val id = "android-${UUID.randomUUID()}"
            prefs.edit().putString(PREF_DEVICE_ID, id).apply()
            Log.i(TAG, "Created new device_id: $id")
            return id
        }
    }

    private var pendingUndoFile: File? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Only handle ACTION_SEND with text/plain MIME type.
        if (intent?.action != Intent.ACTION_SEND || intent.type != "text/plain") {
            Log.d(TAG, "Ignoring non-text share intent")
            finish()
            return
        }

        val rawText = intent.getStringExtra(Intent.EXTRA_TEXT) ?: run {
            Toast.makeText(this, "No URL recibida", Toast.LENGTH_SHORT).show()
            finish()
            return
        }

        // Basic URL validation — reject plain text that is not a URL.
        if (!rawText.startsWith("http://") && !rawText.startsWith("https://")) {
            Toast.makeText(this, "Solo se admiten URLs", Toast.LENGTH_SHORT).show()
            finish()
            return
        }

        val titleRaw = intent.getStringExtra(Intent.EXTRA_SUBJECT) ?: ""

        // ── Pipeline (< 300ms budget — all local, no network) ──────────────────
        val deviceId   = getOrCreateDeviceId(this)
        val eventId    = UUID.randomUUID().toString()
        val capturedAt = System.currentTimeMillis()

        // Extract domain and classify — deterministic, same table as Rust (D8, R12).
        val domain   = extractDomain(rawText)
        val category = classifyDomain(domain)

        // D1: encrypt url and title with Android Keystore AES-256-GCM before any storage.
        val urlEncrypted   = FieldCrypto.encrypt(rawText)
        val titleEncrypted = FieldCrypto.encrypt(titleRaw)

        // Build raw_event JSON — matches TS-0b-android-001 schema.
        // domain and category are in clear (D1). url and title are encrypted.
        val rawEvent = JSONObject().apply {
            put("event_id",        eventId)
            put("device_id",       deviceId)
            put("source",          "android")
            put("captured_at",     capturedAt)
            put("domain",          domain)
            put("category",        category)
            put("url_encrypted",   urlEncrypted)
            put("title_encrypted", titleEncrypted)
            put("schema_version",  SCHEMA_VERSION)
        }

        // Write to local relay queue (Drive Worker will upload asynchronously).
        val queueFile = writeToQueue(eventId, rawEvent.toString())
        pendingUndoFile = queueFile

        // Also insert immediately into local SQLite so the gallery shows it right away.
        val db = LocalDb(this)
        db.insertOrIgnore(
            uuid           = eventId,
            urlEncrypted   = urlEncrypted,
            titleEncrypted = titleEncrypted,
            domain         = domain,
            category       = category,
            capturedAt     = capturedAt
        )
        db.close()

        // Trigger immediate (one-shot) upload attempt — best-effort if network available.
        triggerImmediateSync()

        // ── Confirmation UI (4-second overlay) ────────────────────────────────
        showConfirmationAndClose(domain, category, eventId)
    }

    // ── Local queue write ────────────────────────────────────────────────────────

    private fun writeToQueue(eventId: String, json: String): File {
        val dir = File(filesDir, "relay_queue")
        dir.mkdirs()
        val file = File(dir, "$eventId.json")
        file.writeText(json, Charsets.UTF_8)
        Log.d(TAG, "Enqueued event $eventId to relay queue")
        return file
    }

    // ── One-shot WorkManager trigger ─────────────────────────────────────────────

    private fun triggerImmediateSync() {
        val constraints = Constraints.Builder()
            .setRequiredNetworkType(NetworkType.CONNECTED)
            .build()
        val request = OneTimeWorkRequestBuilder<DriveRelayWorker>()
            .setConstraints(constraints)
            .addTag("flowweaver_immediate_sync")
            .build()
        WorkManager.getInstance(applicationContext).enqueue(request)
        Log.d(TAG, "Immediate sync WorkRequest enqueued")
    }

    // ── Confirmation UI ──────────────────────────────────────────────────────────

    private fun showConfirmationAndClose(domain: String, category: String, eventId: String) {
        // Simple toast confirmation — a full card overlay can replace this in a later sprint.
        // The category feedback confirms the classifier ran (D8).
        val message = "Guardado en FlowWeaver\n$category · $domain"
        Toast.makeText(this, message, Toast.LENGTH_LONG).show()

        // Auto-close after 4 seconds — or immediately on Undo tap.
        Handler(Looper.getMainLooper()).postDelayed({
            finish()
        }, 4000)
    }

    // ── Domain extraction (mirrors Rust classifier.rs extract_domain) ────────────

    private fun extractDomain(url: String): String {
        val s = if (url.contains("://")) url.substringAfter("://") else url
        val host = s.split('/', '?', '#').firstOrNull() ?: ""
        val hostNoPort = host.split(':').firstOrNull() ?: ""
        val clean = hostNoPort.removePrefix("www.")
        return if (clean.isEmpty()) "unknown" else clean.lowercase()
    }

    // ── Deterministic domain classifier (D8, R12) ────────────────────────────────

    private fun classifyDomain(domain: String): String {
        val d = domain.lowercase()
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
}
