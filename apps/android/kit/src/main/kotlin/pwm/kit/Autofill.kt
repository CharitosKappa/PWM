package pwm.kit

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.floatOrNull

/**
 * A field on the screen asking to be filled (D58 §2), as the autofill
 * service reads it: never its value. [id] is the platform's handle for it.
 */
class ScreenField<Id>(
    val id: Id,
    val autofillHints: List<String> = emptyList(),
    /** Android's `InputType` bits. */
    val inputType: Int = 0,
    val resourceId: String? = null,
    val hint: String? = null,
    /** For web pages: the HTML tag and attributes Android passes. */
    val htmlTag: String? = null,
    val htmlAttributes: Map<String, String> = emptyMap(),
    val visible: Boolean = true,
) {
    override fun toString(): String = "ScreenField(<redacted>)"
}

/** Who is asking (D58 §3): a known browser on a site, or an app as itself. */
sealed class Caller {
    /** A browser on the list, reporting the page's address. */
    data class Web(val url: String) : Caller()

    /** Any other app, by its package and signing certificates' SHA-256 digests. */
    data class App(val packageName: String, val certs: List<String>) : Caller()

    /** The address the core matches and classifies against. */
    val pageUrl: String
        get() = when (this) {
            is Web -> url
            is App -> "android-app://$packageName"
        }
}

object Callers {
    /**
     * The browsers whose web domain is believed (D58 §3): package and the
     * SHA-256 of its release signing certificate, from Google's list of
     * browsers privileged for Credential Manager.
     */
    val BROWSERS: Map<String, String> = mapOf(
        "com.android.chrome" to "F0:FD:6C:5B:41:0F:25:CB:25:C3:B5:33:46:C8:97:2F:AE:30:F8:EE:74:11:DF:91:04:80:AD:6B:2D:60:DB:83",
        "org.mozilla.firefox" to "A7:8B:62:A5:16:5B:44:94:B2:FE:AD:9E:76:A2:80:D2:2D:93:7F:EE:62:51:AE:CE:59:94:46:B2:EA:31:9B:04",
        "com.microsoft.emmx" to "01:E1:99:97:10:A8:2C:27:49:B4:D5:0C:44:5D:C8:5D:67:0B:61:36:08:9D:0A:76:6A:73:82:7C:82:A1:EA:C9",
        "com.brave.browser" to "9C:2D:B7:05:13:51:5F:DB:FB:BC:58:5B:3E:DF:3D:71:23:D4:DC:67:C9:4F:FD:30:63:61:C1:D7:9B:BF:18:AC",
        "com.sec.android.app.sbrowser" to "34:DF:0E:7A:9F:1C:F1:89:2E:45:C0:56:B4:97:3C:D8:1C:CF:14:8A:40:50:D1:1A:EA:4A:C5:A6:5F:90:0A:42",
        "com.duckduckgo.mobile.android" to "BB:7B:B3:1C:57:3C:46:A1:DA:7F:C5:C5:28:A6:AC:F4:32:10:84:56:FE:EC:50:81:0C:7F:33:69:4E:B3:D2:D4",
        "com.vivaldi.browser" to "E8:A7:85:44:65:5B:A8:C0:98:17:F7:32:76:8F:56:89:B1:66:2E:C4:B2:BC:5A:0B:C0:EC:13:8D:33:CA:3D:1E",
    )

    /**
     * The caller for a request: a web page only when a listed browser, signed
     * with its listed certificate, reports the domain. Any other app's claim
     * of a domain is ignored, and it is matched as the app it is.
     */
    fun resolve(packageName: String, certs: List<String>, webDomain: String?, webScheme: String?): Caller {
        val listed = BROWSERS[packageName]?.let(::digest)
        val trusted = listed != null && certs.any { digest(it) == listed }
        val domain = webDomain?.trim()?.takeIf { it.isNotEmpty() && it.none(Char::isWhitespace) }
        if (trusted && domain != null) {
            val scheme = if (webScheme == "http") "http" else "https"
            return Caller.Web("$scheme://$domain/")
        }
        return Caller.App(packageName, certs)
    }

    /** 64 lowercase hex digits, colons dropped. */
    fun digest(text: String): String = text.replace(":", "").lowercase()
}

/** Which of the screen's fields are what (D58 §2). */
class FieldRoles<Id>(val username: Id?, val password: Id?, val newPassword: Id?, val oneTimeCode: Id?) {
    val isEmpty: Boolean get() = username == null && password == null && newPassword == null && oneTimeCode == null
}

object FieldMapping {
    private const val CLASS_MASK = 0x0000000f
    private const val VARIATION_MASK = 0x00000ff0
    private const val CLASS_TEXT = 0x1
    private const val CLASS_NUMBER = 0x2
    private const val TEXT_PASSWORD = 0x80
    private const val TEXT_VISIBLE_PASSWORD = 0x90
    private const val TEXT_WEB_PASSWORD = 0xe0
    private const val TEXT_EMAIL = 0x20
    private const val TEXT_WEB_EMAIL = 0xd0
    private const val NUMBER_PASSWORD = 0x10

    /** One-time codes count only with high confidence (`AUTOFILL.md`). */
    const val CODE_CONFIDENCE = 0.8f

    /** Android's autofill hints as the `autocomplete` tokens the core reads. */
    private val HINTS = mapOf(
        "username" to "username",
        "emailaddress" to "email",
        "email" to "email",
        "password" to "current-password",
        "current-password" to "current-password",
        "newpassword" to "new-password",
        "new-password" to "new-password",
        "smsotpcode" to "one-time-code",
        "2faappotpcode" to "one-time-code",
        "one-time-code" to "one-time-code",
    )

    /** The fields as the core's descriptions (`classify_fields`), in the same order. */
    fun <Id> describe(fields: List<ScreenField<Id>>): String = JsonArray(
        fields.map { f ->
            val attrs = f.htmlAttributes.mapKeys { it.key.lowercase() }
            val inputClass = f.inputType and CLASS_MASK
            val variation = f.inputType and VARIATION_MASK
            val password = (inputClass == CLASS_TEXT && variation in setOf(TEXT_PASSWORD, TEXT_VISIBLE_PASSWORD, TEXT_WEB_PASSWORD)) ||
                (inputClass == CLASS_NUMBER && variation == NUMBER_PASSWORD)
            val email = inputClass == CLASS_TEXT && variation in setOf(TEXT_EMAIL, TEXT_WEB_EMAIL)
            val type = attrs["type"] ?: when {
                password -> "password"
                email -> "email"
                inputClass == CLASS_NUMBER -> "number"
                else -> "text"
            }
            val autocomplete = attrs["autocomplete"]
                ?: f.autofillHints.mapNotNull { HINTS[it.lowercase()] }.joinToString(" ")
            JsonObject(
                mapOf(
                    "tag" to JsonPrimitive(f.htmlTag?.lowercase() ?: "input"),
                    "type" to JsonPrimitive(type),
                    "name" to JsonPrimitive(attrs["name"] ?: f.resourceId ?: ""),
                    "id" to JsonPrimitive(attrs["id"] ?: f.resourceId ?: ""),
                    "autocomplete" to JsonPrimitive(autocomplete),
                    "label" to JsonPrimitive(attrs["aria-label"] ?: f.hint ?: ""),
                    "placeholder" to JsonPrimitive(attrs["placeholder"] ?: f.hint ?: ""),
                    "inputmode" to JsonPrimitive(attrs["inputmode"] ?: if (inputClass == CLASS_NUMBER) "numeric" else ""),
                    "visible" to JsonPrimitive(f.visible),
                    "form" to JsonPrimitive(0),
                ),
            )
        },
    ).toString()

    /** The roles from the core's `[{class, confidence}]`; the first field of each wins. */
    fun <Id> roles(fields: List<ScreenField<Id>>, classesJson: String): FieldRoles<Id> {
        val classes = (Json.parseToJsonElement(classesJson) as? JsonArray).orEmpty().map { it.jsonObject }
        var username: Id? = null
        var password: Id? = null
        var newPassword: Id? = null
        var code: Id? = null
        fields.zip(classes).forEach { (field, c) ->
            val name = c["class"]?.jsonPrimitive?.content
            val confidence = c["confidence"]?.jsonPrimitive?.floatOrNull ?: 0f
            when (name) {
                "username", "email" -> if (username == null) username = field.id
                "current-password" -> if (password == null) password = field.id
                "new-password" -> if (newPassword == null) newPassword = field.id
                "one-time-code" -> if (code == null && confidence >= CODE_CONFIDENCE) code = field.id
            }
        }
        return FieldRoles(username, password, newPassword, code)
    }
}

/** A login the caller may get, as the core ranks it. */
class AutofillMatch(
    val vaultId: String,
    val itemId: String,
    val title: String,
    val username: String,
    /** The core's decision: `fill`, `warn_*`, `confirm` or `refuse_*`. */
    val decision: String,
) {
    /** Offered without asking: not refused, and not only from search. */
    val suggested: Boolean get() = decision == "fill" || decision.startsWith("warn_")

    override fun toString(): String = "AutofillMatch(<redacted>)"
}

private fun JsonArray?.orEmpty(): List<kotlinx.serialization.json.JsonElement> = this ?: emptyList()
