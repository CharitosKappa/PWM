package pwm.kit.ui

import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.BiometricState
import pwm.kit.PwmError

/** Fingerprint and face unlock on the screens (D55 §3, §7), over the fake account. */
class BiometricTest {
    private fun TestScope.atUnlock(prefs: MemoryPrefs = MemoryPrefs(), setUp: FakeAccount.() -> Unit = {}): Harness {
        val h = Harness(this, prefs)
        h.nextAccount = { FakeAccount(it, signedIn = true).apply { biometricAvailable = true; setUp() } }
        h.app.start()
        return h
    }

    private val Harness.unlock get() = (screen as AppModel.Screen.Unlock).model

    @Test
    fun theFirstPasswordUnlockOffersItOnceAndTurningItOnAsksOnce() = runTest {
        val h = atUnlock()
        h.unlock.setPassword(FakeAccount.PASSWORD)
        h.unlock.submit()
        advanceUntilIdle()
        assertTrue("the offer shows before the vault", h.unlock.state.value.offer)
        h.unlock.acceptOffer()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertEquals(1, h.accounts.single().prompts)
        assertEquals(BiometricState.On, h.accounts.single().biometricState())
        assertTrue(h.prefs.biometricOffered)
    }

    @Test
    fun notNowIsNeverAskedAgain() = runTest {
        val h = atUnlock()
        h.unlock.setPassword(FakeAccount.PASSWORD)
        h.unlock.submit()
        advanceUntilIdle()
        h.unlock.declineOffer()
        assertTrue(h.screen is AppModel.Screen.Vault)
        h.home.settings.lockNow()
        h.unlock.setPassword(FakeAccount.PASSWORD)
        h.unlock.submit()
        advanceUntilIdle()
        assertTrue("no offer the second time", h.screen is AppModel.Screen.Vault)
    }

    @Test
    fun noOfferWhereItIsUnavailable() = runTest {
        val h = atUnlock { biometricAvailable = false }
        assertEquals(BiometricState.Unavailable("biometric_not_enrolled"), h.unlock.state.value.biometric)
        h.unlock.setPassword(FakeAccount.PASSWORD)
        h.unlock.submit()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertEquals(
            "Set up a fingerprint or face unlock in your phone's settings first.",
            PwmError("biometric_not_enrolled").userMessage,
        )
    }

    @Test
    fun whenOnTheScreenTriesTheFingerOnceAndACancelLeavesThePassword() = runTest {
        val h = atUnlock { biometricOn = true; biometricResult = { throw PwmError("biometric_cancelled") } }
        h.unlock.tryBiometricsOnce()
        advanceUntilIdle()
        h.unlock.tryBiometricsOnce()
        advanceUntilIdle()
        assertEquals("only once by itself", 1, h.accounts.single().prompts)
        assertTrue(h.screen is AppModel.Screen.Unlock)
        assertEquals(null, h.unlock.state.value.error)

        h.accounts.single().biometricResult = {}
        h.unlock.unlockWithBiometrics()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
    }

    @Test
    fun aChangedFingerAsksForThePasswordThenTurnsItBackOn() = runTest {
        val h = atUnlock(MemoryPrefs(biometricOffered = true)) {
            biometricOn = true
            biometricResult = { throw PwmError("biometric_changed") }
        }
        h.unlock.tryBiometricsOnce()
        advanceUntilIdle()
        assertEquals(
            "Your fingerprint or face unlock changed. Enter your master password to turn it back on.",
            h.unlock.state.value.error,
        )
        assertEquals(BiometricState.Off, h.unlock.state.value.biometric)
        h.accounts.single().biometricResult = {}
        h.unlock.setPassword(FakeAccount.PASSWORD)
        h.unlock.submit()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertEquals("turned back on, without the offer", BiometricState.On, h.accounts.single().biometricState())
    }

    @Test
    fun theMasterPasswordDueSaysSo() = runTest {
        val h = atUnlock { biometricOn = true; biometricResult = { throw PwmError("master_password_due") } }
        h.unlock.tryBiometricsOnce()
        advanceUntilIdle()
        assertEquals("Enter your master password. It's asked every 14 days so you don't forget it.", h.unlock.state.value.error)
        assertTrue(h.screen is AppModel.Screen.Unlock)
    }

    @Test
    fun theSettingsSwitch() = runTest {
        val h = atUnlock(MemoryPrefs(biometricOffered = true))
        h.unlock.setPassword(FakeAccount.PASSWORD)
        h.unlock.submit()
        advanceUntilIdle()
        val settings = h.home.settings
        assertEquals(BiometricState.Off, settings.state.value.biometric)

        h.accounts.single().biometricResult = { throw PwmError("biometric_cancelled") }
        settings.setBiometric(true)
        advanceUntilIdle()
        assertEquals("a cancel leaves it off, quietly", BiometricState.Off, settings.state.value.biometric)
        assertEquals(null, settings.state.value.error)

        h.accounts.single().biometricResult = {}
        settings.setBiometric(true)
        advanceUntilIdle()
        assertEquals(BiometricState.On, settings.state.value.biometric)
        settings.setBiometric(false)
        advanceUntilIdle()
        assertEquals(BiometricState.Off, settings.state.value.biometric)
        assertFalse(settings.state.value.biometricBusy)
    }
}
