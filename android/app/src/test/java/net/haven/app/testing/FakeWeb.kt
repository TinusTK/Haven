package net.haven.app.testing

import net.haven.app.core.Http
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody

/** Answers requests from a table of address -> (status, body), without a
 *  network (Swift `FixtureServer`). Each test makes its own. */
class FakeWeb {
    private val table = mutableMapOf<String, Pair<Int, String>>()
    private val counts = mutableMapOf<String, Int>()
    private val bodies = mutableMapOf<String, String>()

    fun answer(url: String, body: String, status: Int = 200) = synchronized(this) { table[url] = status to body }

    /** How many times `url` was asked for. */
    fun count(url: String): Int = synchronized(this) { counts[url] ?: 0 }

    /** The body of the last request to `url`. */
    fun sent(url: String): String? = synchronized(this) { bodies[url] }

    val http: Http = Http(OkHttpClient.Builder().addInterceptor(Interceptor { chain ->
        val request = chain.request()
        val url = request.url.toString()
        val (status, body) = synchronized(this) {
            counts[url] = (counts[url] ?: 0) + 1
            request.body?.let { b -> okio.Buffer().also { b.writeTo(it) }.readUtf8() }?.let { bodies[url] = it }
            table[url] ?: (404 to "")
        }
        Response.Builder().request(request).protocol(Protocol.HTTP_1_1).code(status).message("fake")
            .body(body.toResponseBody("application/json".toMediaType())).build()
    }).build())
}

/** The test answers copied from the Rust build (haven-addons/tests/fixtures). */
object Fixture {
    private fun text(name: String): String =
        Fixture::class.java.getResource("/fixtures/$name.json")!!.readText()

    val manifest get() = text("manifest")
    val catalog get() = text("catalog")
    val meta get() = text("meta")
    val streams get() = text("streams")
}
