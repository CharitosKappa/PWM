package pwm.app

import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.util.Base64
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.uiautomator.By
import androidx.test.uiautomator.BySelector
import androidx.test.uiautomator.StaleObjectException
import androidx.test.uiautomator.UiDevice
import androidx.test.uiautomator.Until
import java.security.KeyFactory
import java.security.MessageDigest
import java.security.Signature
import java.security.spec.X509EncodedKeySpec
import java.util.UUID
import org.json.JSONObject
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import pwm.kit.ui.AppModel

/**
 * The Credential Manager provider on a device (D61 §10): turned on with
 * `settings`, the test app asks the system for a passkey and a password and
 * picks PWM in the system's sheet. The passkey's responses are checked here
 * with the platform's own P-256. Needs Android 14+ and a server.
 */
@RunWith(AndroidJUnit4::class)
class CredentialUITest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val device = UiDevice.getInstance(instrumentation)
    private val app get() = instrumentation.targetContext.applicationContext as PwmApplication
    private val model get() = app.model
    private val password = "harbor-${UUID.randomUUID().toString().take(8)}-cobalt-meadow-42"
    private val testPackage get() = instrumentation.context.packageName
    private val appPackage get() = instrumentation.targetContext.packageName
    private val service get() = "$appPackage/pwm.app.PwmCredentialService"

    @Before
    fun signedInWithTheProviderOn() {
        val server = InstrumentationRegistry.getArguments().getString("server").orEmpty()
        assumeTrue("needs a server: -Pandroid.testInstrumentationRunnerArguments.server=…", server.isNotEmpty())
        assumeTrue("the Credential Manager is Android 14+", Build.VERSION.SDK_INT >= 34)
        shell("settings put secure credential_service $service")
        shell("settings put secure credential_service_primary $service")
        // The site's file for the test app, as example.com would serve it (debug builds only).
        val fingerprint = certificate().chunked(2).joinToString(":").uppercase()
        val links = """[{"relation":["delegate_permission/common.get_login_creds"],"target":{"namespace":"android_app","package_name":"$testPackage","sha256_cert_fingerprints":["$fingerprint"]}}]"""
        testPrefs().edit().putString(Credentials.TEST_ASSET_LINKS, links).commit()

        signOut()
        model.setUsesOtherServer(true)
        model.setOtherServer(server)
        model.showCreateAccount()
        val create = (model.state.value.screen as AppModel.Screen.CreateAccount).model
        create.setEmail("passkey-${UUID.randomUUID()}@example.com")
        create.setPassword(password)
        create.setConfirm(password)
        waitFor("the strength meter") { create.state.value.canSubmit }
        create.submit()
        waitFor("the Secret Key") { model.state.value.screen is AppModel.Screen.SecretKey }
        val key = (model.state.value.screen as AppModel.Screen.SecretKey).model
        key.copy()
        key.done()
        waitFor("the vault") { model.state.value.screen is AppModel.Screen.Vault }
        // As in the autofill test: WorkManager's components settle first.
        waitFor("the background sync scheduled") {
            androidx.work.WorkManager.getInstance(app).getWorkInfosForUniqueWork(WorkManagerScheduler.NAME).get()
                .any { it.state == androidx.work.WorkInfo.State.ENQUEUED || it.state == androidx.work.WorkInfo.State.RUNNING }
        }
        Thread.sleep(3_000)
    }

    @After
    fun tidyUp() {
        shell("settings delete secure credential_service")
        shell("settings delete secure credential_service_primary")
        testPrefs().edit().clear().commit()
        repeat(3) { device.pressBack() }
        device.pressHome()
        signOut()
    }

    @Test
    fun anAppMakesAPasskeyAndSignsInWithItThenSavesAndUsesAPassword() {
        // A new passkey: the sheet, then PWM asks to confirm with the master password.
        val create = """{"challenge":"${b64("challenge-create-0001".toByteArray())}","rp":{"id":"example.com","name":"Example"},"user":{"id":"${b64("ada".toByteArray())}","name":"ada","displayName":"Ada"},"pubKeyCredParams":[{"type":"public-key","alg":-7}],"authenticatorSelection":{"residentKey":"required","userVerification":"required"}}"""
        ask("createPasskey", create, user = "ada")
        confirmInSheet()
        typeMasterPassword()
        val registration = JSONObject(result())
        val response = registration.getJSONObject("response")
        val createdData = clientData(response)
        assertEquals("webauthn.create", createdData.getString("type"))
        assertEquals("android:apk-key-hash:${b64(hex(certificate()))}", createdData.getString("origin"))
        assertEquals(testPackage, createdData.getString("androidPackageName"))
        val publicKey = KeyFactory.getInstance("EC").generatePublic(X509EncodedKeySpec(unb64(response.getString("publicKey"))))

        // Signing in, within a minute of confirming: no second prompt.
        val get = """{"challenge":"${b64("challenge-get-0001".toByteArray())}","rpId":"example.com","userVerification":"required"}"""
        ask("getPasskey", get)
        confirmInSheet("ada")
        val assertion = JSONObject(result())
        assertEquals(registration.getString("id"), assertion.getString("id"))
        val signed = assertion.getJSONObject("response")
        val data = unb64(signed.getString("clientDataJSON"))
        val message = unb64(signed.getString("authenticatorData")) + MessageDigest.getInstance("SHA-256").digest(data)
        val verifier = Signature.getInstance("SHA256withECDSA").apply { initVerify(publicKey); update(message) }
        assertTrue("the assertion verifies", verifier.verify(unb64(signed.getString("signature"))))

        // A password the app asks to save: "Save in PWM", then used.
        ask("createPassword", user = "bob", secret = "app-pass-42")
        confirmInSheet()
        click(By.res("saveLogin"), 30_000)
        assertEquals("saved", result())
        ask("getPassword")
        confirmInSheet("bob")
        assertEquals("bob|app-pass-42", result())
    }

    private fun testPrefs() = app.getSharedPreferences(PwmAutofillService.TEST_PREFS, android.content.Context.MODE_PRIVATE)

    /** The test app's signing certificate's SHA-256, as hex. */
    private fun certificate(): String {
        val info = instrumentation.context.packageManager.getPackageInfo(testPackage, PackageManager.GET_SIGNING_CERTIFICATES)
        val signer = info.signingInfo!!.apkContentsSigners.first()
        return MessageDigest.getInstance("SHA-256").digest(signer.toByteArray()).joinToString("") { "%02x".format(it) }
    }

    /** Opens the test app's screen, which asks the Credential Manager. */
    private fun ask(action: String, json: String? = null, user: String? = null, secret: String? = null) {
        val intent = Intent(Intent.ACTION_MAIN).setClassName(testPackage, CredentialTestActivity::class.java.name)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            .putExtra(CredentialTestActivity.ACTION, action)
            .putExtra(CredentialTestActivity.JSON, json)
            .putExtra(CredentialTestActivity.USER, user)
            .putExtra(CredentialTestActivity.PASSWORD, secret)
        instrumentation.context.startActivity(intent)
        device.waitForIdle()
    }

    /** The system's sheet: PWM's entry, or its one button when PWM is the only choice. */
    private fun confirmInSheet(vararg entries: String) {
        val until = System.currentTimeMillis() + 30_000
        while (System.currentTimeMillis() < until) {
            for (text in entries.toList() + listOf("Create", "Save", "Continue", "Sign in", "Use passkey")) {
                val found = device.findObject(By.text(text).clickable(true)) ?: device.findObject(By.text(text))
                if (found != null && found.applicationPackage != testPackage && found.applicationPackage != appPackage) {
                    runCatching { found.click() }
                    device.waitForIdle()
                    return
                }
            }
            if (resultText()?.startsWith("ok:") == true || device.hasObject(By.pkg(appPackage))) return
            Thread.sleep(200)
        }
        error("no sheet; showing: ${onScreen()}")
    }

    private fun typeMasterPassword() {
        val field = device.wait(Until.findObject(By.res("password")), 30_000) ?: error("no confirm screen; showing: ${onScreen()}")
        field.click()
        field.text = password
        click(By.res("unlock"))
    }

    private fun resultText(): String? = runCatching { device.findObject(By.desc("result"))?.text }.getOrNull()

    /** The test app's result: the JSON after "ok:", or a failure with the error's type. */
    private fun result(): String {
        val until = System.currentTimeMillis() + 60_000
        while (System.currentTimeMillis() < until) {
            val text = resultText()
            if (text != null && text != "waiting") {
                check(text.startsWith("ok:")) { "the request failed: $text" }
                return text.removePrefix("ok:")
            }
            // The sheet may ask once more, for the one choice it has.
            Thread.sleep(200)
        }
        error("no result; showing: ${onScreen()}")
    }

    private fun clientData(response: JSONObject): JSONObject =
        JSONObject(unb64(response.getString("clientDataJSON")).decodeToString())

    private fun b64(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)

    private fun unb64(text: String): ByteArray = Base64.decode(text, Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)

    private fun hex(text: String): ByteArray = ByteArray(text.length / 2) { text.substring(it * 2, it * 2 + 2).toInt(16).toByte() }

    private fun click(selector: BySelector, timeout: Long = 10_000) {
        repeat(3) { attempt ->
            val found = device.wait(Until.findObject(selector), timeout) ?: error("not found: $selector; showing: ${onScreen()}")
            try {
                found.click()
                return
            } catch (e: StaleObjectException) {
                if (attempt == 2) throw e
                device.waitForIdle()
            }
        }
    }

    private fun signOut() {
        when (val screen = model.state.value.screen) {
            is AppModel.Screen.Unlock -> screen.model.signOut()
            is AppModel.Screen.Vault -> screen.model.settings.signOut()
            else -> model.showWelcome()
        }
        waitFor("welcome") { model.state.value.screen == AppModel.Screen.Welcome }
    }

    /** The texts and descriptions on screen, for a failure's message; never a field's value. */
    private fun onScreen(): String = device.findObjects(By.clickable(true)).joinToString(" | ") {
        runCatching { "${it.applicationPackage}:" + (it.contentDescription ?: it.resourceName ?: it.text ?: "?") }.getOrDefault("?")
    }

    private fun waitFor(what: String, condition: () -> Boolean) {
        val until = System.currentTimeMillis() + 60_000
        while (!condition()) {
            check(System.currentTimeMillis() < until) { "timed out waiting for $what, on ${model.state.value.screen::class.simpleName}" }
            Thread.sleep(100)
        }
    }

    private fun shell(command: String) {
        android.os.ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command)).use { it.readBytes() }
    }
}
