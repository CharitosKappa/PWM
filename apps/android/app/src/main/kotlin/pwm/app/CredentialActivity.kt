package pwm.app

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.annotation.RequiresApi
import androidx.credentials.CreatePasswordRequest
import androidx.credentials.CreatePasswordResponse
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CreatePublicKeyCredentialResponse
import androidx.credentials.GetCredentialResponse
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PasswordCredential
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.provider.PendingIntentHandler
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch
import pwm.kit.PasskeyOffer
import pwm.kit.Redacted
import pwm.kit.ui.CredentialResult
import pwm.kit.ui.CredentialTask

/**
 * What a Credential Manager entry opens (D61 §5-7): unlock, confirm a
 * passkey, or show what "Save in PWM" will save; then the result goes back
 * to the system and the screen closes. The request itself is read from the
 * system's intent, never from our extras, which only say which login.
 */
@RequiresApi(34)
class CredentialActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Vault content: never in screenshots or the app switcher (D56).
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        lifecycleScope.launch {
            val task = task() ?: return@launch cancel()
            val model = (application as PwmApplication).model.credentialModel(task) { result -> finishWith(task, result) }
                ?: return@launch cancel()
            setContent { PwmTheme { CredentialScreen(model) } }
        }
    }

    /** What the entry asked for, from the system's request. */
    private suspend fun task(): CredentialTask? = when (intent.getStringExtra(KIND)) {
        UNLOCK -> CredentialTask.Unlock
        PASSWORD -> CredentialTask.GetPassword(intent.getStringExtra(ITEM) ?: return null, intent.getStringExtra(VAULT) ?: return null)
        PASSKEY -> {
            val request = PendingIntentHandler.retrieveProviderGetCredentialRequest(intent) ?: return null
            val option = request.credentialOptions.filterIsInstance<GetPublicKeyCredentialOption>().firstOrNull() ?: return null
            val caller = Credentials.callerOf(request.callingAppInfo)
            val passkey = Credentials.passkeyCaller(this, caller, option.requestJson, option.clientDataHash, create = false)
            val offer = PasskeyOffer(
                intent.getStringExtra(VAULT) ?: return null, intent.getStringExtra(ITEM) ?: return null,
                intent.getStringExtra(CREDENTIAL) ?: return null, "", "",
            )
            CredentialTask.GetPasskey(passkey, option.requestJson, offer)
        }
        CREATE -> {
            val request = PendingIntentHandler.retrieveProviderCreateCredentialRequest(intent) ?: return null
            if (Credentials.isSelf(this, request.callingAppInfo)) return null
            val caller = Credentials.callerOf(request.callingAppInfo)
            when (val asked = request.callingRequest) {
                is CreatePasswordRequest -> {
                    val name = if (caller.origin != null) null else appName(caller.packageName)
                    CredentialTask.CreatePassword(caller, name, asked.id, Redacted(asked.password))
                }
                is CreatePublicKeyCredentialRequest -> CredentialTask.CreatePasskey(
                    Credentials.passkeyCaller(this, caller, asked.requestJson, asked.clientDataHash, create = true),
                    asked.requestJson,
                )
                else -> null
            }
        }
        else -> null
    }

    private fun appName(packageName: String): String? = runCatching {
        packageManager.getApplicationLabel(packageManager.getApplicationInfo(packageName, 0)).toString()
    }.getOrNull()

    private fun finishWith(task: CredentialTask, result: CredentialResult?) {
        lifecycleScope.launch {
            val reply = Intent()
            when (result) {
                null -> return@launch cancel(task)
                CredentialResult.Unlocked -> {
                    // The sheet asks again with what is now open.
                    val begin = PendingIntentHandler.retrieveBeginGetCredentialRequest(intent) ?: return@launch cancel(task)
                    PendingIntentHandler.setBeginGetCredentialResponse(reply, Credentials.entries(this@CredentialActivity, begin))
                }
                is CredentialResult.Password ->
                    PendingIntentHandler.setGetCredentialResponse(reply, GetCredentialResponse(PasswordCredential(result.username, result.password.value)))
                is CredentialResult.Passkey -> when (task) {
                    is CredentialTask.CreatePasskey ->
                        PendingIntentHandler.setCreateCredentialResponse(reply, CreatePublicKeyCredentialResponse(result.responseJson))
                    else -> PendingIntentHandler.setGetCredentialResponse(reply, GetCredentialResponse(PublicKeyCredential(result.responseJson)))
                }
                CredentialResult.Saved -> PendingIntentHandler.setCreateCredentialResponse(reply, CreatePasswordResponse())
            }
            setResult(RESULT_OK, reply)
            finish()
        }
    }

    private fun cancel(task: CredentialTask? = null) {
        val reply = Intent()
        when (task) {
            is CredentialTask.CreatePassword, is CredentialTask.CreatePasskey ->
                PendingIntentHandler.setCreateCredentialException(reply, CreateCredentialCancellationException())
            is CredentialTask.GetPassword, is CredentialTask.GetPasskey ->
                PendingIntentHandler.setGetCredentialException(reply, GetCredentialCancellationException())
            else -> {
                setResult(RESULT_CANCELED)
                return finish()
            }
        }
        setResult(RESULT_OK, reply)
        finish()
    }

    companion object {
        private const val KIND = "pwm.credential"
        private const val UNLOCK = "unlock"
        private const val PASSWORD = "password"
        private const val PASSKEY = "passkey"
        private const val CREATE = "create"
        private const val ITEM = "pwm.item"
        private const val VAULT = "pwm.vault"
        private const val CREDENTIAL = "pwm.credentialId"

        /** Request codes, apart from the autofill service's. */
        private const val REQUESTS = 0x5250

        private fun intent(context: Context, kind: String) =
            Intent(context, CredentialActivity::class.java).putExtra(KIND, kind)

        fun unlockIntent(context: Context): PendingIntent = Credentials.pending(context, intent(context, UNLOCK), REQUESTS)

        fun passwordIntent(context: Context, itemId: String, vaultId: String, index: Int): PendingIntent =
            Credentials.pending(context, intent(context, PASSWORD).putExtra(ITEM, itemId).putExtra(VAULT, vaultId), REQUESTS + 1 + index)

        fun passkeyIntent(context: Context, offer: PasskeyOffer, index: Int): PendingIntent = Credentials.pending(
            context,
            intent(context, PASSKEY).putExtra(ITEM, offer.itemId).putExtra(VAULT, offer.vaultId).putExtra(CREDENTIAL, offer.credentialId),
            REQUESTS + 1 + index,
        )

        fun createIntent(context: Context, passkey: Boolean): PendingIntent =
            Credentials.pending(context, intent(context, CREATE).putExtra("pwm.passkey", passkey), REQUESTS + 0x40)
    }
}
