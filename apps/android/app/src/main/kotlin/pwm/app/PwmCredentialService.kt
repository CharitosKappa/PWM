package pwm.app

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ApplicationInfo
import android.content.pm.Signature
import android.os.CancellationSignal
import android.os.OutcomeReceiver
import androidx.annotation.RequiresApi
import androidx.credentials.exceptions.ClearCredentialException
import androidx.credentials.exceptions.CreateCredentialException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.provider.AuthenticationAction
import androidx.credentials.provider.BeginCreateCredentialRequest
import androidx.credentials.provider.BeginCreateCredentialResponse
import androidx.credentials.provider.BeginCreatePasswordCredentialRequest
import androidx.credentials.provider.BeginCreatePublicKeyCredentialRequest
import androidx.credentials.provider.BeginGetCredentialRequest
import androidx.credentials.provider.BeginGetCredentialResponse
import androidx.credentials.provider.BeginGetPasswordOption
import androidx.credentials.provider.BeginGetPublicKeyCredentialOption
import androidx.credentials.provider.CallingAppInfo
import androidx.credentials.provider.CreateEntry
import androidx.credentials.provider.CredentialEntry
import androidx.credentials.provider.CredentialProviderService
import androidx.credentials.provider.PasswordCredentialEntry
import androidx.credentials.provider.ProviderClearCredentialStateRequest
import androidx.credentials.provider.PublicKeyCredentialEntry
import java.security.MessageDigest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import pwm.kit.CredentialCaller
import pwm.kit.CredentialCallers
import pwm.kit.PasskeyCaller
import pwm.kit.ui.CredentialAccess

/**
 * PWM's Credential Manager provider (D61), Android 14+: passwords and
 * passkeys in the system's own sheet. The first answer is only entries;
 * picking one opens [CredentialActivity]. A listed browser's origin is
 * believed (D58's list, checked by Android and again by the kit); any other
 * app is itself, and gets a site's passkeys only when the site's Digital
 * Asset Links file lists it. Never PWM itself.
 */
@RequiresApi(34)
class PwmCredentialService : CredentialProviderService() {
    private val scope = MainScope()

    override fun onBeginGetCredentialRequest(
        request: BeginGetCredentialRequest,
        cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<BeginGetCredentialResponse, GetCredentialException>,
    ) {
        val info = request.callingAppInfo
        if (info == null || Credentials.isSelf(this, info)) return callback.onResult(BeginGetCredentialResponse())
        val job = scope.launch {
            val response = try {
                Credentials.entries(this@PwmCredentialService, request)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // Nothing to offer is better than a failure in someone else's app.
                BeginGetCredentialResponse()
            }
            callback.onResult(response)
        }
        cancellationSignal.setOnCancelListener { job.cancel() }
    }

    override fun onBeginCreateCredentialRequest(
        request: BeginCreateCredentialRequest,
        cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<BeginCreateCredentialResponse, CreateCredentialException>,
    ) {
        val info = request.callingAppInfo
        if (info == null || Credentials.isSelf(this, info)) return callback.onResult(BeginCreateCredentialResponse())
        val job = scope.launch {
            val response = try {
                Credentials.createEntries(this@PwmCredentialService, request, info)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                BeginCreateCredentialResponse()
            }
            callback.onResult(response)
        }
        cancellationSignal.setOnCancelListener { job.cancel() }
    }

    /** An app signing out: PWM keeps nothing per app to clear (D61 §8). */
    override fun onClearCredentialStateRequest(
        request: ProviderClearCredentialStateRequest,
        cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<Void?, ClearCredentialException>,
    ) = callback.onResult(null)

    override fun onDestroy() {
        super.onDestroy()
        scope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
    }
}

/** What the service and [CredentialActivity] share: who is asking, and the entries. */
@RequiresApi(34)
internal object Credentials {
    /** At most this many of each kind in the sheet. */
    private const val MAX_ENTRIES = 5

    fun isSelf(context: Context, info: CallingAppInfo): Boolean = info.packageName == context.packageName

    /** The caller, as D61 §1 says: an origin only from a listed browser. */
    fun callerOf(info: CallingAppInfo): CredentialCaller {
        val signing = info.signingInfo
        val current = signing.apkContentsSigners.firstOrNull()
        val history: Array<Signature> = if (signing.hasMultipleSigners()) signing.apkContentsSigners else signing.signingCertificateHistory ?: emptyArray()
        val origin = runCatching { info.getOrigin(CredentialCallers.privilegedAllowlist()) }.getOrNull()
        return CredentialCallers.resolve(info.packageName, current?.let(::digest) ?: "", history.map(::digest), origin)
    }

    private fun digest(signature: Signature): String =
        MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).joinToString("") { "%02x".format(it) }

    /** A passkey request's caller, with the browser's hash or the site's asset links files. */
    suspend fun passkeyCaller(context: Context, caller: CredentialCaller, requestJson: String, clientDataHash: ByteArray?, create: Boolean): PasskeyCaller {
        if (caller.origin != null) return PasskeyCaller(caller, clientDataHash, emptyList())
        val app = (context.applicationContext as PwmApplication)
        val links = testAssetLinks(context) ?: app.model.assetLinks.forRequest(requestJson, create)
        return PasskeyCaller(caller, null, links)
    }

    /**
     * Debug builds only: the device test's asset links file, since CI can't
     * serve one over HTTPS (D61 §10). A release build never reads it.
     */
    private fun testAssetLinks(context: Context): List<String>? {
        if (context.applicationInfo.flags and ApplicationInfo.FLAG_DEBUGGABLE == 0) return null
        val text = context.getSharedPreferences(PwmAutofillService.TEST_PREFS, Context.MODE_PRIVATE).getString(TEST_ASSET_LINKS, null)
        return text?.let { listOf(it) }
    }

    const val TEST_ASSET_LINKS = "assetLinks"

    /** The sheet's entries for a sign-in: "Unlock PWM" while locked. */
    suspend fun entries(context: Context, request: BeginGetCredentialRequest): BeginGetCredentialResponse {
        val app = (context.applicationContext as PwmApplication).model
        val info = request.callingAppInfo ?: return BeginGetCredentialResponse()
        when (app.credentialAccess()) {
            CredentialAccess.SIGNED_OUT -> return BeginGetCredentialResponse()
            CredentialAccess.LOCKED -> return BeginGetCredentialResponse.Builder()
                .setAuthenticationActions(
                    listOf(AuthenticationAction(context.getString(R.string.credential_unlock), CredentialActivity.unlockIntent(context))),
                )
                .build()
            CredentialAccess.UNLOCKED -> Unit
        }
        val caller = callerOf(info)
        val out = mutableListOf<CredentialEntry>()
        var index = 0
        for (option in request.beginGetCredentialOptions) {
            when (option) {
                is BeginGetPasswordOption -> for (match in app.passwordOffers(caller).take(MAX_ENTRIES)) {
                    val intent = CredentialActivity.passwordIntent(context, match.itemId, match.vaultId, index++)
                    out += PasswordCredentialEntry.Builder(context, match.username.ifEmpty { match.title }, intent, option)
                        .setDisplayName(match.title)
                        .build()
                }
                is BeginGetPublicKeyCredentialOption -> {
                    val passkey = passkeyCaller(context, caller, option.requestJson, option.clientDataHash, create = false)
                    for (offer in app.passkeyOffers(passkey, option.requestJson).take(MAX_ENTRIES)) {
                        val intent = CredentialActivity.passkeyIntent(context, offer, index++)
                        out += PublicKeyCredentialEntry.Builder(context, offer.userName, intent, option)
                            .setDisplayName(offer.title)
                            .build()
                    }
                }
            }
        }
        return BeginGetCredentialResponse.Builder().setCredentialEntries(out).build()
    }

    /** "Save in PWM" and "Create a passkey in PWM". */
    suspend fun createEntries(context: Context, request: BeginCreateCredentialRequest, info: CallingAppInfo): BeginCreateCredentialResponse {
        val app = (context.applicationContext as PwmApplication).model
        if (app.credentialAccess() == CredentialAccess.SIGNED_OUT) return BeginCreateCredentialResponse()
        val account = context.getString(R.string.app_name)
        val entry: PendingIntent = when (request) {
            is BeginCreatePasswordCredentialRequest -> CredentialActivity.createIntent(context, passkey = false)
            is BeginCreatePublicKeyCredentialRequest -> {
                val caller = passkeyCaller(context, callerOf(info), request.requestJson, request.clientDataHash, create = true)
                if (!app.passkeyCreatable(caller, request.requestJson)) return BeginCreateCredentialResponse()
                CredentialActivity.createIntent(context, passkey = true)
            }
            else -> return BeginCreateCredentialResponse()
        }
        return BeginCreateCredentialResponse.Builder()
            .setCreateEntries(listOf(CreateEntry.Builder(account, entry).build()))
            .build()
    }

    /** The pending intent an entry opens: mutable, not one-shot, as the provider guide says. */
    fun pending(context: Context, intent: Intent, request: Int): PendingIntent =
        PendingIntent.getActivity(context, request, intent, PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
}
