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
import pwm.kit.PwmError

/** Deleting the account from Settings (D74 §7): the plan first when there is one, then the master password. */
class DeleteAccountTest {
    private fun TestScope.unlocked(): Harness {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        h.home.setTab(HomeModel.Tab.SETTINGS)
        return h
    }

    private fun subscription(provider: String, also: List<String> = emptyList()) = BillingStatus(
        "premium",
        BillingStatus.Subscription(provider, "premium", "active", 1_800_000_000_000, false, null, null, also),
        listOf("stripe", "play"),
        null,
    )

    private fun TestScope.page(h: Harness): DeleteAccountModel {
        h.home.settings.showDeleteAccount()
        advanceUntilIdle()
        return h.home.settings.state.value.deleting!!
    }

    @Test
    fun withoutAPlanTheMasterPasswordComesNextAndTheAppSaysWhyItLeft() = runTest {
        val h = unlocked()
        val delete = page(h)
        assertEquals(DeleteAccountModel.Step.INTRO, delete.state.value.step)
        delete.next()
        assertEquals(DeleteAccountModel.Step.PASSWORD, delete.state.value.step)
        assertFalse(delete.state.value.canDelete)

        delete.setPassword("not the password")
        delete.delete()
        advanceUntilIdle()
        assertEquals("Wrong master password.", delete.state.value.error)
        assertEquals("never kept", "", delete.state.value.password.value)
        assertTrue(h.accounts.single().isSignedIn())

        delete.setPassword(FakeAccount.PASSWORD)
        delete.delete()
        advanceUntilIdle()
        assertTrue("deleteAccount" in h.accounts.single().calls)
        assertEquals(AppModel.Screen.Welcome, h.screen)
        assertEquals("This account was deleted.", h.app.state.value.ended)
    }

    @Test
    fun aPlayPlanEndsWithTheUnusedDaysBackAndAnAppleOneIsAppleToCancel() = runTest {
        val h = unlocked()
        h.accounts.single().billing = subscription("play", also = listOf("app_store"))
        val delete = page(h)
        delete.next()
        assertEquals(DeleteAccountModel.Step.PLAN, delete.state.value.step)
        assertTrue(delete.state.value.endsWithRefund)
        assertTrue(delete.state.value.appleBills)
        delete.next()
        assertEquals(DeleteAccountModel.Step.PASSWORD, delete.state.value.step)
    }

    @Test
    fun nothingIsDeletedWhileAPlanCantBeEnded() = runTest {
        val h = unlocked()
        h.accounts.single().deleteError = PwmError("delete_unavailable")
        val delete = page(h)
        delete.next()
        delete.setPassword(FakeAccount.PASSWORD)
        delete.delete()
        advanceUntilIdle()
        assertEquals(PwmError("delete_unavailable").userMessage, delete.state.value.error)
        assertTrue(h.accounts.single().isSignedIn())
        assertTrue(h.screen is AppModel.Screen.Vault)

        // Export first, and Back drops the page.
        h.home.settings.exportFirst()
        assertEquals(SettingsModel.Page.EXPORT, h.home.settings.state.value.page)
        assertTrue(h.app.back())
        assertNull(h.home.settings.state.value.deleting)
    }
}
