package pwm.kit

import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL

/** One request the core asked for, at a full URL. */
class HttpRequest(
    val method: String,
    val url: URL,
    val headers: List<Pair<String, String>>,
    val body: ByteArray?,
) {
    override fun toString(): String = "HttpRequest($method ${url.path})"
}

class HttpResponse(val status: Int, val headers: List<Pair<String, String>>, val body: ByteArray) {
    override fun toString(): String = "HttpResponse($status)"
}

/** Sends requests; tests use a fake. A transport failure throws [IOException]. */
fun interface HttpTransport {
    @Throws(IOException::class)
    fun send(request: HttpRequest): HttpResponse

    /** The same transport giving up on a request after [ms]; a fake may ignore it. */
    fun withTimeout(ms: Int): HttpTransport = this
}

/**
 * The system's HTTP stack (D52 §4): no cookies, no cache, TLS left to the
 * platform, and a timeout so a dead network ends the step loop.
 */
class UrlConnectionTransport(private val timeoutMs: Int = 30_000) : HttpTransport {
    override fun withTimeout(ms: Int): HttpTransport = UrlConnectionTransport(ms)

    override fun send(request: HttpRequest): HttpResponse {
        val connection = request.url.openConnection() as HttpURLConnection
        try {
            connection.requestMethod = request.method
            connection.useCaches = false
            connection.instanceFollowRedirects = false
            connection.connectTimeout = timeoutMs
            connection.readTimeout = timeoutMs
            for ((name, value) in request.headers) connection.setRequestProperty(name, value)
            request.body?.let { body ->
                connection.doOutput = true
                connection.setFixedLengthStreamingMode(body.size)
                connection.outputStream.use { it.write(body) }
            }
            val status = connection.responseCode
            val stream = if (status >= 400) connection.errorStream else connection.inputStream
            val body = stream?.use { it.readBytes() } ?: ByteArray(0)
            val headers = connection.headerFields
                .filterKeys { it != null }
                .flatMap { (name, values) -> values.map { name.lowercase() to it } }
            return HttpResponse(status, headers, body)
        } finally {
            connection.disconnect()
        }
    }
}
