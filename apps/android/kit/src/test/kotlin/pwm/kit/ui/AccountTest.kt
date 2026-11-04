package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.ApprovalEntry
import pwm.kit.SyncResult

/** Settings' account screens (D59 §8): devices, approving a new device, signing in again. */
class AccountTest {
    private fun TestScope.unlocked(prefs: MemoryPrefs = MemoryPrefs()): Harness {
        val h = Harness(this, prefs)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        return h
    }

    @Test
    fun devicesRenameAndRevokeAfterAsking() = runTest {
        val h = unlocked()
        h.home.setTab(HomeModel.Tab.SETTINGS)
        h.home.settings.showDevices()
        advanceUntilIdle()
        val devices = h.home.settings.state.value.devices!!
        assertEquals(listOf("Pixel 8", "iPhone"), devices.state.value.devices.map { it.name })
        val (mine, other) = devices.state.value.devices

        devices.rename(other, "  Work phone ")
        advanceUntilIdle()
        assertEquals("Work phone", devices.state.value.devices[1].name)
        devices.rename(other, "   ")
        advanceUntilIdle()
        assertEquals("a blank name changes nothing", "Work phone", devices.state.value.devices[1].name)

        devices.askRevoke(mine)
        assertNull("this device signs out instead", devices.state.value.revokeAsked)
        devices.askRevoke(devices.state.value.devices[1])
        devices.keepDevice()
        assertEquals(2, devices.state.value.devices.size)
        devices.askRevoke(devices.state.value.devices[1])
        devices.revoke()
        advanceUntilIdle()
        assertEquals(listOf("Pixel 8"), devices.state.value.devices.map { it.name })

        assertTrue(h.app.back())
        assertNull("the page goes with its model", h.home.settings.state.value.devices)
    }

    @Test
    fun approvingShowsTheCodeAndSendsTheKeyOnlyWhenTheCodesMatch() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        account.approvals += ApprovalEntry("r1", "android", 1, "pending", null)
        h.home.settings.showApprovals()
        val approvals = h.home.settings.state.value.approvals!!
        approvals.load()
        assertEquals(1, approvals.state.value.requests.size)

        approvals.approve(approvals.state.value.requests.single())
        advanceUntilIdle()
        assertEquals("r1", approvals.state.value.approving)
        assertNull("no code until the new device reveals its key", approvals.state.value.code)
        approvals.codesMatch()
        advanceUntilIdle()
        assertFalse("nothing sent without a code", "complete r1" in account.calls)

        account.reveal("r1", "482913")
        approvals.load()
        assertEquals("482913", approvals.state.value.code)
        approvals.codesMatch()
        advanceUntilIdle()
        assertTrue("complete r1" in account.calls)
        assertTrue(approvals.state.value.done)
        assertNull(approvals.state.value.approving)
    }

    @Test
    fun aDeniedOrVanishedRequest() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        account.approvals += ApprovalEntry("r1", "ios", 1, "pending", null)
        account.approvals += ApprovalEntry("r2", "ios", 1, "pending", null)
        h.home.settings.showApprovals()
        val approvals = h.home.settings.state.value.approvals!!
        approvals.load()
        approvals.deny("r1")
        advanceUntilIdle()
        assertTrue("deny r1" in account.calls)
        assertEquals(listOf("r2"), approvals.state.value.requests.map { it.id })

        approvals.approve(approvals.state.value.requests.single())
        advanceUntilIdle()
        account.approvals.clear()
        approvals.load()
        assertNull(approvals.state.value.approving)
        assertEquals("That request ended. Ask the new device to try again.", approvals.state.value.error)
    }

    @Test
    fun aRefusedSessionAsksToSignInAgainAndKeepsTheVault() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        account.syncResult = SyncResult.SignedOut
        h.vault.refresh()
        advanceUntilIdle()
        val again = h.home.signInAgain
        assertTrue(again.state.value.needed)
        assertTrue("kept for the next open", h.prefs.signInAgain)
        assertTrue("the vault still shows", h.screen is AppModel.Screen.Vault)

        again.open()
        again.setPassword("wrong")
        again.signIn()
        advanceUntilIdle()
        assertEquals("Wrong email, master password or Secret Key.", again.state.value.error)
        again.setPassword(FakeAccount.PASSWORD)
        again.signIn()
        advanceUntilIdle()
        assertFalse(again.state.value.needed)
        assertFalse(h.prefs.signInAgain)
        assertEquals(1, account.signInAgains)
        assertTrue("synced again", account.syncs > 1)
    }

    @Test
    fun aRefusalSeenInTheBackgroundShowsAtTheNextOpen() = runTest {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        h.accounts.single().backgroundResult = SyncResult.SignedOut
        h.app.backgroundSync()
        assertTrue(h.prefs.signInAgain)
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        assertTrue(h.home.signInAgain.state.value.needed)

        // Signing out forgets it.
        h.home.settings.signOut()
        advanceUntilIdle()
        assertFalse(h.prefs.signInAgain)
    }
}
