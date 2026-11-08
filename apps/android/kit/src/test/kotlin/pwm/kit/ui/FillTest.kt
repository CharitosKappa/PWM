package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.Caller
import pwm.kit.Item

/** The autofill service's plan and PWM's fill screen (D58 §5-7, §9), over the fake account. */
class FillTest {
    private val bankApp = Caller.App("com.example.bank", listOf("f0fd6c5b410f25cb25c3b53346c8972fae30f8ee7411df910480ad6b2d60db83"))
    private val bankSite = Caller.Web("https://bank.example/")

    private fun login(id: String, title: String, url: String) = Item.new(Item.Kind.LOGIN, title, id, 1).edited {
        username = "ada"
        password = "pw-$id"
        setUrls(listOf(Item.Url(url)))
    }

    private fun TestScope.signedIn(unlock: Boolean, vararg items: Item): Harness {
        val h = Harness(this, MemoryPrefs(biometricOffered = false))
        h.nextAccount = { FakeAccount(it, signedIn = true).apply { biometricAvailable = true; items.forEach(::add) } }
        h.app.start()
        if (unlock) {
            val model = (h.screen as AppModel.Screen.Unlock).model
            model.setPassword(FakeAccount.PASSWORD)
            model.submit()
            advanceUntilIdle()
            (h.screen as? AppModel.Screen.Unlock)?.model?.declineOffer()
        }
        return h
    }

    @Test
    fun unlockedTheMatchesAreSuggestedAndFillWithATap() = runTest {
        val h = signedIn(unlock = true, login("1", "Bank", "https://bank.example"), login("2", "Mail", "https://mail.example"))
        val plan = h.app.autofillPlan(bankSite, "[]")!!
        assertTrue(plan.unlocked)
        assertEquals(listOf("Bank"), plan.suggested.map { it.title })
        val values = h.app.fillValues(plan.suggested.single())!!
        assertEquals("ada", values.username)
        assertEquals("pw-1", values.password.value)
        assertNull(values.code)
    }

    @Test
    fun lockedNothingIsSuggestedAndNobodySignedInGetsNoPlan() = runTest {
        val h = signedIn(unlock = false, login("1", "Bank", "https://bank.example"))
        val plan = h.app.autofillPlan(bankSite, "[]")!!
        assertFalse(plan.unlocked)
        assertTrue("no usernames before unlocking (D58 §6)", plan.suggested.isEmpty())

        val nobody = Harness(this)
        nobody.app.start()
        assertNull(nobody.app.autofillPlan(bankSite, "[]"))
    }

    @Test
    fun theFillScreenUnlocksWithoutTheFingerprintOfferThenFills() = runTest {
        val h = signedIn(unlock = false, login("1", "Bank", "https://bank.example"))
        var filled: FillValues? = null
        val fill = h.app.fillModel(bankSite) { filled = it }!!
        advanceUntilIdle()
        val unlock = fill.state.value.unlock!!
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        assertFalse("no offer while filling", unlock.state.value.offer)
        assertNull(fill.state.value.unlock)
        assertEquals(listOf("Bank"), fill.state.value.matches.map { it.title })
        assertTrue("the app is unlocked too", h.screen is AppModel.Screen.Vault)

        fill.pick(fill.state.value.matches.single())
        advanceUntilIdle()
        assertEquals("pw-1", filled!!.password.value)
    }

    @Test
    fun pickingFromSearchForAnAppLinksIt() = runTest {
        val h = signedIn(unlock = true, login("1", "Bank", "https://bank.example"))
        var filled: FillValues? = null
        val fill = h.app.fillModel(bankApp) { filled = it }!!
        advanceUntilIdle()
        assertTrue("not linked yet", fill.state.value.matches.isEmpty())
        assertTrue(fill.canLink)
        fill.setQuery("bank")
        val entry = fill.state.value.results.single()
        fill.pick(entry)
        advanceUntilIdle()
        assertNotNull(filled)
        assertEquals(listOf("1 com.example.bank"), h.accounts.single().links)

        // Next time it's suggested.
        assertEquals(listOf("Bank"), h.app.autofillPlan(bankApp, "[]")!!.suggested.map { it.title })
    }

    @Test
    fun unlinkingIsTheUsersChoiceAndAWebPageNeverLinks() = runTest {
        val h = signedIn(unlock = true, login("1", "Bank", "https://bank.example"))
        val fill = h.app.fillModel(bankApp) {}!!
        advanceUntilIdle()
        fill.setLinkForApp(false)
        fill.setQuery("bank")
        fill.pick(fill.state.value.results.single())
        advanceUntilIdle()
        assertTrue(h.accounts.single().links.isEmpty())

        val web = h.app.fillModel(bankSite) {}!!
        advanceUntilIdle()
        assertFalse(web.canLink)
    }

    @Test
    fun aCodeComesWithTheLoginAndCancelFillsNothing() = runTest {
        val h = signedIn(unlock = true, login("1", "Bank", "https://bank.example").edited { totp = "otpauth://totp/Bank?secret=JBSWY3DPEHPK3PXP" })
        assertEquals("123456", h.app.fillValues(h.app.autofillPlan(bankSite, "[]")!!.suggested.single())!!.code!!.value)
        var done = false
        var filled: FillValues? = FillValues("x", pwm.kit.Redacted("x"), null)
        val fill = h.app.fillModel(bankSite) { filled = it; done = true }!!
        fill.cancel()
        assertTrue(done)
        assertNull(filled)
    }

    @Test
    fun onFreeTheLoginFillsWithoutItsCode() = runTest {
        val h = signedIn(unlock = true, login("1", "Bank", "https://bank.example").edited { totp = "otpauth://totp/Bank?secret=JBSWY3DPEHPK3PXP" })
        h.accounts.single().codeNeedsPlan = true
        val values = h.app.fillValues(h.app.autofillPlan(bankSite, "[]")!!.suggested.single())!!
        assertEquals("pw-1", values.password.value)
        assertNull("no code, and nothing said about it (D73 §1)", values.code)
    }
}
