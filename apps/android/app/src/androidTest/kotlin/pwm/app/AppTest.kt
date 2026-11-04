package pwm.app

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import pwm.kit.KeyName
import uniffi.pwm_ffi.newId

/** On a device or emulator (D52 §8): the core loads, and the Keystore keeps keys. */
@RunWith(AndroidJUnit4::class)
class AppTest {
    @Test
    fun theCoreLoadsOnTheDevice() {
        assertTrue(newId().isNotEmpty())
    }

    @Test
    fun keysAreSealedByTheKeystoreAndComeBack() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        val keys = KeystoreKeys(context)
        keys.set(KeyName.DEVICE_ID, "device-1")
        assertEquals("device-1", KeystoreKeys(context).get(KeyName.DEVICE_ID))
        // Sealed on disk, not stored as it is.
        val file = context.noBackupFilesDir.resolve("keys/device-id")
        assertEquals(false, String(file.readBytes(), Charsets.ISO_8859_1).contains("device-1"))
        keys.delete(KeyName.DEVICE_ID)
        assertNull(keys.get(KeyName.DEVICE_ID))
    }
}
