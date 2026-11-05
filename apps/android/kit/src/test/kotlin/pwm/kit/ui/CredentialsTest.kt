package pwm.kit.ui

import java.io.IOException
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.AssetLinks
import pwm.kit.CredentialCallers
import pwm.kit.HttpResponse
import pwm.kit.HttpTransport
import pwm.kit.Item
import pwm.kit.PasskeyCaller
import pwm.kit.PwmError
import pwm.kit.Redacted

/** The Credential Manager provider's rules and screens (D61), over the fake account. */
class CredentialsTest {
    private val chromeCert = "F0:FD:6C:5B:41:0F:25:CB:25:C3:B5:33:46:C8:97:2F:AE:30:F8:EE:74:11:DF:91:04:80:AD:6B:2D:60:DB:83"
    private val appCert = "ab".repeat(32)
    private val chrome = CredentialCallers.resolve("com.android.chrome", chromeCert, listOf(chromeCert), "https://bank.example")
    private val bankApp = CredentialCallers.resolve("com.example.bank", appCert, listOf(appCert), null)
    private val create = """{"challenge":"AAAA","rp":{"id":"bank.example","name":"Bank"},"user":{"id":"AA","name":"ada"},"pubKeyCredParams":[{"type":"public-key","alg":-7}]}"""
    private val get = """{"challenge":"AAAA","rpId":"bank.example"}"""

    private fun browserCaller() = PasskeyCaller(chrome, ByteArray(32) { 1 }, emptyList())
    private fun appCaller(listed: Boolean) =
        PasskeyCaller(bankApp, null, if (listed) listOf("""[{"target":{"package_name":"com.example.bank"}}]""") else emptyList())

    private fun login(id: String, title: String, url: String) = Item.new(Item.Kind.LOGIN, title, id, 1).edited {
        username = "ada"
        password = "pw-$id"
        setUrls(listOf(Item.Url(url)))
    }

    private fun TestScope.signedIn(unlock: Boolean, vararg items: Item): Harness {
        val h = Harness(this, MemoryPrefs(biometricOffered = true))
        h.nextAccount = { FakeAccount(it, signedIn = true).apply { items.forEach(::add) } }
        h.app.start()
        if (unlock) {
            val model = (h.screen as AppModel.Screen.Unlock).model
            model.setPassword(FakeAccount.PASSWORD)
            model.submit()
            advanceUntilIdle()
        }
        return h
    }

    private fun Harness.account() = accounts.single()

    // --- Who is asking (§1) ---

    @Test
    fun aListedBrowserKeepsItsOriginAndEveryoneElseIsAnApp() {
        assertEquals("https://bank.example", chrome.origin)
        assertEquals("bank.example", chrome.label)
        // Chrome's name under another certificate, an app claiming a site, a plain-HTTP origin.
        assertNull(CredentialCallers.resolve("com.android.chrome", appCert, listOf(appCert), "https://bank.example").origin)
        assertNull(CredentialCallers.resolve("com.example.bank", appCert, listOf(appCert), "https://bank.example").origin)
        assertNull(CredentialCallers.resolve("com.android.chrome", chromeCert, listOf(chromeCert), "http://bank.example").origin)
        assertEquals("com.example.bank", bankApp.label)
        assertEquals(appCert, bankApp.cert)
        assertEquals(32, CredentialCallers.bytes(chromeCert).size)
    }

    @Test
    fun theAllowlistNamesD58sBrowsersInGooglesFormat() {
        val apps = Json.parseToJsonElement(CredentialCallers.privilegedAllowlist()).jsonObject["apps"]!!.jsonArray
        assertEquals(7, apps.size)
        val first = apps.first().jsonObject
        assertEquals("android", first["type"]!!.jsonPrimitive.content)
        val info = first["info"]!!.jsonObject
        assertEquals("com.android.chrome", info["package_name"]!!.jsonPrimitive.content)
        val signature = info["signatures"]!!.jsonArray.single().jsonObject
        assertEquals("release", signature["build"]!!.jsonPrimitive.content)
        assertEquals(chromeCert, signature["cert_fingerprint_sha256"]!!.jsonPrimitive.content)
    }

    // --- Digital Asset Links (§4) ---

    private class Sites : HttpTransport {
        val files = mutableMapOf<String, HttpResponse>()
        val asked = mutableListOf<String>()

        override fun send(request: pwm.kit.HttpRequest): HttpResponse {
            asked += request.url.toString()
            return files[request.url.toString()] ?: throw IOException("unreachable")
        }
    }

    private fun ok(text: String) = HttpResponse(200, emptyList(), text.toByteArray())

    @Test
    fun theSitesFileAndItsIncludesAreFetchedAndKeptForAnHour() = runTest {
        val sites = Sites()
        var now = 0L
        val site = "https://bank.example/.well-known/assetlinks.json"
        val other = "https://other.example/.well-known/assetlinks.json"
        sites.files[site] = ok("site")
        sites.files[other] = ok("other")
        val links = AssetLinks(
            sites, now = { now },
            urlFor = { json, _ -> if ("bank.example" in json) site else throw PwmError("invalid_passkey_request") },
            includesOf = { if (it == "site") listOf(other, "https://gone.example/x") else emptyList() },
        )
        assertEquals(listOf("site", "other"), links.forRequest(get, create = false))
        assertEquals(3, sites.asked.size)
        // Kept: nothing is asked again, except the file that failed.
        now += AssetLinks.MAX_AGE_MS - 1
        assertEquals(listOf("site", "other"), links.forRequest(get, create = false))
        assertEquals(listOf(site, other, "https://gone.example/x", "https://gone.example/x"), sites.asked)
        // After an hour, or a lock, they're fetched again.
        now += 1
        links.forRequest(get, create = false)
        assertEquals(7, sites.asked.size)
        links.clear()
        sites.asked.clear()
        links.forRequest(get, create = false)
        assertTrue(site in sites.asked)
        // No RP id, no fetch.
        sites.asked.clear()
        assertTrue(links.forRequest("""{"challenge":"AAAA"}""", create = false).isEmpty())
        assertTrue(sites.asked.isEmpty())
    }

    @Test
    fun anUnreachableRefusedOrLargeFileCountsAsNone() = runTest {
        val site = "https://bank.example/.well-known/assetlinks.json"
        for (response in listOf(null, HttpResponse(404, emptyList(), ByteArray(0)), ok("x".repeat(AssetLinks.MAX_BYTES + 1)))) {
            val sites = Sites()
            if (response != null) sites.files[site] = response
            val links = AssetLinks(sites, urlFor = { _, _ -> site }, includesOf = { emptyList() })
            assertTrue(links.forRequest(get, create = false).isEmpty())
        }
    }

    // --- What is offered (§5) ---

    @Test
    fun theProviderAnswersByWhetherTheVaultIsOpen() = runTest {
        val nobody = Harness(this)
        nobody.app.start()
        assertEquals(CredentialAccess.SIGNED_OUT, nobody.app.credentialAccess())

        val h = signedIn(unlock = false, login("1", "Bank", "https://bank.example"))
        assertEquals(CredentialAccess.LOCKED, h.app.credentialAccess())
        assertTrue("nothing named before unlocking", h.app.passwordOffers(chrome).isEmpty())
        assertTrue(h.app.passkeyOffers(browserCaller(), get).isEmpty())
        // Locked, an app needs a site file at least; the rest is checked after unlocking.
        assertFalse(h.app.passkeyCreatable(appCaller(listed = false), create))
        assertTrue(h.app.passkeyCreatable(appCaller(listed = true), create))
        assertTrue(h.app.passkeyCreatable(browserCaller(), create))
        assertEquals(0, h.account().passkeyChecks)
    }

    @Test
    fun unlockedPasswordsAndPasskeysMatchTheCaller() = runTest {
        val h = signedIn(unlock = true, login("1", "Bank", "https://bank.example"), login("2", "Mail", "https://mail.example"))
        assertEquals(CredentialAccess.UNLOCKED, h.app.credentialAccess())
        assertEquals(listOf("Bank"), h.app.passwordOffers(chrome).map { it.title })
        // An app matches through its own links only (D58 §4).
        assertTrue(h.app.passwordOffers(bankApp).isEmpty())
        // A login with no password (a passkey's) isn't offered as one.
        h.account().add(login("3", "Passkey only", "https://bank.example").edited { password = "" })
        assertEquals(listOf("Bank"), h.app.passwordOffers(chrome).map { it.title })

        h.account().passkeys += FakeAccount.FakePasskey("bank.example", "ada", "1")
        assertEquals(listOf("ada"), h.app.passkeyOffers(browserCaller(), get).map { it.userName })
        assertEquals(1, h.app.passkeyOffers(appCaller(listed = true), get).size)
        assertTrue("an app the site doesn't list", h.app.passkeyOffers(appCaller(listed = false), get).isEmpty())
        // Excluded once the account has one.
        assertFalse(h.app.passkeyCreatable(browserCaller(), create))
    }

    // --- Confirming (§6) ---

    @Test
    fun aPasswordEntryUnlocksIfNeededThenReturnsTheValues() = runTest {
        val h = signedIn(unlock = false, login("1", "Bank", "https://bank.example"))
        var result: CredentialResult? = null
        val model = h.app.credentialModel(CredentialTask.GetPassword("1", FakeAccount.VAULT)) { result = it }!!
        advanceUntilIdle()
        val unlock = model.state.value.unlock!!
        assertFalse(model.state.value.confirming)
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        val password = result as CredentialResult.Password
        assertEquals("ada", password.username)
        assertEquals("pw-1", password.password.value)
        assertTrue("the app is unlocked too", h.screen is AppModel.Screen.Vault)
        assertFalse("never shown in logs", password.toString().contains("pw-1"))
    }

    @Test
    fun everyPasskeyUseIsConfirmedUnlessTheUserJustDid() = runTest {
        val h = signedIn(unlock = true)
        h.account().passkeys += FakeAccount.FakePasskey("bank.example", "ada", "1")
        val offer = h.app.passkeyOffers(browserCaller(), get).single()
        // Unlocked an hour ago: confirm now.
        h.now += 3_600_000
        var result: CredentialResult? = null
        val first = h.app.credentialModel(CredentialTask.GetPasskey(browserCaller(), get, offer)) { result = it }!!
        advanceUntilIdle()
        assertTrue(first.state.value.confirming)
        assertNull(result)
        first.state.value.unlock!!.apply { setPassword(FakeAccount.PASSWORD); submit() }
        advanceUntilIdle()
        assertTrue((result as CredentialResult.Passkey).responseJson.contains("signed"))

        // Within a minute: no second prompt.
        h.now += 30_000L
        result = null
        val second = h.app.credentialModel(CredentialTask.GetPasskey(browserCaller(), get, offer)) { result = it }!!
        advanceUntilIdle()
        assertNull(second.state.value.unlock)
        assertNotNull(result)

        // A minute later: asked again.
        h.now += 60_000
        result = null
        val third = h.app.credentialModel(CredentialTask.GetPasskey(browserCaller(), get, offer)) { result = it }!!
        advanceUntilIdle()
        assertTrue(third.state.value.confirming)
        assertNull(result)
        third.cancel()
        assertNull(result)
    }

    @Test
    fun unlockingFromTheSheetCountsAsConfirmingThePasskeyAfter() = runTest {
        val h = signedIn(unlock = false)
        var unlocked: CredentialResult? = null
        val unlock = h.app.credentialModel(CredentialTask.Unlock) { unlocked = it }!!
        advanceUntilIdle()
        unlock.state.value.unlock!!.apply { setPassword(FakeAccount.PASSWORD); submit() }
        advanceUntilIdle()
        assertEquals(CredentialResult.Unlocked, unlocked)

        var made: CredentialResult? = null
        val model = h.app.credentialModel(CredentialTask.CreatePasskey(appCaller(listed = true), create)) { made = it }!!
        advanceUntilIdle()
        assertNull("no second prompt", model.state.value.unlock)
        assertTrue((made as CredentialResult.Passkey).responseJson.contains("cred-1"))
        assertEquals("bank.example", h.account().passkeys.single().rpId)
    }

    @Test
    fun anExcludedOrUnlinkedPasskeyIsRefusedAfterUnlocking() = runTest {
        val h = signedIn(unlock = true)
        h.account().passkeys += FakeAccount.FakePasskey("bank.example", "ada", "1")
        var result: CredentialResult? = null
        val excluded = h.app.credentialModel(CredentialTask.CreatePasskey(browserCaller(), create)) { result = it }!!
        advanceUntilIdle()
        excluded.state.value.unlock!!.apply { setPassword(FakeAccount.PASSWORD); submit() }
        advanceUntilIdle()
        assertEquals("PWM already has a passkey for this account.", excluded.state.value.error)
        assertNull(result)

        val unlinked = h.app.credentialModel(CredentialTask.CreatePasskey(appCaller(listed = false), create)) { result = it }!!
        advanceUntilIdle()
        assertEquals("This app's website doesn't list it, so PWM can't use a passkey here.", unlinked.state.value.error)
    }

    // --- Saving a password (§7) ---

    @Test
    fun saveInPwmMakesANewLoginForTheSiteOrTheApp() = runTest {
        val h = signedIn(unlock = true)
        var result: CredentialResult? = null
        val site = h.app.credentialModel(CredentialTask.CreatePassword(chrome, null, "ada", Redacted("new-pw"))) { result = it }!!
        advanceUntilIdle()
        val summary = site.state.value.save!!
        assertEquals("bank.example", summary.where)
        assertFalse(summary.updating)
        site.save()
        advanceUntilIdle()
        assertEquals(CredentialResult.Saved, result)
        val saved = h.account().items.values.single()
        assertEquals("bank.example", saved.title)
        assertEquals("new-pw", saved.password)
        assertEquals(listOf("https://bank.example"), saved.urls.map { it.url })

        val app = h.app.credentialModel(CredentialTask.CreatePassword(bankApp, "Bank", "bob", Redacted("app-pw"))) { result = it }!!
        advanceUntilIdle()
        assertEquals("Bank", app.state.value.save!!.where)
        app.save()
        advanceUntilIdle()
        val linked = h.account().items.values.last()
        assertEquals("Bank", linked.title)
        assertEquals(listOf("android-app://com.example.bank?sha256=$appCert"), linked.urls.map { it.url })
    }

    @Test
    fun saveInPwmUpdatesTheLoginWithThatUsername() = runTest {
        val h = signedIn(unlock = false, login("1", "Bank", "https://bank.example"))
        var result: CredentialResult? = null
        val model = h.app.credentialModel(CredentialTask.CreatePassword(chrome, null, "ADA", Redacted("changed"))) { result = it }!!
        advanceUntilIdle()
        model.state.value.unlock!!.apply { setPassword(FakeAccount.PASSWORD); submit() }
        advanceUntilIdle()
        assertTrue(model.state.value.save!!.updating)
        model.save()
        advanceUntilIdle()
        assertEquals(CredentialResult.Saved, result)
        assertEquals(1, h.account().items.size)
        assertEquals("changed", h.account().items.getValue("1").password)
    }

    @Test
    fun lockingForgetsTheConfirmationAndTheSitesFiles() = runTest {
        val h = signedIn(unlock = true)
        h.account().passkeys += FakeAccount.FakePasskey("bank.example", "ada", "1")
        val offer = h.app.passkeyOffers(browserCaller(), get).single()
        val confirm = h.app.credentialModel(CredentialTask.GetPasskey(browserCaller(), get, offer)) { }!!
        advanceUntilIdle()
        confirm.state.value.unlock!!.apply { setPassword(FakeAccount.PASSWORD); submit() }
        advanceUntilIdle()
        h.home.settings.lockNow()
        advanceUntilIdle()
        h.account().unlocked = true
        val model = h.app.credentialModel(CredentialTask.GetPasskey(browserCaller(), get, offer)) { }!!
        advanceUntilIdle()
        assertTrue("asked again after a lock", model.state.value.confirming)
    }
}
