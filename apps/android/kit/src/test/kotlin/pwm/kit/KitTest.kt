package pwm.kit

import java.io.IOException
import java.net.URL
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test
import pwm.kit.design.Tokens

/** The Kotlin layer without a server (D52 §8), over the host build of the core. */
class KitTest {
    private val strong = Redacted("correct horse battery staple 42")

    private fun session(transport: HttpTransport = HttpTransport { throw IOException("offline") }) =
        Session(cachePath = null, server = URL("https://pwm.invalid"), keys = MemoryKeyStore(), transport = transport)

    private suspend fun code(block: suspend () -> Unit): String =
        try {
            block()
            "none"
        } catch (e: PwmError) {
            e.code
        }

    @Test
    fun secretsNeverPrint() {
        val secret = Redacted("hunter2")
        assertEquals("<redacted>", secret.toString())
        assertEquals("<redacted>", "$secret")
        val item = Item.new(Item.Kind.LOGIN, "Bank", "00000000-0000-4000-8000-000000000001", 1)
            .edited { password = "hunter2" }
        assertFalse(item.toString().contains("hunter2"))
        assertFalse(item.toJson().toString().contains("hunter2"))
        assertFalse(HttpRequest("POST", URL("https://pwm.invalid/v1/x"), emptyList(), "hunter2".toByteArray()).toString().contains("hunter2"))
    }

    @Test
    fun errorsCarryCodesAndPlainMessages() {
        assertEquals("Wrong master password.", PwmError("wrong_password").userMessage)
        assertEquals("Something went wrong. Try again.", PwmError("no_such_code").userMessage)
        // From design/messages.json: the shared wording, not the Apple app's names.
        assertEquals("This file is too large to import.", PwmError("import_too_large").userMessage)
        assertEquals("This device refused to store its keys.", PwmError("keychain").userMessage)
        assertEquals("code=locked", PwmError("locked").message)
    }

    @Test
    fun itemsKeepFieldsThisVersionDoesNotKnow() {
        val json = """
            {"id":"00000000-0000-4000-8000-000000000002","v":1,"type":"login","title":"Bank",
             "future":{"a":1},
             "login":{"username":"ada","password":"pw","urls":[{"url":"https://bank.example","match":"host","extra":true}],"later":"x"}}
        """.trimIndent()
        val item = Item.parse(json)
        assertEquals(Item.Kind.LOGIN, item.kind)
        assertEquals("ada", item.username)
        assertEquals(listOf(Item.Url("https://bank.example", "host")), item.urls)
        assertTrue(item.isEditable)

        val edited = item.edited {
            title = "Bank (new)"
            password = "new"
            setUrls(listOf(Item.Url("https://bank.example", "domain")))
        }
        val text = edited.toJson().value
        assertTrue(text.contains("\"future\":{\"a\":1}"))
        assertTrue(text.contains("\"later\":\"x\""))
        assertTrue(text.contains("\"extra\":true"))
        assertEquals("Bank (new)", edited.title)
        assertEquals("new", edited.password)
        assertEquals("ada", edited.username)

        val future = Item.parse("""{"id":"x","v":2,"type":"login","title":"t"}""")
        assertFalse(future.isEditable)
        assertNull(Item.parse("""{"id":"x","v":1,"type":"recipe","title":"t"}""").kind)
        try {
            Item.parse("not json")
            fail("parsed")
        } catch (e: PwmError) {
            assertEquals("invalid_item", e.code)
        }
    }

    @Test
    fun theCoreRefusesWhatItShould() = runTest {
        val session = session()
        assertFalse(session.isSignedIn())
        assertFalse(session.isUnlocked())
        assertEquals("locked", code { session.vaultIds() })
        assertEquals("not_signed_in", code { session.unlock(strong) })
        assertEquals("weak_password", code { session.signUp("ada@example.com", Redacted("password"), "Phone") })
        // A lost connection ends the sign-up with the network code.
        assertEquals("network", code { session.signUp("ada@example.com", strong, "Phone") })
    }

    @Test
    fun signingOutWipesTheDeviceWhenThereIsNoSessionToEnd() = runTest {
        // A revoked device: the core has cleared its cache, so nothing is sent (D62).
        val keys = MemoryKeyStore()
        for (name in listOf(KeyName.SECRET_KEY, KeyName.ACCESS_TOKEN, KeyName.REFRESH_TOKEN, KeyName.EMAIL)) keys.set(name, "x")
        val sent = mutableListOf<HttpRequest>()
        val session = Session(
            cachePath = null, server = URL("https://pwm.invalid"), keys = keys,
            transport = HttpTransport { sent += it; throw IOException("offline") },
        )
        session.signOut()
        assertFalse(session.isSignedIn())
        for (name in listOf(KeyName.ACCESS_TOKEN, KeyName.REFRESH_TOKEN, KeyName.EMAIL)) assertNull(keys.get(name))
        assertTrue(sent.isEmpty())
    }

    @Test
    fun theSystemTransportTakesAShorterTimeout() {
        assertTrue(UrlConnectionTransport().withTimeout(5_000) is UrlConnectionTransport)
        val fake = HttpTransport { throw IOException("offline") }
        assertTrue(fake.withTimeout(5_000) === fake)
    }

    @Test
    fun theTokensAreGenerated() {
        assertEquals(1, Tokens.VERSION)
        assertEquals(0xFF4F3FD6, Tokens.Color.accent.light)
        assertEquals(16.0, Tokens.Space.s4, 0.0)
    }
}
