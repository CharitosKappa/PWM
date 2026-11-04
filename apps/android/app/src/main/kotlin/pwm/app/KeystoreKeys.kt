package pwm.app

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore as JavaKeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec
import pwm.kit.KeyName
import pwm.kit.KeyStore

/**
 * The device's keys (Secret Key, tokens, device id, email) on disk (D52 §5):
 * each value sealed with AES-256-GCM under a key that lives in the Android
 * Keystore, made for this app and never exportable, in a private file per
 * name. The platform's cipher only protects these values at rest, as the
 * Keychain does on Apple; the vault's crypto stays in the core.
 */
class KeystoreKeys(context: Context) : KeyStore {
    private val dir = File(context.noBackupFilesDir, "keys").apply { mkdirs() }

    private fun key(): SecretKey {
        val store = JavaKeyStore.getInstance(PROVIDER).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, PROVIDER)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }

    private fun file(name: KeyName) = File(dir, name.key)

    @Synchronized
    override fun get(name: KeyName): String? {
        val sealed = file(name).takeIf { it.exists() }?.readBytes() ?: return null
        if (sealed.size <= IV_BYTES) return null
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(TAG_BITS, sealed, 0, IV_BYTES))
        // The name is bound in, so a value moved to another name won't open.
        cipher.updateAAD(name.key.toByteArray())
        return String(cipher.doFinal(sealed, IV_BYTES, sealed.size - IV_BYTES), Charsets.UTF_8)
    }

    @Synchronized
    override fun set(name: KeyName, value: String) {
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, key())
        cipher.updateAAD(name.key.toByteArray())
        val sealed = cipher.iv + cipher.doFinal(value.toByteArray(Charsets.UTF_8))
        val temp = File(dir, "${name.key}.tmp")
        temp.writeBytes(sealed)
        if (!temp.renameTo(file(name))) {
            temp.delete()
            throw IllegalStateException("couldn't store a key")
        }
    }

    @Synchronized
    override fun delete(name: KeyName) {
        file(name).delete()
    }

    private companion object {
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = "pwm.device-keys"
        const val TRANSFORMATION = "AES/GCM/NoPadding"
        const val IV_BYTES = 12
        const val TAG_BITS = 128
    }
}
