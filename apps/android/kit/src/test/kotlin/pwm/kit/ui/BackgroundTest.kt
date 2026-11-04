package pwm.kit.ui

import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.Item
import pwm.kit.SyncResult

/** The periodic background sync's rules (D57 §4), with a fake scheduler. */
class BackgroundTest {
    private val strong = "lantern orbit velvet granite 42"

    @Test
    fun scheduledWhileSignedInAndCancelledAtSignOut() = runTest {
        val h = Harness(this)
        h.app.start()
        assertEquals("nobody signed in", emptyList<String>(), h.scheduled)

        h.app.showCreateAccount()
        val create = (h.screen as AppModel.Screen.CreateAccount).model
        create.setEmail("ada@example.com")
        create.setPassword(strong)
        create.setConfirm(strong)
        advanceUntilIdle()
        create.submit()
        advanceUntilIdle()
        assertEquals(listOf("schedule"), h.scheduled)

        val key = (h.screen as AppModel.Screen.SecretKey).model
        key.copy()
        key.done()
        advanceUntilIdle()
        h.home.settings.signOut()
        advanceUntilIdle()
        assertEquals(listOf("schedule", "cancel"), h.scheduled)

        h.app.showSignIn()
        val signIn = (h.screen as AppModel.Screen.SignIn).model
        signIn.setEmail("ada@example.com")
        signIn.setPassword(FakeAccount.PASSWORD)
        signIn.setSecretKey(FakeAccount.KEY)
        signIn.submit()
        advanceUntilIdle()
        assertEquals(listOf("schedule", "cancel", "schedule"), h.scheduled)
    }

    @Test
    fun aLaunchWithAnAccountSchedulesIt() = runTest {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        assertEquals(listOf("schedule"), h.scheduled)
    }

    @Test
    fun aRunWhileLockedSyncsAndANewItemShowsOnceUnlocked() = runTest {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val account = h.accounts.single()
        account.backgroundResult = SyncResult.Synced(pulled = 1, pushed = 0, conflicts = 0, failed = 0)
        assertEquals(BackgroundResult.NEW_DATA, h.app.backgroundSync())
        assertEquals(1, account.backgroundSyncs)
        assertTrue("still locked", h.screen is AppModel.Screen.Unlock)

        // Unlocked: the vault reloads what the run stored.
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        account.add(Item.new(Item.Kind.NOTE, "From the other phone", "n1", 1))
        assertEquals(BackgroundResult.NEW_DATA, h.app.backgroundSync())
        assertEquals(listOf("From the other phone"), h.vault.state.value.visible.map { it.item.title })

        account.backgroundResult = SyncResult.Synced(0, 0, 0, 0)
        assertEquals(BackgroundResult.NO_DATA, h.app.backgroundSync())
        account.backgroundResult = SyncResult.Retry(1_000)
        assertEquals(BackgroundResult.FAILED, h.app.backgroundSync())
    }

    @Test
    fun aRevokedDeviceIsSignedOutAndNobodySignedInDoesNothing() = runTest {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        h.accounts.single().backgroundResult = SyncResult.DeviceRevoked
        assertEquals(BackgroundResult.NO_DATA, h.app.backgroundSync())
        assertEquals(AppModel.Screen.Welcome, h.screen)
        assertEquals(listOf("schedule", "cancel"), h.scheduled)
        assertEquals(BackgroundResult.NO_DATA, h.app.backgroundSync())
        assertEquals("not run again once signed out", 1, h.accounts.single().backgroundSyncs)
    }
}
