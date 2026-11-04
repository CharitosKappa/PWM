package pwm.kit

import java.net.URL
import java.util.UUID
import kotlinx.coroutines.test.runTest
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
        val first = Session(cachePath = null, server = server, keys = MemoryKeyStore())
        val secretKey = first.signUp(email, password, "Android phone")
        assertTrue(secretKey.value.startsWith("A1-"))
        assertTrue(first.isUnlocked())

        val vault = first.vaultIds().first()
        val item = first.newItem(Item.Kind.LOGIN, "Example").edited {
            username = "ada"
            this.password = "hunter2-synthetic"
            setUrls(listOf(Item.Url("https://example.com")))
        }
        val id = first.save(item, vault)
        val pushed = first.sync() as SyncResult.Synced
        assertEquals(1L, pushed.pushed)

        val keys = MemoryKeyStore()
        val second = Session(cachePath = null, server = server, keys = keys)
        assertTrue(second.logIn(email, password, secretKey, "Android tablet") is SyncResult.Synced)
        val copy = second.items(vault).first { it.id == id }
        assertEquals("Example", copy.title)
        assertEquals("hunter2-synthetic", copy.password)

        // Locked, then unlocked offline with the stored keys.
        second.lock()
        second.unlock(password)
        assertTrue(second.isUnlocked())
        second.signOut()
        assertTrue(keys.get(KeyName.SECRET_KEY) == null)
    }
}
