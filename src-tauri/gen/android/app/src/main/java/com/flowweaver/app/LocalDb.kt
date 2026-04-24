package com.flowweaver.app

// ── LocalDb.kt ─────────────────────────────────────────────────────────────────
// SQLiteOpenHelper that accesses the same DB file as the Rust backend (storage.rs).
//
// This Kotlin class must NOT be used from the main Tauri/WebView activity — the
// Rust layer owns that connection. LocalDb is used exclusively from WorkManager
// background workers (DriveRelayWorker) that run in processes where the Tauri
// WebView is NOT active and therefore cannot use IPC/invoke.
//
// Schema mirrors Rust storage.rs exactly (no divergence allowed):
//   resources(id, uuid, url, title, domain, category, captured_at)
//   UNIQUE INDEX on uuid → drives INSERT OR IGNORE idempotence
//
// D1: url and title are stored encrypted. This class never writes plaintext
//     values for those columns — callers must encrypt before passing them in.
// R12: no Episode Detector, Pattern Detector or longitudinal analysis here.
//      This class only reads/writes the resources table.

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper

class LocalDb(context: Context) : SQLiteOpenHelper(
    context,
    DB_NAME,
    null,
    DB_VERSION
) {

    companion object {
        const val DB_NAME    = "resources.db"
        const val DB_VERSION = 1

        // Column names — must match storage.rs schema exactly.
        private const val TABLE     = "resources"
        private const val COL_ID    = "id"
        private const val COL_UUID  = "uuid"
        private const val COL_URL   = "url"
        private const val COL_TITLE = "title"
        private const val COL_DOM   = "domain"
        private const val COL_CAT   = "category"
        private const val COL_TS    = "captured_at"
    }

    override fun onCreate(db: SQLiteDatabase) {
        // Schema is identical to the one in storage.rs migrate().
        db.execSQL("""
            CREATE TABLE IF NOT EXISTS $TABLE (
                $COL_ID     INTEGER PRIMARY KEY,
                $COL_UUID   TEXT    NOT NULL,
                $COL_URL    TEXT    NOT NULL,
                $COL_TITLE  TEXT    NOT NULL,
                $COL_DOM    TEXT    NOT NULL,
                $COL_CAT    TEXT    NOT NULL,
                $COL_TS     INTEGER NOT NULL DEFAULT 0
            )
        """.trimIndent())
        db.execSQL(
            "CREATE UNIQUE INDEX IF NOT EXISTS idx_resources_uuid ON $TABLE($COL_UUID)"
        )
    }

    override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
        // Mirrors the ALTER TABLE migration in storage.rs migrate() for captured_at.
        if (oldVersion < 1) {
            try {
                db.execSQL(
                    "ALTER TABLE $TABLE ADD COLUMN $COL_TS INTEGER NOT NULL DEFAULT 0"
                )
            } catch (_: Exception) {
                // Column already exists — ignore.
            }
        }
    }

    // ── Read ────────────────────────────────────────────────────────────────────

    /** Return true if [uuid] already exists in the resources table. */
    fun uuidExists(uuid: String): Boolean {
        val db = readableDatabase
        db.rawQuery(
            "SELECT 1 FROM $TABLE WHERE $COL_UUID = ? LIMIT 1", arrayOf(uuid)
        ).use { c -> return c.moveToFirst() }
    }

    /**
     * Return all rows that still use XOR encryption (fw0a magic prefix on url).
     * Used by the migration pass in DriveRelayWorker.
     */
    fun getXorEncryptedRows(): List<ResourceRow> {
        val rows = mutableListOf<ResourceRow>()
        val db = readableDatabase
        // The hex encoding of "fw0a" is "66773061" — check with LIKE prefix.
        db.rawQuery(
            "SELECT $COL_ID, $COL_UUID, $COL_URL, $COL_TITLE FROM $TABLE WHERE $COL_URL LIKE '66773061%'",
            null
        ).use { c ->
            while (c.moveToNext()) {
                rows += ResourceRow(
                    id    = c.getLong(0),
                    uuid  = c.getString(1),
                    url   = c.getString(2),
                    title = c.getString(3)
                )
            }
        }
        return rows
    }

    // ── Write ────────────────────────────────────────────────────────────────────

    /**
     * Insert a resource using INSERT OR IGNORE — idempotent on uuid.
     * Returns true if inserted, false if uuid already existed (skip).
     * D1: [url] and [title] must already be encrypted by the caller.
     */
    fun insertOrIgnore(
        uuid: String,
        urlEncrypted: String,
        titleEncrypted: String,
        domain: String,
        category: String,
        capturedAt: Long
    ): Boolean {
        val db = writableDatabase
        val cv = ContentValues().apply {
            put(COL_UUID,  uuid)
            put(COL_URL,   urlEncrypted)
            put(COL_TITLE, titleEncrypted)
            put(COL_DOM,   domain)
            put(COL_CAT,   category)
            put(COL_TS,    capturedAt)
        }
        val rowId = db.insertWithOnConflict(TABLE, null, cv, SQLiteDatabase.CONFLICT_IGNORE)
        return rowId != -1L
    }

    /**
     * Update url and title for a row identified by [id].
     * Used during XOR → AES-256-GCM migration.
     * D1: [urlEncrypted] and [titleEncrypted] must be AES-encrypted by the caller.
     */
    fun updateEncryptedFields(id: Long, urlEncrypted: String, titleEncrypted: String) {
        val db = writableDatabase
        val cv = ContentValues().apply {
            put(COL_URL,   urlEncrypted)
            put(COL_TITLE, titleEncrypted)
        }
        db.update(TABLE, cv, "$COL_ID = ?", arrayOf(id.toString()))
    }

    data class ResourceRow(
        val id: Long,
        val uuid: String,
        val url: String,
        val title: String
    )
}
