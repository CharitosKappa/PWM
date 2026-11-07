package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.BillingStatus
import pwm.kit.PlayProducts
import pwm.kit.PwmError
import pwm.kit.Redacted

/** The Plan section (D6 §12): buying through a fake Play, handing purchases to the server, and plans bought elsewhere. */
class PlanTest {
    private fun TestScope.unlocked(setUp: (Harness, FakeAccount) -> Unit = { _, _ -> }): Harness {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true).also { account -> setUp(h, account) } }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        return h
    }

    private val Harness.plan get() = home.settings.plan
    private val Harness.account get() = accounts.single()

    private fun subscription(provider: String, also: List<String> = emptyList(), canceling: Boolean = false) =
        BillingStatus.Subscription(provider, "premium", "active", 2_000_000_000_000, canceling, null, also)

    @Test
    fun aFreePlanBuysPremiumOnPlayAndTheServerGetsThePurchase() = runTest {
        val h = unlocked()
        assertEquals("nothing asked of the server before Settings shows", 0, h.account.billingCalls)
        h.plan.load()
        advanceUntilIdle()
        val before = h.plan.state.value
        assertEquals("free", before.plan)
        assertTrue(before.canBuy)
        assertNull(before.managedBy)

        h.plan.buy()
        advanceUntilIdle()
        assertEquals("linked by the billing token", listOf("${PlayProducts.PREMIUM} ${FakeAccount.BILLING_TOKEN}"), h.play.bought)
        assertEquals(listOf("${PlayProducts.PREMIUM} purchase-1"), h.account.playPurchases)
        val after = h.plan.state.value
        assertEquals("premium", after.plan)
        assertEquals("play", after.managedBy)
        assertFalse(after.canBuy)
        assertFalse(after.busy)
        assertEquals("the token is kept from the status", Redacted(FakeAccount.BILLING_TOKEN), after.status?.token)
        assertEquals(
            "https://play.google.com/store/account/subscriptions?sku=premium_yearly&package=invalid.pwm.app",
            h.plan.manageUrl,
        )
        assertFalse("a purchase never prints its token", PlayPurchase(listOf("p"), Redacted("secret-token"), PlayPurchase.State.PURCHASED, null, false).toString().contains("secret"))
        assertFalse(after.status.toString().contains(FakeAccount.BILLING_TOKEN))
    }

    @Test
    fun closingPlaysSheetChangesNothingAndNoPlayShowsWhy() = runTest {
        val h = unlocked()
        h.plan.load()
        advanceUntilIdle()
        h.play.next = { _, _ -> PlayResult.Canceled }
        h.plan.buy()
        advanceUntilIdle()
        assertTrue(h.account.playPurchases.isEmpty())
        assertNull("no message, no nag", h.plan.state.value.error)
        assertTrue(h.plan.state.value.canBuy)

        h.play.next = { _, _ -> PlayResult.Unavailable }
        h.plan.buy()
        advanceUntilIdle()
        assertEquals("Google Play isn't available on this phone.", h.plan.state.value.error)
    }

    @Test
    fun aPendingPaymentWaitsAndGoesToTheServerWhenItClears() = runTest {
        val h = unlocked { harness, _ -> harness.play.live = true }
        h.plan.load()
        advanceUntilIdle()
        h.play.next = { product, account ->
            PlayResult.Purchases(listOf(FakePlayBilling.purchase(product, "purchase-2", account.value, PlayPurchase.State.PENDING)))
        }
        h.plan.buy()
        advanceUntilIdle()
        assertTrue(h.plan.state.value.pending)
        assertFalse("no second purchase while one waits", h.plan.state.value.canBuy)
        assertTrue("nothing handed over yet", h.account.playPurchases.isEmpty())

        // Play reports it paid, through the listener.
        h.play.later.emit(PlayResult.Purchases(listOf(FakePlayBilling.purchase(token = "purchase-2"))))
        advanceUntilIdle()
        assertEquals(listOf("${PlayProducts.PREMIUM} purchase-2"), h.account.playPurchases)
        assertFalse(h.plan.state.value.pending)
        assertEquals("premium", h.plan.state.value.plan)

        // Locked, Play's answers wait for the next unlock.
        h.home.settings.lockNow()
        advanceUntilIdle()
        h.play.later.emit(PlayResult.Purchases(listOf(FakePlayBilling.purchase(token = "purchase-3"))))
        advanceUntilIdle()
        assertEquals(1, h.account.playPurchases.size)
    }

    @Test
    fun aPurchaseFinishedWhileClosedIsHandedOverAtUnlock() = runTest {
        val h = unlocked { harness, _ ->
            harness.play.owned += FakePlayBilling.purchase(token = "mine")
            harness.play.owned += FakePlayBilling.purchase(token = "another-account", account = "someone-else")
            harness.play.owned += FakePlayBilling.purchase(product = "other_product", token = "not-ours")
        }
        assertEquals(listOf("${PlayProducts.PREMIUM} mine"), h.account.playPurchases)
        assertEquals("premium", h.plan.state.value.plan)

        // Nothing waiting at Play: the server isn't asked at unlock.
        val quiet = unlocked { harness, _ -> harness.play.owned += FakePlayBilling.purchase(token = "done", acknowledged = true) }
        assertEquals(0, quiet.account.billingCalls)
        assertTrue(quiet.account.playPurchases.isEmpty())
    }

    @Test
    fun theServersRefusalsShowPlainly() = runTest {
        val h = unlocked()
        h.plan.load()
        advanceUntilIdle()
        h.account.playPurchaseError = PwmError("purchase_not_yours")
        h.plan.buy()
        advanceUntilIdle()
        assertEquals("This purchase belongs to another account.", h.plan.state.value.error)

        h.account.playPurchaseError = PwmError("purchase_pending")
        h.plan.buy()
        advanceUntilIdle()
        assertTrue(h.plan.state.value.pending)

        val down = unlocked { _, account -> account.billingError = PwmError("billing_unavailable") }
        down.plan.load()
        advanceUntilIdle()
        assertNull(down.plan.state.value.status)
        assertEquals("Plans can't be changed right now. Try again later.", down.plan.state.value.error)
        down.account.billingError = null
        down.plan.load()
        advanceUntilIdle()
        assertEquals("free", down.plan.state.value.plan)
        assertNull(down.plan.state.value.error)
    }

    @Test
    fun aPlanBoughtElsewhereSaysWhereAndOffersNoPurchase() = runTest {
        val h = unlocked { _, account ->
            account.billing = BillingStatus("premium", subscription("stripe"), listOf("stripe", "play"), Redacted(FakeAccount.BILLING_TOKEN))
        }
        h.plan.load()
        advanceUntilIdle()
        assertEquals("stripe", h.plan.state.value.managedBy)
        assertFalse(h.plan.state.value.canBuy)
        assertTrue(h.plan.state.value.alsoPaying.isEmpty())

        val twice = unlocked { _, account ->
            account.billing = BillingStatus("premium", subscription("app_store", also = listOf("play")), listOf("app_store", "play"), Redacted("t"))
        }
        twice.plan.load()
        advanceUntilIdle()
        assertEquals("app_store", twice.plan.state.value.managedBy)
        assertEquals("the second subscription and where to cancel it", listOf("play"), twice.plan.state.value.alsoPaying)
        assertFalse(twice.plan.state.value.canBuy)
    }

    @Test
    fun aServerWithoutBillingIncludesEverythingAndOneNotSellingOnPlaySellsNothingHere() = runTest {
        val h = unlocked { _, account -> account.billing = BillingStatus("premium", null, emptyList(), Redacted("t")) }
        h.plan.load()
        advanceUntilIdle()
        assertTrue(h.plan.state.value.includedWithServer)
        assertFalse(h.plan.state.value.canBuy)

        val web = unlocked { _, account -> account.billing = BillingStatus("free", null, listOf("stripe"), Redacted("t")) }
        web.plan.load()
        advanceUntilIdle()
        assertFalse("Android buys only through Play (D6 §2)", web.plan.state.value.canBuy)
        web.plan.buy()
        advanceUntilIdle()
        assertTrue(web.play.bought.isEmpty())
    }
}
