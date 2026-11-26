package net.haven.app.core

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import okhttp3.Call
import okhttp3.Callback
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.UnknownHostException
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLException
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/** What can go wrong asking a server something. */
sealed class HttpError(message: String) : IOException(message) {
    class InvalidUrl(val address: String) : HttpError("\"$address\" is not a web address")
    class UnsupportedScheme : HttpError("only http and https addresses can be used")
    class Request(why: String) : HttpError("could not reach the server: $why")
    class Status(val code: Int) : HttpError("the server answered with error $code")
    class TooLarge(val limit: Int) : HttpError("the answer was larger than $limit bytes")

    override fun equals(other: Any?): Boolean =
        other is HttpError && other::class == this::class && other.message == message

    override fun hashCode(): Int = message.hashCode()
}

/** The answer to a request: its status and body, whatever the status. */
class HttpAnswer(val status: Int, val body: ByteArray) {
    val ok: Boolean get() = status in 200..299
    val text: String get() = body.toString(Charsets.UTF_8)
}

/** The web client every network feature shares: Haven's user agent, its
 *  time limits (10 s to connect, 30 s for a whole request unless the caller
 *  says otherwise), and answers that may not grow past a size the caller
 *  sets, so a wrong address can never fill the phone's memory. */
class Http(base: OkHttpClient = OkHttpClient()) {
    private val client: OkHttpClient = base.newBuilder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(DEFAULT_TIMEOUT_SECONDS, TimeUnit.SECONDS)
        .cache(null)
        .addInterceptor { chain ->
            chain.proceed(chain.request().newBuilder().header("User-Agent", Haven.userAgent).build())
        }
        .build()

    companion object {
        val shared: Http by lazy { Http() }
        const val DEFAULT_TIMEOUT_SECONDS = 30L

        /** Parses an address and accepts only http and https. */
        fun webUrl(address: String): HttpUrl {
            val trimmed = address.trim()
            val scheme = trimmed.substringBefore("://", "").lowercase()
            if (scheme.isNotEmpty() && scheme != "http" && scheme != "https") throw HttpError.UnsupportedScheme()
            if (!trimmed.contains("://") && trimmed.contains(":") && !trimmed.contains(" ")) {
                throw HttpError.UnsupportedScheme()
            }
            val url = trimmed.toHttpUrlOrNull() ?: throw HttpError.InvalidUrl(trimmed)
            if (url.host.isEmpty()) throw HttpError.InvalidUrl(trimmed)
            return url
        }

        /** An error in words, without the address (which may carry a key). */
        fun words(error: Throwable): String = when (error) {
            is HttpError -> error.message ?: "network error"
            is SocketTimeoutException -> "the server took too long to answer"
            is UnknownHostException -> "the server's name was not found"
            is ConnectException -> "the server refused the connection"
            is SSLException -> "the secure connection failed"
            else -> error.message?.substringBefore(" http")?.substringBefore(" HTTP") ?: "network error"
        }
    }

    /** Fetches [address], giving up once the body passes [limit] bytes.
     *  Anything but a 2xx answer is an error. */
    suspend fun getBytes(
        address: String,
        limit: Int,
        headers: List<Pair<String, String>> = emptyList(),
        timeoutSeconds: Long = DEFAULT_TIMEOUT_SECONDS,
    ): ByteArray {
        val answer = send(address, headers = headers, limit = limit, timeoutSeconds = timeoutSeconds)
        if (!answer.ok) throw HttpError.Status(answer.status)
        return answer.body
    }

    /** Sends a request and reads the answer, giving up once it passes
     *  [limit] bytes. A body without a method is POSTed as [contentType].
     *  Returns the status with the body whatever the status is, so callers
     *  can read an error's details. */
    suspend fun send(
        address: String,
        method: String? = null,
        headers: List<Pair<String, String>> = emptyList(),
        body: ByteArray? = null,
        contentType: String = "application/json",
        limit: Int,
        timeoutSeconds: Long = DEFAULT_TIMEOUT_SECONDS,
    ): HttpAnswer {
        val url = webUrl(address)
        val verb = method ?: if (body == null) "GET" else "POST"
        val request = Request.Builder().url(url).apply {
            headers.forEach { (name, value) -> header(name, value) }
            method(verb, body?.toRequestBody(contentType.toMediaType()))
        }.build()
        val call = client.newBuilder().callTimeout(timeoutSeconds, TimeUnit.SECONDS).build().newCall(request)
        return suspendCancellableCoroutine { continuation ->
            continuation.invokeOnCancellation { call.cancel() }
            call.enqueue(object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    if (continuation.isActive) {
                        continuation.resumeWithException(if (e is HttpError) e else HttpError.Request(words(e)))
                    }
                }

                override fun onResponse(call: Call, response: Response) {
                    val result = runCatching { response.use { read(it, limit) } }
                    if (!continuation.isActive) return
                    result.fold(
                        { continuation.resume(it) },
                        { continuation.resumeWithException(if (it is HttpError) it else HttpError.Request(words(it))) },
                    )
                }
            })
        }
    }

    /** Opens a request whose answer is read as it arrives (a long list of
     *  stream sources). The caller reads the body and closes the response.
     *  `readTimeoutSeconds` is the longest wait between two parts. */
    suspend fun open(
        address: String,
        method: String = "GET",
        headers: List<Pair<String, String>> = emptyList(),
        body: ByteArray? = null,
        contentType: String = "application/json",
        readTimeoutSeconds: Long = DEFAULT_TIMEOUT_SECONDS,
    ): Response {
        val url = webUrl(address)
        val request = Request.Builder().url(url).apply {
            headers.forEach { (name, value) -> header(name, value) }
            method(method, body?.toRequestBody(contentType.toMediaType()))
        }.build()
        val call = client.newBuilder().readTimeout(readTimeoutSeconds, TimeUnit.SECONDS).build().newCall(request)
        return withContext(Dispatchers.IO) {
            try {
                call.execute()
            } catch (e: IOException) {
                throw if (e is HttpError) e else HttpError.Request(words(e))
            }
        }
    }

    private fun read(response: Response, limit: Int): HttpAnswer {
        val body = response.body ?: return HttpAnswer(response.code, ByteArray(0))
        // A size the server announces past the limit: stop before reading.
        if (body.contentLength() > limit) throw HttpError.TooLarge(limit)
        val out = ByteArrayOutputStream()
        val buffer = ByteArray(16 * 1024)
        body.byteStream().use { stream ->
            while (true) {
                val n = stream.read(buffer)
                if (n < 0) break
                if (out.size() + n > limit) throw HttpError.TooLarge(limit)
                out.write(buffer, 0, n)
            }
        }
        return HttpAnswer(response.code, out.toByteArray())
    }
}
