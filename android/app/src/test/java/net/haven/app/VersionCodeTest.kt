package net.haven.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class VersionCodeTest {
    @Test
    fun buildThreeOfTheCurrentVersion() {
        assertEquals(20_020_603, VersionCode.of("0.2.6", 3))
    }

    @Test
    fun everyCodeIsAboveTheRustAppsAndRises() {
        assertEquals(16_777_734, VersionCode.rust("0.2.6"))
        assertTrue(VersionCode.of("0.2.6", 1) > VersionCode.rust("0.9.99"))
        assertTrue(VersionCode.of("0.2.6", 4) > VersionCode.of("0.2.6", 3))
        assertTrue(VersionCode.of("0.2.7", 1) > VersionCode.of("0.2.6", 99))
        assertTrue(VersionCode.of("0.3.0", 1) > VersionCode.of("0.2.99", 99))
        assertTrue(VersionCode.of("1.0.0", 1) > VersionCode.of("0.99.99", 99))
    }
}
