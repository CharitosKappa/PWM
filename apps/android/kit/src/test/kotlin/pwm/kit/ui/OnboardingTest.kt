package pwm.kit.ui

import java.net.URL
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import pwm.kit.Account
import pwm.kit.EmergencyKit
import pwm.kit.PwmError
import pwm.kit.Redacted
import pwm.kit.Server
import pwm.kit.SyncResult

/** The onboarding view models (D53 §6), over a fake account and the real core's strength meter. */
class OnboardingTest {
    private val strong = "lantern orbit velvet granite 42"

    private class Copied(val value: String, val clearAfterMs: Long)

    private class Harness(scope: TestScope, val prefs: MemoryPrefs = MemoryPrefs()) {
        val accounts = mutableListOf<FakeAccount>()
        var nextAccount: (URL) -> FakeAccount = { FakeAccount(it) }
        val copied = mutableListOf<Copied>()
        val app = AppModel(
            scope, prefs, "Pixel 8", { value, after -> copied += Copied(value.value, after) },
            EmergencyKit.Paper.A4,
            makeAccount = { server -> nextAccount(server).also { accounts += it } },
            work = StandardTestDispatcher(scope.testScheduler),
        )
        val screen get() = app.state.value.screen
    }

    @Test
    fun createAccountNeedsAnEmailAStrongPasswordAndTheSameAgain() = runTest {
        val h = Harness(this)
        h.app.start()
        h.app.showCreateAccount()
        val model = (h.screen as AppModel.Screen.CreateAccount).model

        model.setEmail("ada@example.com")
        model.setPassword("password1")
        model.setConfirm("password1")
        advanceUntilIdle()
        assertFalse(model.state.value.strongEnough)
        assertFalse(model.state.value.canSubmit)

        model.setPassword(strong)
        advanceUntilIdle()
        assertTrue(model.state.value.strongEnough)
        assertFalse("the second password must match", model.state.value.canSubmit)
        model.setConfirm(strong)
        assertTrue(model.state.value.canSubmit)
        model.setEmail("ada")
        assertFalse(model.state.value.canSubmit)
        // Passwords never print.
        assertFalse(model.state.value.toString().contains(strong))
    }

    @Test
    fun signingUpShowsTheSecretKeyOnceAndTheKitStaysPendingUntilSaved() = runTest {
        val h = Harness(this)
        h.app.start()
        h.app.showCreateAccount()
        val create = (h.screen as AppModel.Screen.CreateAccount).model
        create.setEmail(" ada@example.com ")
        create.setPassword(strong)
        create.setConfirm(strong)
        advanceUntilIdle()
        create.submit()
        advanceUntilIdle()

        assertEquals(listOf("signUp ada@example.com Pixel 8"), h.accounts.single().calls)
        assertEquals("", create.state.value.password.value)
        val key = (h.screen as AppModel.Screen.SecretKey).model
        assertEquals(FakeAccount.KEY, key.secretKey.value)
        assertTrue(h.prefs.kitPending)
        assertNotNull(key.state.value.kit)
        assertFalse(key.state.value.canContinue)
        key.done()
        assertTrue("not before a save, a print or a copy", h.screen is AppModel.Screen.SecretKey)

        key.copy()
        assertEquals(FakeAccount.KEY, h.copied.single().value)
        assertEquals(30_000L, h.copied.single().clearAfterMs)
        key.done()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertFalse(h.prefs.kitPending)
        assertNull("the kit is dropped", key.state.value.kit)
        assertEquals(2, (h.screen as AppModel.Screen.Vault).model.state.value.itemCount)
    }

    @Test
    fun aSavedKitAllowsContinuingAndAFailedSaveSaysSo() = runTest {
        val h = Harness(this)
        h.app.start()
        h.app.showCreateAccount()
        val create = (h.screen as AppModel.Screen.CreateAccount).model
        create.setEmail("ada@example.com")
        create.setPassword(strong)
        create.setConfirm(strong)
        advanceUntilIdle()
        create.submit()
        advanceUntilIdle()
        val key = (h.screen as AppModel.Screen.SecretKey).model

        key.saveFailed()
        assertEquals("The Emergency Kit couldn't be saved. Try again, or print it.", key.state.value.error)
        assertFalse(key.state.value.canContinue)
        key.didSaveKit()
        assertNull(key.state.value.error)
        assertTrue(key.state.value.canContinue)
    }

    @Test
    fun aKitStillPendingAtLaunchIsShownAgainAfterUnlocking() = runTest {
        val h = Harness(this, MemoryPrefs(kitPending = true))
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        assertEquals("ada@example.com", unlock.email)

        unlock.setPassword("wrong")
        unlock.submit()
        advanceUntilIdle()
        assertEquals("Wrong master password.", unlock.state.value.error)
        assertEquals("the field is cleared", "", unlock.state.value.password.value)

        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        val key = (h.screen as AppModel.Screen.SecretKey).model
        assertEquals(FakeAccount.KEY, key.secretKey.value)
        key.didSaveKit()
        key.done()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertFalse(h.prefs.kitPending)
    }

    @Test
    fun withNoKitPendingUnlockGoesToTheVaultAndLockComesBack() = runTest {
        val h = Harness(this)
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        val unlock = (h.screen as AppModel.Screen.Unlock).model
        unlock.setPassword(FakeAccount.PASSWORD)
        unlock.submit()
        advanceUntilIdle()
        val vault = (h.screen as AppModel.Screen.Vault).model
        vault.lock()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Unlock)
        assertFalse(h.accounts.single().unlocked)
    }

    @Test
    fun signingOutGoesBackToWelcomeAndForgetsAPendingKit() = runTest {
        val h = Harness(this, MemoryPrefs(kitPending = true))
        h.nextAccount = { FakeAccount(it, signedIn = true) }
        h.app.start()
        (h.screen as AppModel.Screen.Unlock).model.signOut()
        advanceUntilIdle()
        assertEquals(AppModel.Screen.Welcome, h.screen)
        assertFalse(h.prefs.kitPending)
        assertFalse(h.accounts.single().isSignedIn())
    }

    @Test
    fun signInWithAllThreeSecrets() = runTest {
        val h = Harness(this)
        h.app.start()
        h.app.showSignIn()
        val signIn = (h.screen as AppModel.Screen.SignIn).model
        signIn.setEmail("ada@example.com")
        signIn.setPassword("wrong")
        assertFalse("no Secret Key yet", signIn.state.value.canSubmit)
        signIn.setSecretKey(" ${FakeAccount.KEY} ")
        signIn.submit()
        advanceUntilIdle()
        assertEquals("Wrong email, master password or Secret Key.", signIn.state.value.error)

        signIn.setPassword(FakeAccount.PASSWORD)
        signIn.submit()
        advanceUntilIdle()
        assertEquals(listOf("logIn ada@example.com ${FakeAccount.KEY} Pixel 8"), h.accounts.single().calls.takeLast(1))
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertEquals("", signIn.state.value.password.value)
    }

    @Test
    fun aScannedKitFillsTheEmailSecretKeyAndServerButNeverThePassword() = runTest {
        val h = Harness(this)
        h.app.start()
        h.app.showSignIn()
        val signIn = (h.screen as AppModel.Screen.SignIn).model
        signIn.setPassword("typed")

        assertTrue(
            signIn.scanned("pwm-kit:1?email=ada%40example.com&secret-key=$KIT_KEY&server=https%3A%2F%2Fvault.example.org"),
        )
        assertEquals("ada@example.com", signIn.state.value.email)
        assertEquals(KIT_KEY, signIn.state.value.secretKey.value)
        assertEquals("typed", signIn.state.value.password.value)
        assertTrue(h.app.state.value.usesOtherServer)
        assertEquals("https://vault.example.org", h.app.state.value.otherServer)

        assertFalse(signIn.scanned("https://example.com"))
        assertEquals("That's not an Emergency Kit code.", signIn.state.value.error)
        assertFalse(signIn.scanned("pwm-kit:2?email=ada%40example.com&secret-key=$KIT_KEY"))
        assertEquals(
            "This Emergency Kit is from a newer version of the app. Update the app to scan it.",
            signIn.state.value.error,
        )
        signIn.cameraRefused()
        assertEquals("Allow camera access in Settings to scan your Emergency Kit.", signIn.state.value.error)
    }

    @Test
    fun askingAnotherDeviceShowsTheCodeAndCanBeCancelled() = runTest {
        val h = Harness(this)
        h.nextAccount = { server ->
            FakeAccount(server).apply { approval = { onCode -> onCode("482913"); awaitCancellation() } }
        }
        h.app.start()
        h.app.showSignIn()
        val signIn = (h.screen as AppModel.Screen.SignIn).model
        assertFalse("needs the email first", signIn.state.value.canAsk)
        signIn.setEmail("ada@example.com")
        signIn.startAsking()
        advanceUntilIdle()
        assertTrue(signIn.state.value.asking)
        assertEquals("482913", signIn.state.value.approvalCode)
        assertFalse(signIn.state.value.canAsk)

        signIn.cancelAsking()
        advanceUntilIdle()
        assertFalse(signIn.state.value.asking)
        assertNull(signIn.state.value.approvalCode)
        assertNull(signIn.state.value.error)
        assertTrue(h.accounts.single().approvalCancelled)
    }

    @Test
    fun anApprovalFillsTheSecretKeyAndAnEndedOneSaysSo() = runTest {
        val h = Harness(this)
        var answer: suspend ((suspend (String) -> Unit)) -> Redacted<String> = { Redacted(FakeAccount.KEY) }
        h.nextAccount = { server -> FakeAccount(server).apply { approval = { answer(it) } } }
        h.app.start()
        h.app.showSignIn()
        val signIn = (h.screen as AppModel.Screen.SignIn).model
        signIn.setEmail("ada@example.com")
        signIn.startAsking()
        advanceUntilIdle()
        assertTrue(signIn.state.value.approved)
        assertEquals(FakeAccount.KEY, signIn.state.value.secretKey.value)
        assertFalse(signIn.state.value.asking)

        answer = { throw PwmError("approval_ended") }
        signIn.startAsking()
        advanceUntilIdle()
        assertEquals("The request ended. Ask again, or sign in with your Secret Key.", signIn.state.value.error)
    }

    @Test
    fun theServerChoiceIsCheckedAndRemembered() = runTest {
        val h = Harness(this)
        h.app.start()
        assertFalse(h.app.state.value.usesOtherServer)
        h.app.setUsesOtherServer(true)
        h.app.setOtherServer("http://vault.example.org")
        h.app.showSignIn()
        val signIn = (h.screen as AppModel.Screen.SignIn).model
        signIn.setEmail("ada@example.com")
        signIn.setPassword(FakeAccount.PASSWORD)
        signIn.setSecretKey(FakeAccount.KEY)
        signIn.submit()
        advanceUntilIdle()
        assertEquals("Enter the server's address, starting with https://.", signIn.state.value.error)

        h.app.setOtherServer("http://localhost:8080/")
        signIn.submit()
        advanceUntilIdle()
        assertTrue(h.screen is AppModel.Screen.Vault)
        assertEquals("http://localhost:8080", h.prefs.server)
        assertEquals("http://localhost:8080", h.accounts.last().server.toString())
    }

    @Test
    fun backFromCreateAccountOrSignInGoesToWelcome() = runTest {
        val h = Harness(this)
        h.app.start()
        assertEquals(AppModel.Screen.Welcome, h.screen)
        assertFalse("the system handles back on welcome", h.app.back())
        h.app.showCreateAccount()
        assertTrue(h.app.back())
        assertEquals(AppModel.Screen.Welcome, h.screen)
        h.app.showSignIn()
        assertTrue(h.app.back())
        assertEquals(AppModel.Screen.Welcome, h.screen)
    }

    @Test
    fun serverAddresses() {
        assertEquals("https://vault.example.org", Server.address(" https://vault.example.org/ ").toString())
        assertEquals("http://127.0.0.1:8080", Server.address("http://127.0.0.1:8080").toString())
        assertEquals("http://[::1]:8080", Server.address("http://[::1]:8080").toString())
        for (refused in listOf("http://vault.example.org", "ftp://vault.example.org", "vault.example.org", "https://", "")) {
            assertEquals(refused, "invalid_server", runCatching { Server.address(refused) }.exceptionOrNull()?.let { (it as PwmError).code })
        }
        assertTrue(Server.isHosted(Server.HOSTED))
    }

    @Test
    fun paperFollowsTheRegion() {
        assertEquals(EmergencyKit.Paper.LETTER, EmergencyKit.Paper.of("US"))
        assertEquals(EmergencyKit.Paper.LETTER, EmergencyKit.Paper.of("ca"))
        assertEquals(EmergencyKit.Paper.A4, EmergencyKit.Paper.of("GR"))
        assertEquals(EmergencyKit.Paper.A4, EmergencyKit.Paper.of(null))
    }

    companion object {
        /** A well-formed Secret Key, the same as the Apple tests'. */
        const val KIT_KEY = "A1-014D2P-F2DBSQ-QZXQ5T-K1V58C-GGA"
    }
}

/** An account with no server: the right password is [PASSWORD]. */
class FakeAccount(override val server: URL = Server.HOSTED, signedIn: Boolean = false) : Account {
    private var signedIn = signedIn
    private var email: String? = if (signedIn) "ada@example.com" else null
    var unlocked = false
    val calls = mutableListOf<String>()
    var approval: suspend (suspend (String) -> Unit) -> Redacted<String> = { throw PwmError("approval_ended") }
    var approvalCancelled = false

    override suspend fun signUp(email: String, password: Redacted<String>, deviceName: String): Redacted<String> {
        calls += "signUp $email $deviceName"
        signedIn = true
        unlocked = true
        this.email = email
        return Redacted(KEY)
    }

    override suspend fun logIn(
        email: String,
        password: Redacted<String>,
        secretKey: Redacted<String>,
        deviceName: String,
    ): SyncResult {
        if (password.value != PASSWORD) throw PwmError("invalid_credentials")
        calls += "logIn $email ${secretKey.value} $deviceName"
        signedIn = true
        unlocked = true
        this.email = email
        return SyncResult.Synced(0, 0, 0, 0)
    }

    override suspend fun askForApproval(email: String, onCode: suspend (String) -> Unit): Redacted<String> = try {
        approval(onCode)
    } catch (e: kotlinx.coroutines.CancellationException) {
        approvalCancelled = true
        throw e
    }

    override suspend fun unlock(password: Redacted<String>) {
        if (password.value != PASSWORD) throw PwmError("wrong_password")
        unlocked = true
    }

    override suspend fun lock() {
        unlocked = false
    }

    override fun isSignedIn(): Boolean = signedIn

    override fun email(): String? = email

    override fun secretKey(): Redacted<String>? = if (signedIn) Redacted(KEY) else null

    override suspend fun emergencyKit(paper: EmergencyKit.Paper): Redacted<ByteArray> = Redacted("%PDF-1.4".toByteArray())

    override suspend fun signOut() {
        signedIn = false
        unlocked = false
        email = null
    }

    override suspend fun sync(): SyncResult = SyncResult.Synced(0, 0, 0, 0)

    override suspend fun itemCount(): Int = 2

    companion object {
        const val PASSWORD = "right horse battery staple"
        const val KEY = "A1-FAKE00-SECRET-KEY000-000000-AAA"
    }
}
