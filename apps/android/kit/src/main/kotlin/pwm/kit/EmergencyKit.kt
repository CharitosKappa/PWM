package pwm.kit

import java.net.URI
import java.net.URL
import uniffi.pwm_ffi.passwordStrength
import uniffi.pwm_ffi.scanKit

/** The server an account lives on (D42 §4, D53 §5). */
object Server {
    /** The hosted service; a placeholder address until D1. */
    val HOSTED: URL = URL("https://api.pwm.invalid")

    /** Compared as text: `URL.equals` looks the host up on the network. */
    fun isHosted(server: URL): Boolean = server.toString() == HOSTED.toString()

    /**
     * A self-hosted address the user typed: HTTPS, or plain HTTP to this
     * device only, as in the other apps and on the kit.
     */
    fun address(text: String): URL {
        val trimmed = text.trim().removeSuffix("/")
        val uri = runCatching { URI(trimmed) }.getOrNull() ?: throw PwmError("invalid_server")
        val host = uri.host?.removeSurrounding("[", "]")
        if (host.isNullOrEmpty()) throw PwmError("invalid_server")
        val allowed = when (uri.scheme) {
            "https" -> true
            "http" -> host in LOCAL_HOSTS
            else -> false
        }
        if (!allowed) throw PwmError("invalid_server")
        return runCatching { uri.toURL() }.getOrElse { throw PwmError("invalid_server") }
    }

    private val LOCAL_HOSTS = setOf("localhost", "127.0.0.1", "::1")
}

/** The Emergency Kit (D42 §5): its paper, and what a scanned kit fills in. */
object EmergencyKit {
    enum class Paper {
        A4,
        LETTER,
        ;

        companion object {
            /** The regions that use US Letter, as in the Apple app. */
            private val LETTER_REGIONS = setOf(
                "US", "CA", "MX", "PH", "CL", "CO", "VE", "GT", "CR", "PA", "DO", "SV", "NI", "PR", "BZ",
            )

            /** A4, or Letter for a region that uses it. */
            fun of(region: String?): Paper = if (region?.uppercase() in LETTER_REGIONS) LETTER else A4
        }
    }

    /** What a scanned kit fills in on the sign-in screen: never a password. */
    class Scanned(val email: String, val secretKey: Redacted<String>, val server: URL?) {
        override fun toString(): String = "Scanned(<redacted>)"
    }

    /** A QR code's text: a kit (`pwm-kit:1?…`), or `not_a_kit` / `kit_too_new`. */
    fun scan(text: String): Scanned {
        val kit = core { scanKit(text) }
        val server = kit.server?.let { Server.address(it) }
        return Scanned(kit.email, Redacted(kit.secretKey), server)
    }
}

/** A password's strength, from the core (the same meter as the other apps). */
class PasswordStrength(val score: Int, val crackTime: String, val warning: String?, val suggestions: List<String>) {
    companion object {
        /** Scores a password; `context` (such as the email) counts against it. */
        fun of(password: Redacted<String>, context: List<String> = emptyList()): PasswordStrength {
            val s = passwordStrength(password.value, context)
            return PasswordStrength(s.score.toInt(), s.crackTime, s.warning, s.suggestions)
        }
    }

    override fun toString(): String = "PasswordStrength($score)"
}
