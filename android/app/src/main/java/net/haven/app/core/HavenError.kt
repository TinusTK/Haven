package net.haven.app.core

/** What can go wrong in Haven's own files and database (Swift `HavenError`). */
sealed class HavenError(message: String) : Exception(message) {
    class Io(val path: String, why: String) : HavenError("could not use $path: $why")
    class Database(why: String) : HavenError("database error: $why")
    class Json(why: String) : HavenError("stored data could not be read: $why")
    class DatabaseTooNew(val found: Int, val known: Int) :
        HavenError("this database was made by a newer Haven (schema $found, this build knows $known)")

    override fun equals(other: Any?): Boolean =
        other is HavenError && other::class == this::class && other.message == message

    override fun hashCode(): Int = message.hashCode()
}

/** A failure already put in words for the person using Haven. */
class Problem(val words: String) : Exception(words)

/** Milliseconds since 1970, as Haven stores times. */
fun nowMillis(): Long = System.currentTimeMillis()
