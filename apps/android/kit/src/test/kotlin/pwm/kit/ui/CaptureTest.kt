package pwm.kit.ui

import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Which screens leave the window out of screenshots and the app switcher (D56 §5). */
class CaptureTest {
    private val strong = "lantern orbit velvet granite 42"

    @Test
    fun theLockedScreensShowAndTheSecretKeyAndVaultDont() = runTest {
        val h = Harness(this)
        h.app.start()
        assertFalse("welcome", h.app.state.value.secureWindow)
        assertFalse(h.app.state.value.hideFromRecents)
        h.app.showSignIn()
        assertFalse("sign in", h.app.state.value.secureWindow)
        h.app.showCreateAccount()
        assertFalse("create account", h.app.state.value.secureWindow)

        val create = (h.screen as AppModel.Screen.CreateAccount).model
        create.setEmail("ada@example.com")
        create.setPassword(strong)
        create.setConfirm(strong)
        advanceUntilIdle()
        create.submit()
        advanceUntilIdle()
        assertTrue("the Secret Key", h.app.state.value.secureWindow)
        assertTrue(h.app.state.value.hideFromRecents)

        val key = (h.screen as AppModel.Screen.SecretKey).model
        key.copy()
        key.done()
        advanceUntilIdle()
        assertTrue("the vault", h.app.state.value.secureWindow)
        assertTrue(h.app.state.value.hideFromRecents)

        h.home.settings.lockNow()
        assertFalse("unlock", h.app.state.value.secureWindow)
        assertFalse(h.app.state.value.hideFromRecents)
    }

    @Test
    fun theSwitchOpensTheVaultToCaptureButNeverTheSwitcherOrTheSecretKey() = runTest {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        val settings = h.home.settings
        assertTrue("on by default", settings.state.value.hideFromCapture)

        settings.setHideFromCapture(false)
        assertFalse(h.prefs.hideFromCapture)
        assertFalse(h.app.state.value.secureWindow)
        assertTrue("the switcher still shows no picture", h.app.state.value.hideFromRecents)

        // Kept for the next unlock.
        settings.lockNow()
        val again = (h.screen as AppModel.Screen.Unlock).model
        again.setPassword(FakeAccount.PASSWORD)
        again.submit()
        advanceUntilIdle()
        assertFalse(h.app.state.value.secureWindow)
        assertFalse(h.home.settings.state.value.hideFromCapture)
    }

    @Test
    fun theSecretKeyIsSecureWhateverTheSwitch() = runTest {
        val h = Harness(this, MemoryPrefs(hideFromCapture = false, kitPending = true))
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.SecretKey)
        assertTrue(h.app.state.value.secureWindow)
    }
}
