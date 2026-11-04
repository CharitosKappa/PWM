package pwm.app

import android.content.Intent
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.StaleObjectException
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import java.util.UUID
import org.junit.After
import org.junit.Assert.assertFalse
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import pwm.kit.Item
import pwm.kit.ui.AppModel

/**
 * The autofill service on a device (D58 §9): turned on with `settings`, it
 * fills another app's login screen through the dropdown and PWM's fill
 * screen. Needs a server, as the onboarding test does.
 */
@RunWith(AndroidJUnit4::class)
class AutofillUITest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val app get() = instrumentation.targetContext.applicationContext as PwmApplication
    private val model get() = app.model
    private val password = "lantern-${UUID.randomUUID().toString().take(8)}-orbit-velvet-42"

    @Before
    fun signedInWithABankLogin() {
        val server = InstrumentationRegistry.getArguments().getString("server").orEmpty()
        assumeTrue("needs a server: -Pandroid.testInstrumentationRunnerArguments.server=…", server.isNotEmpty())
        shell("settings put secure autofill_service invalid.pwm.app/pwm.app.PwmAutofillService")
        // Keyboards' inline chips can't be read by UiAutomator: the dropdown can.
        testPrefs().edit().putBoolean(PwmAutofillService.DROPDOWN_ONLY, true).commit()

        signOut()
        model.setUsesOtherServer(true)
        model.setOtherServer(server)
        model.showCreateAccount()
        val create = (model.state.value.screen as AppModel.Screen.CreateAccount).model
        create.setEmail("autofill-${UUID.randomUUID()}@example.com")
        create.setPassword(password)
        create.setConfirm(password)
        waitFor("the strength meter") { create.state.value.canSubmit }
        create.submit()
        waitFor("the Secret Key") { model.state.value.screen is AppModel.Screen.SecretKey }
        val key = (model.state.value.screen as AppModel.Screen.SecretKey).model
        key.copy()
        key.done()
        waitFor("the vault") { model.state.value.screen is AppModel.Screen.Vault }

        val vault = (model.state.value.screen as AppModel.Screen.Vault).model.vault
        vault.newItem(Item.Kind.LOGIN)
        waitFor("the editor") { vault.state.value.stack.isNotEmpty() }
        val edit = (vault.state.value.stack.last() as pwm.kit.ui.VaultModel.Route.Edit).model
        edit.edit { it.copy(title = "Bank", username = "ada", password = pwm.kit.Redacted("bank-pass-42")) }
        edit.addUrl()
        edit.edit { s -> s.copy(urls = s.urls.map { it.copy(url = "https://bank.example") }) }
        edit.save()
        waitFor("the login saved") { vault.state.value.visible.any { it.item.title == "Bank" } }
        // Scheduling the background sync lets WorkManager turn its own components
        // on a moment later, and a package change makes Android choose the
        // autofill service again, closing any list on screen. Let that pass first.
        waitFor("the background sync scheduled") {
            androidx.work.WorkManager.getInstance(app).getWorkInfosForUniqueWork(WorkManagerScheduler.NAME).get()
                .any { it.state == androidx.work.WorkInfo.State.ENQUEUED || it.state == androidx.work.WorkInfo.State.RUNNING }
        }
        Thread.sleep(3_000)
    }

    @After
    fun tidyUp() {
        shell("settings delete secure autofill_service")
        testPrefs().edit().clear().commit()
        // The test app's and PWM's fill screens leave the front, so the next test's taps reach PWM.
        repeat(3) { device.pressBack() }
        device.pressHome()
        signOut()
    }



    @Test
    fun anotherAppFillsThroughPwmAndAFakeWebDomainGetsNothing() {
        // An app claiming the bank's web domain isn't a listed browser: no bank login.
        openUntilOffered(FakeWebActivity::class.java)
        assertFalse("a fake web domain got the bank's login", device.hasObject(By.text("Bank")))
        device.pressBack()

        // Another app's login screen, not linked yet: search, pick, and link it.
        tapEntryUntil(LoginTestActivity::class.java, "Search PWM", By.res("fillSearch"))
        val search = device.wait(Until.findObject(By.res("fillSearch")), 10_000) ?: error("no fill screen")
        search.click()
        search.text = "Bank"
        click(By.res("result.Bank"))
        assertFilled()

        // Linked: suggested by name on the next visit.
        pickUntilFilled(By.text("Bank"))

        // Locked: "Unlock PWM to fill", then the master password, then the login.
        (model.state.value.screen as AppModel.Screen.Vault).model.settings.lockNow()
        waitFor("locked") { model.state.value.screen is AppModel.Screen.Unlock }
        tapEntryUntil(LoginTestActivity::class.java, "Unlock PWM to fill", By.res("password"))
        val field = device.wait(Until.findObject(By.res("password")), 10_000) ?: error("no unlock screen")
        field.click()
        field.text = password
        click(By.res("unlock"))
        click(By.res("fill.Bank"), 30_000)
        assertFilled()
        device.pressBack()
    }

    private fun testPrefs() = app.getSharedPreferences(PwmAutofillService.TEST_PREFS, android.content.Context.MODE_PRIVATE)

    /** A screen of the test app: another app, in its own process. */
    private fun open(screen: Class<*>) {

        val intent = Intent(Intent.ACTION_MAIN).setClassName(instrumentation.context.packageName, screen.name)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
        instrumentation.context.startActivity(intent)
        device.waitForIdle()
        check(device.wait(Until.hasObject(By.desc("username")), 10_000)) { "the test screen didn't open" }
    }

    /** Focusing a field asks the autofill service. */
    private fun tapField(name: String) = click(By.desc(name))

    /** Taps what [selector] finds, again if a screen was just replaced under it. */
    private fun click(selector: BySelector, timeout: Long = 10_000) {
        repeat(3) { attempt ->
            val found = device.wait(Until.findObject(selector), timeout) ?: error("not found: $selector")
            try {
                found.click()
                return
            } catch (e: StaleObjectException) {
                if (attempt == 2) throw e
                device.waitForIdle()
            }
        }
    }

    /**
     * Opens a test screen and taps its username field until PWM's entry
     * shows. The first request after the service is chosen can be lost on a
     * slow emulator, so the screen is opened again, at most three times.
     */
    private fun openUntilOffered(screen: Class<*>) {
        repeat(3) { attempt ->
            open(screen)
            tapField("username")
            if (device.wait(Until.hasObject(By.text("Search PWM")), 15_000)) return
            check(attempt < 2) { "no PWM entry, on ${model.state.value.screen::class.simpleName}; showing: ${onScreen()}" }
            device.pressBack()
        }
    }

    /**
     * Opens a test screen, taps its username field and then PWM's [entry],
     * until [next] shows: a list closed by Android under the tap is tried
     * again, at most three times.
     */
    private fun tapEntryUntil(screen: Class<*>, entry: String, next: BySelector) {
        repeat(3) { attempt ->
            open(screen)
            tapField("username")
            if (device.wait(Until.hasObject(By.text(entry)), 15_000)) {
                runCatching { device.findObject(By.text(entry))?.click() }
                if (device.wait(Until.hasObject(next), 10_000)) return
            }
            check(attempt < 2) { "no $entry, on ${model.state.value.screen::class.simpleName}; showing: ${onScreen()}" }
            device.pressBack()
        }
    }

    private fun filled(): Boolean {
        val username = device.findObject(By.desc("username"))?.text
        // A password field reads back masked: its length is enough.
        val password = device.findObject(By.desc("password"))?.text
        return username == "ada" && password?.length == "bank-pass-42".length
    }

    private fun assertFilled() = waitFor("the fields filled", { onScreen() }) { filled() }

    /**
     * Opens the test app's login screen and picks [entry] from PWM's list.
     * On a slow emulator a tap can land while the list is still being laid
     * out and do nothing, so the screen is opened again, at most three times.
     */
    private fun pickUntilFilled(entry: BySelector) {
        repeat(3) { attempt ->
            open(LoginTestActivity::class.java)
            tapField("username")
            click(entry)
            val until = System.currentTimeMillis() + 15_000
            while (System.currentTimeMillis() < until) {
                if (filled()) return
                Thread.sleep(100)
            }
            if (attempt == 2) assertFilled()
        }
    }

    private fun signOut() {
        when (val screen = model.state.value.screen) {
            is AppModel.Screen.Unlock -> screen.model.signOut()
            is AppModel.Screen.Vault -> screen.model.settings.signOut()
            // A set-up that stopped part way.
            else -> model.showWelcome()
        }
        waitFor("welcome") { model.state.value.screen == AppModel.Screen.Welcome }
    }

    /** The texts and descriptions on screen, for a failure's message; never a field's value. */
    private fun onScreen(): String = device.findObjects(By.clickable(true)).joinToString(" | ") {
        runCatching { (it.contentDescription ?: it.resourceName ?: it.text ?: "?").toString() }.getOrDefault("?")
    }

    private fun waitFor(what: String, detail: () -> String = { "" }, condition: () -> Boolean) {
        val until = System.currentTimeMillis() + 60_000
        while (!condition()) {
            check(System.currentTimeMillis() < until) {
                "timed out waiting for $what, on ${model.state.value.screen::class.simpleName}; showing: ${detail()}"
            }
            Thread.sleep(100)
        }
    }

    private fun shell(command: String) {
        shellOutput(command)
    }

    private fun shellOutput(command: String): String =
        android.os.ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command))
            .use { it.readBytes().decodeToString() }
}
