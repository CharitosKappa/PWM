package pwm.app

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.IntentSender
import android.os.Build
import android.os.Bundle
import android.service.autofill.Dataset
import android.view.WindowManager
import android.view.autofill.AutofillId
import android.view.autofill.AutofillManager
import android.view.autofill.AutofillValue
import android.widget.RemoteViews
import androidx.activity.compose.setContent
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.launch
import pwm.kit.AutofillMatch
import pwm.kit.Caller
import pwm.kit.FieldRoles
import pwm.kit.ui.FillValues
import pwm.kit.ui.SecretKeyModel

/**
 * PWM's fill screen, opened by a tap on an autofill entry (D58 §6): unlock
 * if needed, then the matches and search; the login picked is returned to
 * the system as the dataset to fill, and the screen closes. In "code" mode it
 * shows nothing: it fills a suggestion and copies its one-time code.
 */
class FillActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // Vault content: never in screenshots or the app switcher (D56).
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        val caller = callerOf(intent) ?: return finish()
        val roles = FieldRoles<AutofillId>(id(USERNAME), id(PASSWORD), null, id(CODE))
        val app = (application as PwmApplication).model
        val match = intent.getStringExtra(ITEM)?.let { item ->
            AutofillMatch(intent.getStringExtra(VAULT) ?: "", item, "", "", "fill")
        }
        if (match != null) {
            lifecycleScope.launch { finishWith(app.fillValues(match), roles) }
            return
        }
        val model = app.fillModel(caller) { values -> finishWith(values, roles) } ?: return finish()
        setContent { PwmTheme { FillScreen(model) } }
    }

    private fun finishWith(values: FillValues?, roles: FieldRoles<AutofillId>) {
        if (values == null) {
            setResult(RESULT_CANCELED)
            return finish()
        }
        // The one-time code: filled when there is a code field, otherwise copied (D58 §7).
        val code = values.code
        if (code != null && roles.oneTimeCode == null) {
            (application as PwmApplication).clipboard.copy(code, SecretKeyModel.CLIPBOARD_MS)
        }
        val blank = RemoteViews(packageName, R.layout.autofill_item)
        @Suppress("DEPRECATION")
        val dataset = Dataset.Builder(blank).apply {
            roles.username?.let { setValue(it, AutofillValue.forText(values.username)) }
            roles.password?.let { setValue(it, AutofillValue.forText(values.password.value)) }
            val codeField = roles.oneTimeCode
            if (codeField != null && code != null) setValue(codeField, AutofillValue.forText(code.value))
        }.build()
        setResult(RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, dataset))
        finish()
    }

    private fun id(key: String): AutofillId? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) intent.getParcelableExtra(key, AutofillId::class.java)
        else @Suppress("DEPRECATION") intent.getParcelableExtra(key)

    companion object {
        private const val USERNAME = "pwm.username"
        private const val PASSWORD = "pwm.password"
        private const val CODE = "pwm.code"
        private const val PACKAGE = "pwm.package"
        private const val CERTS = "pwm.certs"
        private const val URL = "pwm.url"
        private const val ITEM = "pwm.item"
        private const val VAULT = "pwm.vault"

        private fun intent(context: Context, caller: Caller, roles: FieldRoles<AutofillId>): Intent =
            Intent(context, FillActivity::class.java).apply {
                putExtra(USERNAME, roles.username)
                putExtra(PASSWORD, roles.password)
                putExtra(CODE, roles.oneTimeCode)
                when (caller) {
                    is Caller.Web -> putExtra(URL, caller.url)
                    is Caller.App -> {
                        putExtra(PACKAGE, caller.packageName)
                        putExtra(CERTS, caller.certs.toTypedArray())
                    }
                }
            }

        private fun sender(context: Context, intent: Intent, request: Int): IntentSender =
            PendingIntent.getActivity(
                context, request, intent, PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_CANCEL_CURRENT,
            ).intentSender

        /** The fill screen: unlock, the matches and search. */
        fun fillSender(context: Context, caller: Caller, roles: FieldRoles<AutofillId>, request: Int): IntentSender =
            sender(context, intent(context, caller, roles), REQUESTS + request)

        /** A suggestion with a one-time code: filled at once, its code copied. */
        fun codeSender(context: Context, caller: Caller, roles: FieldRoles<AutofillId>, match: AutofillMatch, request: Int): IntentSender =
            sender(context, intent(context, caller, roles).putExtra(ITEM, match.itemId).putExtra(VAULT, match.vaultId), REQUESTS + request)

        private fun callerOf(intent: Intent): Caller? {
            intent.getStringExtra(URL)?.let { return Caller.Web(it) }
            val packageName = intent.getStringExtra(PACKAGE) ?: return null
            return Caller.App(packageName, intent.getStringArrayExtra(CERTS)?.toList().orEmpty())
        }

        private const val REQUESTS = 0x5157
    }
}
