package com.flowweaver.app

// ── MainActivity.kt ────────────────────────────────────────────────────────────
// Main entry point for the Tauri Android app.
// Registers the periodic WorkManager task for the Drive relay (T-0c-002).
//
// R12: this file registers the WorkManager Worker only. No Episode Detector,
//   Pattern Detector or longitudinal analysis is started here.

import android.os.Bundle
import androidx.activity.enableEdgeToEdge
import androidx.work.Constraints
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import java.util.concurrent.TimeUnit

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)

    // Ensure device_id is created before the first Worker run.
    ShareIntentActivity.getOrCreateDeviceId(this)

    // Register the periodic relay Worker (15 min, network required).
    // Policy KEEP: if a Worker with this name is already enqueued, leave it running.
    // This is the only background task — no observer, no polling of files (D9).
    val constraints = Constraints.Builder()
      .setRequiredNetworkType(NetworkType.CONNECTED)
      .build()

    val syncRequest = PeriodicWorkRequestBuilder<DriveRelayWorker>(
      15, TimeUnit.MINUTES
    )
      .setConstraints(constraints)
      .addTag("flowweaver_periodic_sync")
      .build()

    WorkManager.getInstance(applicationContext).enqueueUniquePeriodicWork(
      "flowweaver_sync",
      ExistingPeriodicWorkPolicy.KEEP,
      syncRequest
    )
  }
}
