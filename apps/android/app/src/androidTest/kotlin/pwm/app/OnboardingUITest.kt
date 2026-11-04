package pwm.app

import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.UUID
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The onboarding screens on a device (D53 §6). The full flow needs a server,
 * passed as `-Pandroid.testInstrumentationRunnerArguments.server=http://localhost:8080`
 * (CI runs `adb reverse tcp:8080 tcp:8080` first); without one it is skipped.
 */
@RunWith(AndroidJUnit4::class)
class OnboardingUITest {
    @get:Rule
    val compose = createAndroidComposeRule<MainActivity>()

    private val password = "lantern-${UUID.randomUUID().toString().take(8)}-orbit-velvet-42"

    @Before
    fun fromWelcome() {
        // A previous run may have left an account on this device.
        compose.waitUntil(10_000) { exists("welcome") || exists("unlockScreen") }
        if (exists("unlockScreen")) tap("signOut")
        compose.waitUntil(10_000) { exists("welcome") }
    }

    @Test
    fun createAccountChecksThePasswordAndBackReturnsToWelcome() {
        tap("createAccount")
        type("email", "ada@example.com")
        type("password", "password1")
        type("confirm", "password1")
        compose.onNodeWithTag("create").assertIsNotEnabled()
        replace("password", password)
        replace("confirm", password)
        compose.waitUntil(10_000) { enabled("create") }
        tap("back")
        compose.waitUntil(5_000) { exists("welcome") }
    }

    @Test
    fun createAnAccountSignOutThenSignInAgainWithTheSecretKey() {
        val server = InstrumentationRegistry.getArguments().getString("server").orEmpty()
        assumeTrue("needs a server: -Pandroid.testInstrumentationRunnerArguments.server=…", server.isNotEmpty())

        tap("otherServerSwitch")
        replace("otherServer", server)

        // Create an account.
        val email = "android-${UUID.randomUUID()}@example.com"
        tap("createAccount")
        type("email", email)
        type("password", password)
        type("confirm", password)
        compose.waitUntil(10_000) { enabled("create") }
        tap("create")

        // The Secret Key, shown once; continue after copying it.
        compose.waitUntil(60_000) { exists("secretKeyScreen") }
        val secretKey = text("secretKey")
        check(secretKey.startsWith("A1-")) { "not a Secret Key" }
        compose.waitUntil(10_000) { enabled("saveKit") }
        compose.onNodeWithTag("saved").assertIsNotEnabled()
        tap("copy")
        tap("saved")
        compose.waitUntil(60_000) { exists("vaultCount") }

        // Sign out, then sign in again as on a new device.
        tap("signOut")
        compose.waitUntil(10_000) { exists("welcome") }
        tap("signIn")
        type("email", email)
        type("password", password)
        type("secretKey", secretKey)
        tap("submit")
        compose.waitUntil(60_000) { exists("vaultCount") }

        // Lock, then unlock with the master password only.
        tap("lock")
        compose.waitUntil(10_000) { exists("unlockScreen") }
        type("password", password)
        tap("unlock")
        compose.waitUntil(30_000) { exists("vaultCount") }
        tap("signOut")
        compose.waitUntil(10_000) { exists("welcome") }
    }

    private fun exists(tag: String) = compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty()

    private fun enabled(tag: String) =
        compose.onAllNodes(hasTestTag(tag) and SemanticsMatcher.keyNotDefined(SemanticsProperties.Disabled))
            .fetchSemanticsNodes().isNotEmpty()

    private fun tap(tag: String) {
        compose.waitUntil(10_000) { enabled(tag) }
        compose.onNodeWithTag(tag).performScrollTo().assertIsEnabled().performClick()
        compose.waitForIdle()
    }

    private fun type(tag: String, text: String) {
        compose.waitUntil(10_000) { exists(tag) }
        compose.onNodeWithTag(tag).performScrollTo().performTextInput(text)
    }

    private fun replace(tag: String, text: String) {
        compose.waitUntil(10_000) { exists(tag) }
        compose.onNodeWithTag(tag).performScrollTo().performTextReplacement(text)
    }

    private fun text(tag: String): String =
        compose.onNodeWithTag(tag).fetchSemanticsNode().config[SemanticsProperties.Text].joinToString("") { it.text }
}
