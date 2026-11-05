package pwm.kit

import java.io.ByteArrayOutputStream
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import uniffi.pwm_ffi.assetLinksIncludes
import uniffi.pwm_ffi.passkeyAssetLinksUrl

/**
 * Who asks Android's Credential Manager (D61 §1): a privileged browser,
 * with the origin Android returned for it, or any other app as itself.
 * [cert] is the SHA-256 of the app's signing certificate (64 hex digits),
 * [certs] every certificate it has been signed with, for password links.
 */
class CredentialCaller(
    val packageName: String,
    val cert: String,
    val certs: List<String>,
    /** Only for a browser on the list; null for every other caller. */
    val origin: String?,
) {
    /** The password autofill's caller (D58): the browser's site, or the app. */
    val autofillCaller: Caller
        get() = origin?.let { Caller.Web(it.trimEnd('/') + "/") } ?: Caller.App(packageName, certs.ifEmpty { listOf(cert) })

    /** The site's host for a browser, the package for an app. */
    val label: String
        get() = origin?.removePrefix("https://")?.removePrefix("http://")?.trimEnd('/') ?: packageName

    override fun toString(): String = "CredentialCaller($packageName)"
}

/** A passkey request's caller, with what the core needs to check it. */
class PasskeyCaller(
    val caller: CredentialCaller,
    /** A browser's hash of the client data it built; null for an app. */
    val clientDataHash: ByteArray?,
    /** For an app: the site's Digital Asset Links file and the files it includes. */
    val assetLinks: List<String>,
) {
    override fun toString(): String = "PasskeyCaller(${caller.packageName})"
}

/** What a passkey `create` gets (D61 §5). */
sealed class PasskeyCheck {
    data class Ready(val rpId: String, val userName: String) : PasskeyCheck()

    /** The site excludes a passkey this vault holds: no entry. */
    data object Excluded : PasskeyCheck()

    /** No ES256: no entry. */
    data object Unsupported : PasskeyCheck()
}

/** A stored passkey a sign-in may use; never its key. */
class PasskeyOffer(val vaultId: String, val itemId: String, val credentialId: String, val userName: String, val title: String) {
    override fun toString(): String = "PasskeyOffer(<redacted>)"
}

object CredentialCallers {
    /**
     * The caller for a request. An origin counts only from a browser on
     * D58's list, signed with its listed certificate: Android checks that
     * with [privilegedAllowlist], and this checks it again.
     */
    fun resolve(packageName: String, cert: String, certs: List<String>, origin: String?): CredentialCaller {
        val listed = Callers.BROWSERS[packageName]?.let(Callers::digest)
        val trusted = listed != null && (certs + cert).any { Callers.digest(it) == listed }
        val web = origin?.trim()?.trimEnd('/')?.takeIf { trusted && it.startsWith("https://") }
        return CredentialCaller(packageName, Callers.digest(cert), certs.map(Callers::digest), web)
    }

    /** D58's browsers in the format `CallingAppInfo.getOrigin` reads (Google's privileged-apps list). */
    fun privilegedAllowlist(): String = JsonObject(
        mapOf(
            "apps" to JsonArray(
                Callers.BROWSERS.map { (pkg, cert) ->
                    JsonObject(
                        mapOf(
                            "type" to JsonPrimitive("android"),
                            "info" to JsonObject(
                                mapOf(
                                    "package_name" to JsonPrimitive(pkg),
                                    "signatures" to JsonArray(
                                        listOf(
                                            JsonObject(
                                                mapOf(
                                                    "build" to JsonPrimitive("release"),
                                                    "cert_fingerprint_sha256" to JsonPrimitive(cert),
                                                ),
                                            ),
                                        ),
                                    ),
                                ),
                            ),
                        ),
                    )
                },
            ),
        ),
    ).toString()

    /** 64 hex digits as 32 bytes. */
    fun bytes(hex: String): ByteArray {
        val clean = Callers.digest(hex)
        require(clean.length == 64) { "not a SHA-256 digest" }
        return ByteArray(32) { i -> clean.substring(i * 2, i * 2 + 2).toInt(16).toByte() }
    }
}

/**
 * The sites' Digital Asset Links files (D61 §4), fetched for an app's
 * passkey request: the RP id's file, then the files it includes, one level
 * deep. The core reads them; this only fetches. Files stay in memory for an
 * hour and go at lock and sign-out; a failed fetch isn't kept.
 */
class AssetLinks(
    private val transport: HttpTransport = AssetLinksTransport(),
    private val now: () -> Long = System::currentTimeMillis,
    private val urlFor: (String, Boolean) -> String = { json, create -> core { passkeyAssetLinksUrl(json, create) } },
    private val includesOf: (String) -> List<String> = ::assetLinksIncludes,
) {
    private val cache = mutableMapOf<String, Pair<String, Long>>()

    /** The texts to hand the core with the request; empty when the site's file couldn't be fetched. */
    suspend fun forRequest(requestJson: String, create: Boolean): List<String> = withContext(Dispatchers.IO) {
        val url = try {
            urlFor(requestJson, create)
        } catch (e: PwmError) {
            return@withContext emptyList()
        }
        val site = fetch(url) ?: return@withContext emptyList()
        listOf(site) + includesOf(site).mapNotNull(::fetch)
    }

    /** At lock and sign-out. */
    fun clear() = synchronized(cache) { cache.clear() }

    private fun fetch(url: String): String? {
        synchronized(cache) {
            val kept = cache[url]
            if (kept != null && now() - kept.second < MAX_AGE_MS) return kept.first
        }
        val text = try {
            val response = transport.send(HttpRequest("GET", URL(url), listOf("Accept" to "application/json"), null))
            if (response.status != 200 || response.body.size > MAX_BYTES) return null
            response.body.decodeToString()
        } catch (e: IOException) {
            return null
        } catch (e: IllegalArgumentException) {
            return null
        }
        synchronized(cache) { cache[url] = text to now() }
        return text
    }

    companion object {
        const val MAX_AGE_MS = 60 * 60_000L

        /** As the core's limit (`MAX_ASSET_LINKS`). */
        const val MAX_BYTES = 256 * 1024
    }
}

/**
 * How asset links files are fetched (D61 §4): straight to the site, no
 * cookies, no cache, no redirects (the Digital Asset Links rule), 5
 * seconds, and never more than [AssetLinks.MAX_BYTES] read (one byte more
 * marks a file too large).
 */
class AssetLinksTransport(private val timeoutMs: Int = 5_000) : HttpTransport {
    override fun send(request: HttpRequest): HttpResponse {
        if (request.url.protocol != "https") throw IOException("not https")
        val connection = request.url.openConnection() as HttpURLConnection
        try {
            connection.requestMethod = request.method
            connection.useCaches = false
            connection.instanceFollowRedirects = false
            connection.connectTimeout = timeoutMs
            connection.readTimeout = timeoutMs
            for ((name, value) in request.headers) connection.setRequestProperty(name, value)
            val status = connection.responseCode
            if (status != 200) return HttpResponse(status, emptyList(), ByteArray(0))
            val out = ByteArrayOutputStream()
            connection.inputStream.use { input ->
                val buffer = ByteArray(8 * 1024)
                while (out.size() <= AssetLinks.MAX_BYTES) {
                    val read = input.read(buffer)
                    if (read < 0) break
                    out.write(buffer, 0, read)
                }
            }
            return HttpResponse(status, emptyList(), out.toByteArray())
        } finally {
            connection.disconnect()
        }
    }
}
