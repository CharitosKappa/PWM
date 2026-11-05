package pwm.kit

import java.net.URL
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.delay
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withContext
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assume.assumeTrue
import org.junit.Before
import org.junit.Test

/**
 * Against a real sync server (D52 §8): `PWM_TEST_SERVER=http://127.0.0.1:8080`.
 * Skipped when it is not set. One sign-up per run, as the server limits them.
 */
class ServerTest {
    private lateinit var server: URL

    @Before
    fun setUp() {
        val address = System.getenv("PWM_TEST_SERVER").orEmpty()
        assumeTrue("PWM_TEST_SERVER is not set", address.isNotEmpty())
        server = URL(address)
    }

    @Test
    fun signUpSaveSyncAndSignInOnASecondDevice() = runTest {
        val email = "kotlin-${UUID.randomUUID()}@example.com"
        val password = Redacted("correct horse battery staple 42")
        var clock = System.currentTimeMillis()
        val firstKeys = MemoryKeyStore()
        val fingers = FakeBiometrics()
        val breaches = FakeBreaches()
        val first = Session(
            cachePath = null, server = server, transport = JdkTransport(), keys = firstKeys, now = { clock }, biometrics = fingers,
            breachTransport = breaches,
        )
        val secretKey = first.signUp(email, password, "Android phone")
        assertTrue(secretKey.value.startsWith("A1-"))
        assertTrue(first.isUnlocked())
        // The Emergency Kit, drawn by the core for this account (D53 §2).
        assertEquals("%PDF-", String(first.emergencyKit(EmergencyKit.Paper.A4).value, 0, 5, Charsets.ISO_8859_1))
        assertEquals(secretKey, first.secretKey())

        val vault = first.vaultIds().first()
        val item = first.newItem(Item.Kind.LOGIN, "Example").edited {
            username = "ada"
            this.password = "hunter2-synthetic"
            setUrls(listOf(Item.Url("https://example.com")))
            totp = OneTimeCode.normalize("JBSWY3DPEHPK3PXP", "Example")
        }
        val id = first.save(item, vault)
        val pushed = first.sync() as SyncResult.Synced
        assertEquals(1L, pushed.pushed)
        // A one-time code, then trash and restore (D54 §9).
        assertEquals(6, first.currentCode(id, vault)!!.code.value.length)
        first.setTrashed(id, vault, trashed = true)
        assertTrue(first.item(id, vault)!!.trashedAtMs != null)
        first.setTrashed(id, vault, trashed = false)
        assertEquals(null, first.item(id, vault)!!.trashedAtMs)
        assertEquals(0, first.purgeTrash())

        val keys = MemoryKeyStore()
        val second = Session(cachePath = null, server = server, transport = JdkTransport(), keys = keys)
        assertTrue(second.logIn(email, password, secretKey, "Android tablet") is SyncResult.Synced)
        val copy = second.items(vault).first { it.id == id }
        assertEquals("Example", copy.title)
        assertEquals("hunter2-synthetic", copy.password)

        // Locked, the background sync stores a change from the first device
        // and uploads one queued here (D57 §2).
        val queued = second.newItem(Item.Kind.NOTE, "Queued on the second device")
        val queuedId = second.save(queued, vault)
        second.lock()
        val fromFirst = first.save(first.newItem(Item.Kind.NOTE, "Made while the second was locked"), vault)
        assertTrue((first.sync() as SyncResult.Synced).pushed >= 1)
        val locked = second.backgroundSync() as SyncResult.Synced
        assertTrue("pulled while locked", locked.pulled >= 1)
        assertEquals("the queued note went up", 1L, locked.pushed)
        second.unlock(password)
        assertEquals("Made while the second was locked", second.items(vault).first { it.id == fromFirst }.title)
        first.sync()
        assertEquals("Queued on the second device", first.items(vault).first { it.id == queuedId }.title)

        // Health (D59 §5): a reused password, and the leak check against a fake
        // breach service that knows the item's password.
        val twin = first.save(first.newItem(Item.Kind.LOGIN, "Twin").edited { this.password = "hunter2-synthetic" }, vault)
        val health = first.passwordHealth()
        assertTrue("reused", health.reused.any { id in it && twin in it })
        breaches.known["hunter2-synthetic"] = 42
        assertEquals(42L, first.breachedPasswords()[id])
        assertTrue("only the breach service was asked", breaches.hosts.all { it == "api.pwnedpasswords.com" })
        assertTrue("with the padding header", breaches.padded)

        // Export (D59 §6): the master password first, then the file.
        assertTrue(runCatching { first.export(ExportFormat.Json) }.exceptionOrNull() is PwmError)
        assertTrue(runCatching { first.authorizeExport(Redacted("not it")) }.exceptionOrNull() is PwmError)
        first.authorizeExport(password)
        val json = first.export(ExportFormat.Json).value.decodeToString()
        assertTrue("Example" in json && "hunter2-synthetic" in json)

        // Import (D59 §7): a browser's CSV, previewed, then saved and synced.
        val csv = Redacted("name,url,username,password\nImported,https://imported.example,bob,pw-imported-1\n".toByteArray())
        assertEquals("csv", first.guessImport("logins.csv", csv)?.format)
        val columns = first.importColumns(csv)
        assertEquals(1, columns.rows)
        assertEquals(1, first.prepareImport("csv", csv, null, null, columns.suggested).logins)
        assertEquals(1, first.finishImport().logins)
        second.sync()
        assertTrue(second.items(vault).any { it.title == "Imported" && it.username == "bob" })

        // Push (D60 §5): an FCM-shaped token is registered, locked too, and removed.
        val fcm = "dGVzdC1pbnN0YW5jZS1pZA:APA91bH_synthetic-token-for-tests_" + "a".repeat(100)
        second.lock()
        second.setPushToken(fcm)
        assertEquals("invalid_push_token", (runCatching { second.setPushToken("short") }.exceptionOrNull() as PwmError).code)
        second.setPushToken(null)
        second.unlock(password)

        // Locked, then unlocked offline with the stored keys.
        second.lock()
        second.unlock(password)
        assertTrue(second.isUnlocked())

        // Devices (D59 §2): both sessions are listed, and a rename comes back sealed and opened.
        val listed = first.devices()
        assertEquals(2, listed.size)
        val mine = listed.single { it.isCurrent }
        val theirs = listed.single { !it.isCurrent }
        first.renameDevice(theirs.id, "Test tablet")
        assertEquals("Test tablet", second.devices().single { it.isCurrent }.name)
        assertEquals(mine.id, first.devices().single { it.isCurrent }.id)

        // Approving a new device (D46, D59 §3): a third session asks, the first approves.
        withContext(Dispatchers.Default) {
            var shown: String? = null
            val newcomer = Session(cachePath = null, server = server, transport = JdkTransport(), keys = MemoryKeyStore())
            val asked = async { newcomer.askForApproval(email) { code -> shown = code } }
            var request: ApprovalEntry? = null
            repeat(50) {
                if (request == null) request = first.approvalRequests().firstOrNull()
                if (request == null) delay(200)
            }
            first.acceptApproval(request!!.id)
            var code: String? = null
            repeat(50) {
                if (code == null) code = first.approvalRequests().firstOrNull { it.id == request!!.id }?.code
                if (code == null) delay(300)
            }
            assertEquals("the same code on both devices", shown, code)
            first.completeApproval(request!!.id)
            assertEquals("the Secret Key arrived", secretKey, asked.await())
        }

        // Signing in again (D50 §5, D59 §4): the master password only, the same device, the vault kept.
        assertTrue(first.signInAgain(password) is SyncResult.Synced)
        assertEquals(mine.id, first.devices().single { it.isCurrent }.id)
        assertEquals("Example", first.items(vault).first { it.id == id }.title)

        // Fingerprint and face unlock through the core (D55 §7), with a fake key store.
        assertEquals(BiometricState.Off, first.biometricState())
        first.enableBiometricUnlock()
        assertEquals("turning it on asks once", 1, fingers.prompts)
        assertEquals(BiometricState.On, first.biometricState())
        first.lock()
        first.unlockWithBiometrics()
        assertTrue(first.isUnlocked())
        // After 14 days the master password is asked once, and then a finger works again.
        clock += BiometricRules.MASTER_PASSWORD_EVERY_MS
        first.lock()
        assertEquals("master_password_due", (runCatching { first.unlockWithBiometrics() }.exceptionOrNull() as PwmError).code)
        first.unlock(password)
        first.lock()
        first.unlockWithBiometrics()
        assertTrue(first.isUnlocked())
        // A cancelled prompt leaves it on.
        first.lock()
        fingers.prompt = { throw PwmError("biometric_cancelled") }
        assertEquals("biometric_cancelled", (runCatching { first.unlockWithBiometrics() }.exceptionOrNull() as PwmError).code)
        assertEquals(BiometricState.On, first.biometricState())
        first.unlock(password)

        // Another device's blob doesn't open here: it is refused and turned off.
        second.lock()
        for (name in listOf(KeyName.BIOMETRIC_BLOB, KeyName.BIOMETRIC_KEY)) keys.set(name, firstKeys.get(name)!!)
        val stolen = Session(cachePath = null, server = server, transport = JdkTransport(), keys = keys, biometrics = FakeBiometrics())
        assertEquals("biometric_changed", (runCatching { stolen.unlockWithBiometrics() }.exceptionOrNull() as PwmError).code)
        assertTrue(keys.get(KeyName.BIOMETRIC_BLOB) == null)

        // Signing out deletes it.
        first.signOut()
        assertTrue(firstKeys.get(KeyName.BIOMETRIC_BLOB) == null)
        assertTrue(fingers.deleted > 0)
        second.signOut()
        assertTrue(keys.get(KeyName.SECRET_KEY) == null)

        // The same phone, signed out, signs in again to that account as the
        // same device, and a new account signs up there as a new device.
        val device = keys.get(KeyName.DEVICE_ID)
        val again = Session(cachePath = null, server = server, transport = JdkTransport(), keys = keys)
        assertTrue(again.logIn(email, password, secretKey, "Android tablet") is SyncResult.Synced)
        assertEquals(device, keys.get(KeyName.DEVICE_ID))
        again.signOut()
        val other = Session(cachePath = null, server = server, transport = JdkTransport(), keys = keys)
        other.signUp("kotlin-${UUID.randomUUID()}@example.com", password, "Android tablet")
        assertTrue("a new device for the new account", keys.get(KeyName.DEVICE_ID) != device)
        other.signOut()
    }
}

/** The breach service's range API, for passwords the test names (D22 §4). */
private class FakeBreaches : HttpTransport {
    val known = mutableMapOf<String, Int>()
    val hosts = mutableListOf<String>()
    var padded = false

    override fun send(request: HttpRequest): HttpResponse {
        hosts += request.url.host
        if (request.headers.any { it.first.equals("Add-Padding", ignoreCase = true) }) padded = true
        val prefix = request.url.path.substringAfterLast('/').uppercase()
        val body = known.entries.mapNotNull { (password, count) ->
            val hash = java.security.MessageDigest.getInstance("SHA-1").digest(password.toByteArray())
                .joinToString("") { "%02X".format(it) }
            if (hash.startsWith(prefix)) "${hash.removePrefix(prefix)}:$count" else null
        }.joinToString("\r\n")
        return HttpResponse(200, emptyList(), body.toByteArray())
    }
}
