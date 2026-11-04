package pwm.kit

import java.io.IOException
import java.net.http.HttpClient
import java.net.http.HttpRequest as JdkRequest
import java.net.http.HttpResponse as JdkResponse

/**
 * The JVM tests' transport: the JDK's `HttpURLConnection` refuses PATCH
 * (renaming a device), while Android's accepts it, so the server test sends
 * through `java.net.http` instead.
 */
class JdkTransport : HttpTransport {
    private val client = HttpClient.newHttpClient()

    override fun send(request: HttpRequest): HttpResponse {
        val body = request.body?.let { JdkRequest.BodyPublishers.ofByteArray(it) } ?: JdkRequest.BodyPublishers.noBody()
        val builder = JdkRequest.newBuilder(request.url.toURI()).method(request.method, body)
        for ((name, value) in request.headers) {
            if (name.equals("content-length", ignoreCase = true) || name.equals("host", ignoreCase = true)) continue
            builder.header(name, value)
        }
        val response = try {
            client.send(builder.build(), JdkResponse.BodyHandlers.ofByteArray())
        } catch (e: InterruptedException) {
            throw IOException(e)
        }
        val headers = response.headers().map().flatMap { (name, values) -> values.map { name to it } }
        return HttpResponse(response.statusCode(), headers, response.body())
    }
}
