package net.haven.app.app

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import net.haven.app.core.Store

/** "What's New": this version's notes, over Haven right after the loading
 *  screen. It shows at every start after an update until "Don't show again"
 *  is ticked; a fresh install never sees it. The notes are built in
 *  (`whats-new.json`, the Rust build's file, copied in at build time), so
 *  every build shows the same ones. */
object WhatsNew {
    /** Which version's notes this device has put away (kv). */
    const val SEEN_KEY = "whats_new_seen"

    data class Notes(val version: String, val sections: List<Section>) {
        data class Section(val title: String, val lines: List<String>)
    }

    fun parse(text: String): Notes? {
        val o = runCatching { Json.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return null
        val version = (o["version"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: return null
        val sections = o["sections"] as? JsonArray ?: return null
        return Notes(version, sections.mapNotNull { item ->
            val section = item as? JsonObject ?: return@mapNotNull null
            val title = (section["title"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: return@mapNotNull null
            val lines = section["lines"] as? JsonArray ?: return@mapNotNull null
            Notes.Section(title, lines.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content })
        })
    }

    /** Whether the notes show at this start. `existing` is false on a fresh
     *  install (no settings saved yet): those notes then count as seen. */
    fun due(notes: Notes?, store: Store, existing: Boolean): Boolean {
        notes ?: return false
        if (!existing) {
            runCatching { store.put(SEEN_KEY, notes.version) }
            return false
        }
        return runCatching { store.get<String>(SEEN_KEY) }.getOrNull() != notes.version
    }

    /** Done was pressed: with "Don't show again" ticked, these notes are put away. */
    fun done(notes: Notes?, store: Store, dontShowAgain: Boolean) {
        if (!dontShowAgain || notes == null) return
        runCatching { store.put(SEEN_KEY, notes.version) }
    }
}
