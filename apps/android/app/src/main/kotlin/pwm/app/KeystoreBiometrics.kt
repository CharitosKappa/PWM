package pwm.app

import android.app.KeyguardManager
import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricManager.Authenticators.BIOMETRIC_STRONG
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.spec.MGF1ParameterSpec
import java.security.spec.X509EncodedKeySpec
import javax.crypto.Cipher
import javax.crypto.spec.OAEPParameterSpec
import javax.crypto.spec.PSource
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException
import kotlinx.coroutines.suspendCancellableCoroutine
import pwm.kit.BiometricAvailability
import pwm.kit.BiometricKeys
import pwm.kit.BiometricPurpose
import pwm.kit.PwmError
import pwm.kit.Redacted

/**
 * Fingerprint and face unlock's key (D55 §1-2): an RSA-3072 key in the
 * Android Keystore, never exportable. The unlock key is encrypted to its
 * public half with OAEP, with no prompt; only the private half needs a fresh
 * strong biometric, through `BiometricPrompt` with the cipher as its crypto
 * object. A newly enrolled finger or face makes the key unusable. The core
 * does all the vault crypto; this is the platform-key wrap `SECURITY.md` names.
 */
class KeystoreBiometrics(
    private val context: Context,
    /** The Activity showing now, for the prompt; null in the background. */
    private val activity: () -> FragmentActivity?,
) : BiometricKeys {
    override fun availability(): BiometricAvailability {
        val keyguard = context.getSystemService(KeyguardManager::class.java)
        return when (BiometricManager.from(context).canAuthenticate(BIOMETRIC_STRONG)) {
            BiometricManager.BIOMETRIC_SUCCESS ->
                if (keyguard?.isDeviceSecure == true) BiometricAvailability.Available
                else BiometricAvailability.Unavailable("biometric_no_screen_lock")
            BiometricManager.BIOMETRIC_ERROR_NONE_ENROLLED -> BiometricAvailability.Unavailable("biometric_not_enrolled")
            else -> BiometricAvailability.Unavailable("biometric_unavailable")
        }
    }

    override fun wrap(key: Redacted<ByteArray>): ByteArray {
        val store = keyStore()
        if (!store.containsAlias(ALIAS)) generate()
        // The public half, in software: Keystore public keys need no prompt.
        val public = KeyFactory.getInstance("RSA").generatePublic(X509EncodedKeySpec(store.getCertificate(ALIAS).publicKey.encoded))
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, public, OAEP)
        return cipher.doFinal(key.value)
    }

    override suspend fun unwrap(wrapped: ByteArray, purpose: BiometricPurpose): Redacted<ByteArray> {
        val host = activity() ?: throw PwmError("biometric_cancelled")
        val private = keyStore().getKey(ALIAS, null) as? PrivateKey ?: throw PwmError("biometric_changed")
        val cipher = Cipher.getInstance(TRANSFORMATION)
        try {
            cipher.init(Cipher.DECRYPT_MODE, private, OAEP)
        } catch (e: KeyPermanentlyInvalidatedException) {
            // A new finger or face since it was turned on (D45 §4).
            throw PwmError("biometric_changed")
        }
        return suspendCancellableCoroutine { done ->
            val prompt = BiometricPrompt(
                host,
                ContextCompat.getMainExecutor(host),
                object : BiometricPrompt.AuthenticationCallback() {
                    override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                        val opened = runCatching { result.cryptoObject?.cipher?.doFinal(wrapped) }.getOrNull()
                        if (opened == null) done.resumeWithException(PwmError("biometric_unlock_failed"))
                        else done.resume(Redacted(opened))
                    }

                    // The master password button, a cancel, or too many tries: the password field shows.
                    override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                        if (done.isActive) done.resumeWithException(PwmError("biometric_cancelled"))
                    }
                },
            )
            val info = BiometricPrompt.PromptInfo.Builder()
                .setTitle(host.getString(if (purpose == BiometricPurpose.UNLOCK) R.string.biometric_unlock_title else R.string.biometric_turn_on_title))
                .setNegativeButtonText(host.getString(R.string.use_master_password))
                .setAllowedAuthenticators(BIOMETRIC_STRONG)
                .setConfirmationRequired(false)
                .build()
            prompt.authenticate(info, BiometricPrompt.CryptoObject(cipher))
            done.invokeOnCancellation { prompt.cancelAuthentication() }
        }
    }

    override fun delete() {
        val store = keyStore()
        if (store.containsAlias(ALIAS)) store.deleteEntry(ALIAS)
    }

    private fun generate() {
        val spec = KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_DECRYPT)
            .setKeySize(3072)
            .setDigests(KeyProperties.DIGEST_SHA256)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_RSA_OAEP)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
            .apply {
                // Each use needs a fresh strong biometric.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
                } else {
                    @Suppress("DEPRECATION")
                    setUserAuthenticationValidityDurationSeconds(-1)
                }
            }
            .build()
        KeyPairGenerator.getInstance(KeyProperties.KEY_ALGORITHM_RSA, PROVIDER).apply { initialize(spec) }.generateKeyPair()
    }

    private fun keyStore(): KeyStore = KeyStore.getInstance(PROVIDER).apply { load(null) }

    private companion object {
        const val PROVIDER = "AndroidKeyStore"
        const val ALIAS = "pwm.biometric-unlock"
        const val TRANSFORMATION = "RSA/ECB/OAEPWithSHA-256AndMGF1Padding"

        /** SHA-256, with MGF1 over SHA-1: the MGF1 digest the Keystore takes on every Android version. */
        val OAEP = OAEPParameterSpec("SHA-256", "MGF1", MGF1ParameterSpec.SHA1, PSource.PSpecified.DEFAULT)
    }
}
