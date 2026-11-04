package pwm.kit

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.pwm_ffi.PwmClient

/** Reading the screen and deciding who is asking (D58 §2-3, §9), with the real core's classifier. */
class AutofillTest {
    private val chrome = Callers.BROWSERS.getValue("com.android.chrome")
    private val otherCert = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"

    @Test
    fun aListedBrowserSignedByItsCertificateIsBelieved() {
        assertEquals(Caller.Web("https://bank.example/"), Callers.resolve("com.android.chrome", listOf(chrome), "bank.example", "https"))
        assertEquals("the digest's form doesn't matter", Caller.Web("https://bank.example/"),
            Callers.resolve("com.android.chrome", listOf(Callers.digest(chrome)), "bank.example", null))
        assertEquals(Caller.Web("http://bank.example/"), Callers.resolve("com.android.chrome", listOf(chrome), "bank.example", "http"))
    }

    @Test
    fun anyOtherClaimOfADomainIsTheAppItself() {
        // Another app claiming the bank's domain.
        assertEquals(Caller.App("com.evil", listOf(otherCert)), Callers.resolve("com.evil", listOf(otherCert), "bank.example", "https"))
        // Chrome's name with another signer.
        assertEquals(Caller.App("com.android.chrome", listOf(otherCert)),
            Callers.resolve("com.android.chrome", listOf(otherCert), "bank.example", "https"))
        // A browser with no domain is an app too.
        assertEquals(Caller.App("com.android.chrome", listOf(chrome)), Callers.resolve("com.android.chrome", listOf(chrome), null, null))
        assertEquals("android-app://com.evil", Caller.App("com.evil", emptyList()).pageUrl)
        assertEquals(7, Callers.BROWSERS.size)
        assertTrue(Callers.BROWSERS.values.all { Callers.digest(it).matches(Regex("[0-9a-f]{64}")) })
    }

    @Test
    fun aNativeLoginScreenIsClassifiedByTheCore() {
        val fields = listOf(
            ScreenField("user", autofillHints = listOf("username"), inputType = 0x1, resourceId = "login_user"),
            ScreenField("pass", inputType = 0x81, resourceId = "login_password", hint = "Password"),
            ScreenField("code", autofillHints = listOf("smsOTPCode"), inputType = 0x2),
        )
        val json = FieldMapping.describe(fields)
        val described = Json.parseToJsonElement(json).jsonArray.map { it.jsonObject }
        assertEquals("password", described[1]["type"]!!.jsonPrimitive.content)
        assertEquals("one-time-code", described[2]["autocomplete"]!!.jsonPrimitive.content)
        assertTrue("never a value", described.none { "value" in it })

        val client = PwmClient.inMemory()
        val roles = FieldMapping.roles(fields, client.classifyFields(json, "android-app://com.example.bank"))
        assertEquals("user", roles.username)
        assertEquals("pass", roles.password)
        assertEquals("code", roles.oneTimeCode)
    }

    @Test
    fun aWebFormFromABrowserIsClassifiedFromItsHtml() {
        val fields = listOf(
            ScreenField(1, htmlTag = "INPUT", htmlAttributes = mapOf("type" to "email", "name" to "email", "autocomplete" to "username")),
            ScreenField(2, htmlTag = "input", htmlAttributes = mapOf("type" to "password", "autocomplete" to "current-password")),
            ScreenField(3, htmlTag = "input", htmlAttributes = mapOf("type" to "search", "name" to "q")),
        )
        val roles = FieldMapping.roles(fields, PwmClient.inMemory().classifyFields(FieldMapping.describe(fields), "https://bank.example/"))
        assertEquals(1, roles.username)
        assertEquals(2, roles.password)
        assertNull(roles.oneTimeCode)
    }

    @Test
    fun aCodeFieldCountsOnlyWithHighConfidence() {
        val fields = listOf(ScreenField("a"), ScreenField("b"))
        val roles = FieldMapping.roles(fields, """[{"class":"one-time-code","confidence":0.5},{"class":"one-time-code","confidence":0.9}]""")
        assertEquals("b", roles.oneTimeCode)
        assertTrue(FieldMapping.roles(fields, """[{"class":"ignore","confidence":1}]""").isEmpty)
    }
}
