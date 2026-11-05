package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.PwmError

/** Push on Android (D60 §8): the token, what a push does, and the notice switch. */
class PushTest {
    /** Signed in with push set up, then unlocked. */
    private fun TestScope.unlocked(available: Boolean = true): Harness {
        val h = Harness(this)
        h.push.available = available
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        advanceUntilIdle()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        return h
    }

    @Test
    fun theTokenIsSentOnceAndAgainOnlyWhenItChanges() = runTest {
        val h = unlocked()
        val account = h.accounts.single()
        assertEquals("sent at launch, locked, and not again at unlock", listOf<String?>("fcm-token-1"), account.pushTokens)
        assertEquals("fcm-token-1", h.prefs.pushToken)

        h.app.pushTokenChanged("fcm-token-2")
        advanceUntilIdle()
        assertEquals(listOf<String?>("fcm-token-1", "fcm-token-2"), account.pushTokens)

        // Signing out removes it from the server and forgets it here.
        h.home.settings.signOut()
        advanceUntilIdle()
        assertEquals(null, account.pushTokens.last())
        assertNull(h.prefs.pushToken)
    }

    @Test
    fun aFailedRegistrationIsTriedAgainAndNoFirebaseMeansNoPush() = runTest {
        val h = Harness(this)
        h.push.available = true
        h.nextAccount = { FakeAccount(it, signedIn = true).apply { pushTokenError = PwmError("network") } }
        h.app.start()
        advanceUntilIdle()
        assertNull("not kept as sent", h.prefs.pushToken)
        val account = h.accounts.single()
        account.pushTokenError = null
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        assertEquals("fcm-token-1", h.prefs.pushToken)

        val off = unlocked(available = false)
        assertTrue(off.accounts.single().pushTokens.isEmpty())
        assertFalse("no switch without push", off.home.settings.state.value.pushAvailable)
    }

    @Test
    fun aChangeSyncsAndARequestShowsTheNoticeOnlyWithTheSwitchOn() = runTest {
        val h = unlocked()
        h.app.pushReceived("sync")
        assertEquals("syncNow", h.scheduled.last())

        h.app.pushReceived("approval")
        assertEquals("the switch is off until turned on", 0, h.notices.shown)
        h.home.settings.setApprovalNotices(true)
        assertTrue(h.prefs.approvalNotices)
        h.app.pushReceived("approval")
        assertEquals(1, h.notices.shown)

        // Android took the permission back in system settings.
        h.notices.allowed = false
        h.app.pushReceived("approval")
        assertEquals(1, h.notices.shown)

        // Anything else, or nobody signed in: nothing.
        val before = h.scheduled.size
        h.app.pushReceived("other")
        h.app.pushReceived(null)
        assertEquals(before, h.scheduled.size)
        h.home.settings.signOut()
        advanceUntilIdle()
        h.notices.allowed = true
        h.app.pushReceived("approval")
        h.app.pushReceived("sync")
        assertEquals(1, h.notices.shown)
        assertFalse("syncNow" in h.scheduled.drop(before))
    }

    @Test
    fun theSwitchAsksAndARefusalStaysOff() = runTest {
        val h = unlocked()
        h.notices.allowed = false
        val settings = h.home.settings
        assertTrue(settings.state.value.pushAvailable)
        settings.setApprovalNotices(true)
        assertTrue("the screen asks Android now", settings.state.value.askNoticePermission)
        assertFalse(h.prefs.approvalNotices)

        settings.noticePermissionAnswered(false)
        assertFalse(settings.state.value.approvalNotices)
        assertTrue(settings.state.value.noticesRefused)
        assertFalse(settings.state.value.askNoticePermission)

        settings.setApprovalNotices(true)
        settings.noticePermissionAnswered(true)
        assertTrue(settings.state.value.approvalNotices)
        assertTrue(h.prefs.approvalNotices)
        settings.setApprovalNotices(false)
        assertFalse(h.prefs.approvalNotices)
    }

    @Test
    fun tappingTheNoticeOpensApprovalsOnceUnlocked() = runTest {
        val h = Harness(this)
        h.push.available = true
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        advanceUntilIdle()
        h.app.openApprovals()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        assertEquals(HomeModel.Tab.SETTINGS, h.home.state.value.tab)
        assertEquals(SettingsModel.Page.APPROVALS, h.home.settings.state.value.page)

        // Already open: straight there.
        h.home.back()
        h.home.setTab(HomeModel.Tab.VAULT)
        h.app.openApprovals()
        assertEquals(SettingsModel.Page.APPROVALS, h.home.settings.state.value.page)
    }
}
