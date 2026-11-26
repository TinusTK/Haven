package net.haven.app.app

import androidx.compose.ui.graphics.vector.PathParser
import net.haven.app.core.Settings
import net.haven.app.core.Store
import net.haven.app.ui.IconPaths
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class ShellTest {
    private fun store(): Store = Store.open(File.createTempFile("haven-test-", ".sqlite3").also { it.delete(); it.deleteOnExit() })

    @Test fun pagesAreNumberedAsTheRustBuildSavedThem() {
        assertEquals(listOf("Home", "Explore", "Library", "Live TV", "Calendar", "Settings"), Page.entries.map { it.label })
        assertEquals(Page.LiveTV, Page.of(3))
        assertEquals(Page.Settings, Page.of(5))
        assertEquals(Page.Home, Page.of(6))
        assertEquals(Page.Home, Page.of(-1))
    }

    @Test fun layoutFollowsTheScreen() {
        assertEquals(Layout.Phone, Layout.of(false, 411f))
        assertEquals(Layout.Tablet, Layout.of(false, 600f))
        assertEquals(Layout.Tablet, Layout.of(false, 1280f))
        assertEquals(Layout.TV, Layout.of(true, 960f))
    }

    @Test fun loadingBarNeverRunsAheadOfTheClockInTheFirstSecond() {
        assertEquals(0.5, AppModel.splashTarget(1.0, 0.5), 1e-9)
        assertEquals(0.3, AppModel.splashTarget(0.3, 0.5), 1e-9)
        assertEquals(1.0, AppModel.splashTarget(1.0, 1.5), 1e-9)
        assertEquals(0.4, AppModel.splashTarget(0.4, 5.0), 1e-9)
        // Never more than 12 seconds.
        assertEquals(1.0, AppModel.splashTarget(0.1, 12.0), 1e-9)
        assertEquals(0.0, AppModel.splashTarget(-1.0, 3.0), 1e-9)
        assertEquals("Loading…  42%", AppModel.splashText(0.42))
        assertEquals("Ready", AppModel.splashText(1.0))
    }

    @Test fun whatsNewReadsTheBuiltInNotes() {
        val text = File("../../app/crates/haven-app/whats-new.json").readText()
        val notes = WhatsNew.parse(text)!!
        assertTrue(notes.version.isNotEmpty())
        assertTrue(notes.sections.isNotEmpty())
        // Never mentions add-ons (the store builds show the same notes).
        assertFalse(text.lowercase().contains("add-on") || text.lowercase().contains("addon"))
        assertNull(WhatsNew.parse("nope"))
        assertNull(WhatsNew.parse("""{"sections": []}"""))
        assertEquals(
            WhatsNew.Notes("1.0", listOf(WhatsNew.Notes.Section("A", listOf("x")))),
            WhatsNew.parse("""{"version": "1.0", "sections": [{"title": "A", "lines": ["x", 3]}, {"title": 1}]}"""),
        )
    }

    @Test fun whatsNewShowsAfterAnUpdateUntilPutAway() {
        val notes = WhatsNew.Notes("0.2.6", emptyList())
        store().use { fresh ->
            // A fresh install never sees it, nor at the next start.
            assertFalse(WhatsNew.due(notes, fresh, existing = false))
            assertFalse(WhatsNew.due(notes, fresh, existing = true))
        }
        store().use { updated ->
            Settings().save(updated)
            assertTrue(WhatsNew.due(notes, updated, existing = true))
            WhatsNew.done(notes, updated, dontShowAgain = false)
            assertTrue(WhatsNew.due(notes, updated, existing = true))
            WhatsNew.done(notes, updated, dontShowAgain = true)
            assertFalse(WhatsNew.due(notes, updated, existing = true))
            // The next version's notes show again.
            assertTrue(WhatsNew.due(WhatsNew.Notes("0.2.7", emptyList()), updated, existing = true))
            assertFalse(WhatsNew.due(null, updated, existing = true))
        }
    }

    @Test fun everyIconDrawingReads() {
        assertEquals(28, IconPaths.drawings.size)
        Page.entries.forEach { assertTrue(it.icon, IconPaths.drawings.containsKey(it.icon)) }
        IconPaths.drawings.forEach { (name, d) -> assertTrue(name, PathParser().parsePathString(d).toNodes().isNotEmpty()) }
    }
}
