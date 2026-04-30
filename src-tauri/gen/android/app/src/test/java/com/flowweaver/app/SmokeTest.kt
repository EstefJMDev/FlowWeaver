package com.flowweaver.app

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Phase 2.1 spike — verifies that gradle picks up app/src/test/java as a JVM
 * unit test source set with junit:4.13.2 already declared in app/build.gradle.kts.
 * If this fails, Phase 2.2 (Test A) and Phase 2.3 (Test B) cannot run on JVM and
 * the cross-language gate must be reconsidered (instrumented test on device).
 */
class SmokeTest {

    @Test
    fun arithmetic_smoke() {
        assertEquals(4, 2 + 2)
    }

    @Test
    fun relay_crypto_object_is_loadable() {
        // Triggers RelayCrypto class loading from the main source set — confirms
        // the JVM test classpath sees the production code.
        val key32 = RelayCrypto.deriveKey("0123456789abcdef")
        assertEquals(32, key32.size)
    }
}
