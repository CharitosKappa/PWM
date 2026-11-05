package pwm.app

import android.content.ComponentName
import android.content.Context
import android.view.autofill.AutofillManager
import androidx.annotation.RequiresApi
import androidx.credentials.CredentialManager

/** Whether PWM is the credential provider (D61 §8), and the system's screen to turn it on. */
@RequiresApi(34)
internal object PasskeySettings {
    /** Autofill is PWM's, but the credential provider is off. */
    fun autofillOnProviderOff(context: Context): Boolean {
        val autofill = context.getSystemService(AutofillManager::class.java)
        if (autofill == null || !autofill.hasEnabledAutofillServices()) return false
        val credentials = context.getSystemService(android.credentials.CredentialManager::class.java) ?: return false
        return !credentials.isEnabledCredentialProviderService(ComponentName(context, PwmCredentialService::class.java))
    }

    fun open(context: Context) {
        runCatching { CredentialManager.create(context).createSettingsPendingIntent().send() }
    }
}
