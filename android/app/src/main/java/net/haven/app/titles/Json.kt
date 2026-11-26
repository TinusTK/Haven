package net.haven.app.titles

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Reading the JSON that catalogue services, media servers and add-ons send.
 * The same field can arrive as text, a number, a list or nothing, so the
 * lenient readers accept all of those, as the Rust build did. The strict
 * ones fail on a wrong type, as serde did (which drops that entry, or fails
 * the whole answer, depending on the caller). Swift `JSONObject`.
 */
class JObj(val raw: JsonObject) {
    /** Something went wrong reading a field. */
    class Wrong(val field: String) : Exception("$field has the wrong type")

    companion object {
        fun of(value: JsonElement?): JObj? = (value as? JsonObject)?.let { JObj(it) }

        /** Parses JSON text whose top level is an object. */
        fun parse(text: String): JObj {
            val value = Json.parseToJsonElement(text)
            // kotlinx reads a bare word ("nope") as a value; it is not JSON.
            if (value is JsonPrimitive && !value.isString && value !is JsonNull &&
                !isBool(value) && value.content.toDoubleOrNull() == null
            ) {
                throw IllegalArgumentException("not JSON")
            }
            return of(value) ?: throw Wrong("(top)")
        }

        fun isBool(p: JsonPrimitive): Boolean = !p.isString && (p.content == "true" || p.content == "false")

        /** Written with a point or exponent ("8.4", "8.0"), not a whole number. */
        fun isFloat(p: JsonPrimitive): Boolean =
            !p.isString && !isBool(p) && p.content.any { it == '.' || it == 'e' || it == 'E' }

        fun text(value: JsonElement?): String? {
            val p = value as? JsonPrimitive ?: return null
            if (p is JsonNull) return null
            if (p.isString) return p.content.takeIf { it.isNotBlank() }
            if (isBool(p)) return null
            if (isFloat(p)) {
                val d = p.content.toDoubleOrNull() ?: return null
                return if (d.isFinite()) d.toString() else null
            }
            return p.content
        }
    }

    operator fun get(key: String): JsonElement? = raw[key]?.takeUnless { it is JsonNull }

    fun has(key: String): Boolean = get(key) != null

    /** Whether the key is there at all, even as null. */
    fun present(key: String): Boolean = raw.containsKey(key)

    // Strict (as serde's String / Option<String> / Vec<String>)

    fun string(key: String): String = (get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content ?: throw Wrong(key)

    /** Missing or null is null; anything but text is wrong. */
    fun optionalString(key: String): String? {
        val value = get(key) ?: return null
        return (value as? JsonPrimitive)?.takeIf { it.isString }?.content ?: throw Wrong(key)
    }

    /** Missing is empty; anything but a list of text is wrong. */
    fun strings(key: String): List<String> {
        val value = get(key) ?: return emptyList()
        val list = value as? JsonArray ?: throw Wrong(key)
        return list.map { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content ?: throw Wrong(key) }
    }

    /** Missing or null is null; anything but a list of text is wrong. */
    fun optionalStrings(key: String): List<String>? = if (get(key) == null) null else strings(key)

    fun bool(key: String): Boolean {
        val value = get(key) ?: return false
        val p = value as? JsonPrimitive
        if (p == null || !isBool(p)) throw Wrong(key)
        return p.content == "true"
    }

    // Lenient

    /** Text or a number, as text. Anything else, or blank text, is null. */
    fun textOrNumber(key: String): String? = text(get(key))

    /** A whole number sent as a number or as text. */
    fun number(key: String): Int? {
        val p = get(key) as? JsonPrimitive ?: return null
        if (p.isString) return p.content.trim().toUIntOrNull()?.toLong()?.toInt()
        if (isBool(p) || isFloat(p)) return null // serde's as_u64: whole numbers only
        val value = p.content.toLongOrNull() ?: return null
        return if (value in 0..UInt.MAX_VALUE.toLong()) value.toInt() else null
    }

    /** A list of text, a single text, or nothing. */
    fun texts(key: String): List<String> = when (val value = get(key)) {
        is JsonArray -> value.mapNotNull { text(it) }
        else -> listOfNotNull(text(value))
    }
}
