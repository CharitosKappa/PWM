package pwm.app

import androidx.compose.ui.semantics.SemanticsActions
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.semantics.getOrNull
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assertIsEnabled
import androidx.compose.ui.test.assertIsNotEnabled
import androidx.compose.ui.test.hasTestTag
import androidx.compose.ui.test.junit4.createAndroidComposeRule
import androidx.compose.ui.test.onNodeWithTag
import androidx.compose.ui.test.performScrollTo
import androidx.compose.ui.test.performSemanticsAction
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.test.performTextReplacement
import android.view.WindowManager
import androidx.lifecycle.Lifecycle
import androidx.work.ListenableWorker
import androidx.work.WorkInfo
import androidx.work.WorkManager
import androidx.work.testing.TestListenableWorkerBuilder
import kotlinx.coroutines.runBlocking
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import java.util.UUID
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

/**
 * Onboarding and the vault on a device (D53 §6, D54 §9). The full flow needs a server,
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
        compose.waitUntil(10_000) { exists("welcome") || exists("unlockScreen") || exists("vaultScreen") }
        if (exists("unlockScreen")) tap("signOut")
        if (exists("vaultScreen")) signOut()
        compose.waitUntil(10_000) { exists("welcome") }
        check(!secure()) { "the welcome screen shows in screenshots (D56 §1)" }
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
    fun createAnAccountUseTheVaultThenSignInAgainWithTheSecretKey() {
        val server = InstrumentationRegistry.getArguments().getString("server").orEmpty()
        assumeTrue("needs a server: -Pandroid.testInstrumentationRunnerArguments.server=…", server.isNotEmpty())

        // Another test may have left it on.
        if (!exists("otherServer")) tap("otherServerSwitch")
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
        check(secure()) { "the Secret Key screen is left out of screenshots" }
        tap("copy")
        tap("saved")
        compose.waitUntil(60_000) { exists("itemList") }
        check(secure()) { "the vault is left out of screenshots" }

        // A login with a generated password (D54 §9).
        tap("newItem")
        tap("newItem.login")
        type("title", "Bank")
        type("username", "ada")
        tap("generate")
        tap("useGenerated")
        compose.waitUntil(10_000) { !exists("useGenerated") }
        tap("save")
        compose.waitUntil(30_000) { exists("item.Bank") }

        // Found by search, edited, its password copied.
        type("search", "ban")
        // Screen readers (design/README.md): the row is one stop, its copies are its actions.
        check(description("item.Bank") == "Bank, ada") { "the row says ${description("item.Bank")}" }
        check(actions("item.Bank") == listOf(string(R.string.copy_password), string(R.string.copy_username))) { "the row's actions" }
        tap("item.Bank")
        tap("edit")
        replace("username", "grace")
        tap("save")
        compose.waitUntil(30_000) { exists("detailScreen") && exists("field.username") && text("field.username") == "grace" }
        // A hidden password says "hidden", never the dots; Show and Copy are the field's actions.
        check(node("field.password").getOrNull(SemanticsProperties.StateDescription) == string(R.string.hidden)) { "the hidden password isn't hidden" }
        check(node("field.password").getOrNull(SemanticsProperties.Text) == null) { "the hidden password is read out" }
        act("field.password", string(R.string.show))
        check(text("field.password").length == 20) { "the generated password wasn't saved" }
        act("field.password", string(R.string.copy))

        // Trashed and restored.
        tap("trash")
        compose.waitUntil(30_000) { exists("emptyList") }
        replace("search", "")
        tap("section.trash")
        tap("item.Bank")
        tap("restore")
        tap("section.all")
        compose.waitUntil(30_000) { exists("item.Bank") }

        // Sign out, then sign in again as on a new device: the login synced.
        signOut()
        tap("signIn")
        type("email", email)
        type("password", password)
        type("secretKey", secretKey)
        tap("submit")
        compose.waitUntil(60_000) { exists("item.Bank") }

        // The background sync is scheduled, and a run succeeds (D57 §4).
        // Scheduling is asynchronous, and the first run may start at once.
        compose.waitUntil(10_000) { syncJob() in setOf(WorkInfo.State.ENQUEUED, WorkInfo.State.RUNNING) }
        val worker = TestListenableWorkerBuilder<SyncWorker>(InstrumentationRegistry.getInstrumentation().targetContext.applicationContext).build()
        check(runBlocking { worker.doWork() } == ListenableWorker.Result.success()) { "the background sync failed" }

        // No finger enrolled on the emulator: Settings says why (D55 §7).
        tap("tab.settings")
        compose.waitUntil(10_000) { exists("biometricUnavailable") }

        // Rename this device: the name goes up sealed and comes back opened (D59 §2).
        tap("devices")
        tap("rename.current")
        replace("deviceName", "Test phone")
        tap("confirmRename")
        compose.waitUntil(30_000) { exists("device.current.name") && text("device.current.name") == "Test phone" }
        tap("back")

        // Health, with the leak check off so the test asks no outside service (D59 §5):
        // one login with a generated password has no problems.
        tap("breachCheckSwitch")
        tap("health")
        compose.waitUntil(30_000) { exists("healthClean") }
        tap("back")
        tap("breachCheckSwitch")

        // Import a CSV the test writes, then delete it (D59 §7). The system's
        // picker is checked by hand (QUALITY.md); its address goes in directly.
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val csv = java.io.File(context.cacheDir, "browser-export.csv")
        csv.writeText("name,url,username,password\nImported,https://imported.example,bob,pw-imported-1\n")
        tap("import")
        compose.runOnUiThread { settings().state.value.importing!!.picked(android.net.Uri.fromFile(csv).toString()) }
        compose.waitUntil(10_000) { exists("importFile") }
        tap("checkImport")
        compose.waitUntil(30_000) { exists("importReady") }
        tap("importNow")
        compose.waitUntil(30_000) { exists("importDone") }
        tap("deleteImport")
        compose.waitUntil(10_000) { exists("importDeleted") }
        check(!csv.exists()) { "the export file is still there" }
        tap("back")

        // Export JSON after the master password and the warning (D59 §6), written
        // where "Save to" would write it.
        tap("export")
        tap("export.JSON")
        compose.onNodeWithTag("exportNow").assertIsNotEnabled()
        type("exportMasterPassword", password)
        tap("exportUnderstood")
        tap("exportNow")
        compose.waitUntil(60_000) { exists("saveExport") }
        val exported = java.io.File(context.cacheDir, "pwm-export.json")
        val export = settings().state.value.export!!
        check(DataFiles.write(context, android.net.Uri.fromFile(exported), export.state.value.file!!.value)) { "the export wasn't written" }
        compose.runOnUiThread { export.didSave() }
        compose.waitUntil(10_000) { exists("exportSaved") }
        val json = exported.readText()
        exported.delete()
        check("Bank" in json && "Imported" in json) { "the export is missing items" }
        tap("back")

        // Push (D60 §8), without a real FCM: a change starts the sync job at once,
        // and with the switch on and the permission given, a request shows the notice.
        val pwm = context.applicationContext as PwmApplication
        compose.runOnUiThread { pwm.model.pushReceived("sync") }
        compose.waitUntil(10_000) {
            WorkManager.getInstance(context).getWorkInfosForUniqueWork(WorkManagerScheduler.NOW).get().isNotEmpty()
        }
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.TIRAMISU) {
            InstrumentationRegistry.getInstrumentation().uiAutomation
                .grantRuntimePermission(context.packageName, android.Manifest.permission.POST_NOTIFICATIONS)
        }
        val prefs = AndroidPrefs(context)
        prefs.approvalNotices = true
        val manager = context.getSystemService(android.app.NotificationManager::class.java)
        compose.runOnUiThread { pwm.model.pushReceived("approval") }
        compose.waitUntil(10_000) { manager.activeNotifications.any { it.id == AndroidNotices.NOTICE_ID } }
        val notice = manager.activeNotifications.single { it.id == AndroidNotices.NOTICE_ID }.notification
        check(notice.channelId == AndroidNotices.CHANNEL) { "the notice isn't on its channel" }
        manager.cancel(AndroidNotices.NOTICE_ID)
        prefs.approvalNotices = false

        // The switch lets the vault show in screenshots and screen sharing (D56 §2).
        tap("hideFromCaptureSwitch")
        compose.waitUntil(5_000) { !secure() }
        tap("hideFromCaptureSwitch")
        compose.waitUntil(5_000) { secure() }

        // Lock now, then unlock with the master password only.
        tap("lockNow")
        unlock()

        // Immediately: leaving the app locks it (D54 §5).
        tap("tab.settings")
        tap("lockTimeout.IMMEDIATELY")
        compose.activityRule.scenario.moveToState(Lifecycle.State.CREATED)
        // The whole app's stop comes a moment after the Activity's.
        Thread.sleep(2_000)
        compose.activityRule.scenario.moveToState(Lifecycle.State.RESUMED)
        unlock()
        signOut()
        compose.waitUntil(10_000) { syncJob() == WorkInfo.State.CANCELLED }
    }

    private fun settings(): pwm.kit.ui.SettingsModel {
        val app = InstrumentationRegistry.getInstrumentation().targetContext.applicationContext as PwmApplication
        return (app.model.state.value.screen as pwm.kit.ui.AppModel.Screen.Vault).model.settings
    }

    private fun syncJob(): WorkInfo.State? =
        WorkManager.getInstance(InstrumentationRegistry.getInstrumentation().targetContext)
            .getWorkInfosForUniqueWork(WorkManagerScheduler.NAME).get().lastOrNull()?.state

    private fun unlock() {
        compose.waitUntil(10_000) { exists("unlockScreen") }
        type("password", password)
        tap("unlock")
        compose.waitUntil(30_000) { exists("itemList") }
    }

    private fun signOut() {
        tap("tab.settings")
        tap("signOut")
        tap("confirmSignOut")
        compose.waitUntil(10_000) { exists("welcome") }
    }

    /** Whether the window is left out of screenshots and screen sharing. */
    private fun secure(): Boolean {
        compose.waitForIdle()
        var flags = 0
        compose.activityRule.scenario.onActivity { flags = it.window.attributes.flags }
        return flags and WindowManager.LayoutParams.FLAG_SECURE != 0
    }

    private fun exists(tag: String) = compose.onAllNodes(hasTestTag(tag)).fetchSemanticsNodes().isNotEmpty()

    private fun enabled(tag: String) =
        compose.onAllNodes(hasTestTag(tag) and SemanticsMatcher.keyNotDefined(SemanticsProperties.Disabled))
            .fetchSemanticsNodes().isNotEmpty()

    private fun tap(tag: String) {
        compose.waitUntil(10_000) { enabled(tag) }
        // Tabs and menus aren't in a scrolling list.
        runCatching { compose.onNodeWithTag(tag).performScrollTo() }
        // The element's own click, not a touch at its place: on a small screen
        // the keyboard or a closing sheet can still cover it.
        compose.onNodeWithTag(tag).assertIsEnabled().performSemanticsAction(SemanticsActions.OnClick)
        compose.waitForIdle()
    }

    private fun type(tag: String, text: String) {
        compose.waitUntil(10_000) { exists(tag) }
        runCatching { compose.onNodeWithTag(tag).performScrollTo() }
        compose.onNodeWithTag(tag).performTextInput(text)
    }

    private fun replace(tag: String, text: String) {
        compose.waitUntil(10_000) { exists(tag) }
        runCatching { compose.onNodeWithTag(tag).performScrollTo() }
        compose.onNodeWithTag(tag).performTextReplacement(text)
    }

    private fun text(tag: String): String =
        compose.onNodeWithTag(tag).fetchSemanticsNode().config[SemanticsProperties.Text].joinToString("") { it.text }

    private fun node(tag: String) = compose.onNodeWithTag(tag).fetchSemanticsNode().config

    private fun string(id: Int) = InstrumentationRegistry.getInstrumentation().targetContext.getString(id)

    /** What TalkBack says for a stop's name. */
    private fun description(tag: String) = node(tag)[SemanticsProperties.ContentDescription].joinToString()

    /** A stop's TalkBack actions, by name. */
    private fun actions(tag: String) = node(tag).getOrNull(SemanticsActions.CustomActions).orEmpty().map { it.label }

    /** Runs one of a stop's TalkBack actions, as a TalkBack user would from its actions menu. */
    private fun act(tag: String, label: String) {
        val action = node(tag)[SemanticsActions.CustomActions].first { it.label == label }
        compose.runOnUiThread { action.action() }
        compose.waitForIdle()
    }
}
