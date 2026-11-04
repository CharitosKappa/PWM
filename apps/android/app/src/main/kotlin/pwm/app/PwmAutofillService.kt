package pwm.app

import android.app.PendingIntent
import android.app.assist.AssistStructure
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.CancellationSignal
import android.service.autofill.AutofillService
import android.service.autofill.Dataset
import android.service.autofill.FillCallback
import android.service.autofill.FillRequest
import android.service.autofill.FillResponse
import android.service.autofill.InlinePresentation
import android.service.autofill.SaveCallback
import android.service.autofill.SaveRequest
import android.view.View
import android.view.autofill.AutofillId
import android.view.autofill.AutofillValue
import android.widget.RemoteViews
import android.widget.inline.InlinePresentationSpec
import androidx.autofill.inline.v1.InlineSuggestionUi
import java.security.MessageDigest
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.MainScope
import kotlinx.coroutines.launch
import pwm.kit.AutofillMatch
import pwm.kit.Caller
import pwm.kit.Callers
import pwm.kit.FieldMapping
import pwm.kit.FieldRoles
import pwm.kit.ScreenField
import pwm.kit.ui.FillValues

/**
 * PWM's autofill service (D58): passwords in apps and browsers, after a tap.
 * It reads the fields' descriptions, never their values, believes a web
 * domain only from a listed browser, and offers the core's matches while
 * unlocked, or "Unlock PWM to fill" while locked. Saving new logins, cards
 * and identities are not offered yet (D58 §7).
 */
class PwmAutofillService : AutofillService() {
    private val scope = MainScope()

    override fun onFillRequest(request: FillRequest, cancellationSignal: CancellationSignal, callback: FillCallback) {
        val structure = request.fillContexts.lastOrNull()?.structure ?: return callback.onSuccess(null)
        val caller = structure.activityComponent.packageName
        // Never into PWM's own screens.
        if (caller == packageName) return callback.onSuccess(null)
        val screen = Screen.read(structure)
        if (screen.fields.isEmpty()) return callback.onSuccess(null)
        val who = Callers.resolve(caller, certificates(this, caller), screen.webDomain, screen.webScheme)
        val inline = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && !dropdownOnly(this)) {
            request.inlineSuggestionsRequest?.inlinePresentationSpecs
        } else {
            null
        }
        val job = scope.launch {
            try {
                callback.onSuccess(respond(who, screen, inline))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // Nothing to offer is better than a crash in someone else's app.
                callback.onSuccess(null)
            }
        }
        cancellationSignal.setOnCancelListener { job.cancel() }
    }

    private suspend fun respond(caller: Caller, screen: Screen, inline: List<InlinePresentationSpec>?): FillResponse? {
        val app = (application as PwmApplication).model
        val plan = app.autofillPlan(caller, FieldMapping.describe(screen.fields)) ?: return null
        val roles = FieldMapping.roles(screen.fields, plan.classes)
        if (roles.username == null && roles.password == null) return null
        val response = FillResponse.Builder()
        var index = 0
        fun inlineSpec(): InlinePresentationSpec? = inline?.takeIf { it.isNotEmpty() }?.let { it[minOf(index, it.size - 1)] }
        for (match in plan.suggested.take(MAX_SUGGESTIONS)) {
            val values = app.fillValues(match) ?: continue
            val presentation = Presentations.dropdown(this, match.title, match.username)
            val dataset = if (values.code != null && roles.oneTimeCode == null) {
                // Copies the code after the fill (D58 §7): through PWM, which fills at once.
                emptyDataset(roles, presentation, inlineSpec(), match.title, match.username)
                    .setAuthentication(FillActivity.codeSender(this, caller, roles, match, index))
            } else {
                filled(values, roles, presentation, inlineSpec(), match.title, match.username)
            }
            response.addDataset(dataset.build())
            index += 1
        }
        val (title, subtitle) = if (plan.unlocked) getString(R.string.autofill_search) to null else getString(R.string.autofill_unlock) to null
        response.addDataset(
            emptyDataset(roles, Presentations.dropdown(this, title, subtitle), inlineSpec(), title, subtitle)
                .setAuthentication(FillActivity.fillSender(this, caller, roles, index))
                .build(),
        )
        return response.build()
    }

    @Suppress("DEPRECATION")
    private fun emptyDataset(roles: FieldRoles<AutofillId>, presentation: RemoteViews, spec: InlinePresentationSpec?, title: String, subtitle: String?): Dataset.Builder {
        val builder = Dataset.Builder(presentation)
        val inline = spec?.let { Presentations.inline(this, it, title, subtitle) }
        for (id in listOfNotNull(roles.username, roles.password, roles.oneTimeCode)) {
            if (inline != null) builder.setValue(id, null, presentation, inline) else builder.setValue(id, null, presentation)
        }
        return builder
    }

    @Suppress("DEPRECATION")
    private fun filled(values: FillValues, roles: FieldRoles<AutofillId>, presentation: RemoteViews, spec: InlinePresentationSpec?, title: String, subtitle: String?): Dataset.Builder {
        val builder = Dataset.Builder(presentation)
        val inline = spec?.let { Presentations.inline(this, it, title, subtitle) }
        fun set(id: AutofillId?, value: String?) {
            if (id == null || value == null) return
            val v = AutofillValue.forText(value)
            if (inline != null) builder.setValue(id, v, presentation, inline) else builder.setValue(id, v, presentation)
        }
        set(roles.username, values.username)
        set(roles.password, values.password.value)
        set(roles.oneTimeCode, values.code?.value)
        return builder
    }

    /** No saving yet (D58 §7): the fields' values are never kept. */
    override fun onSaveRequest(request: SaveRequest, callback: SaveCallback) = callback.onSuccess()

    override fun onDestroy() {
        super.onDestroy()
        scope.coroutineContext[kotlinx.coroutines.Job]?.cancel()
    }

    /** The fields on screen, and the page address a browser reported. */
    class Screen(val fields: List<ScreenField<AutofillId>>, val webDomain: String?, val webScheme: String?) {
        companion object {
            fun read(structure: AssistStructure): Screen {
                val fields = mutableListOf<ScreenField<AutofillId>>()
                var domain: String? = null
                var scheme: String? = null
                fun walk(node: AssistStructure.ViewNode) {
                    if (domain == null && !node.webDomain.isNullOrEmpty()) {
                        domain = node.webDomain
                        scheme = node.webScheme
                    }
                    val id = node.autofillId
                    if (id != null && node.autofillType == View.AUTOFILL_TYPE_TEXT && node.visibility == View.VISIBLE) {
                        val html = node.htmlInfo
                        fields += ScreenField(
                            id = id,
                            autofillHints = node.autofillHints?.toList().orEmpty(),
                            inputType = node.inputType,
                            resourceId = node.idEntry,
                            hint = node.hint?.toString(),
                            htmlTag = html?.tag,
                            htmlAttributes = html?.attributes?.associate { it.first to it.second }.orEmpty(),
                        )
                    }
                    for (i in 0 until node.childCount) walk(node.getChildAt(i))
                }
                for (i in 0 until structure.windowNodeCount) walk(structure.getWindowNodeAt(i).rootViewNode)
                return Screen(fields, domain, scheme)
            }
        }
    }

    companion object {
        /**
         * Debug builds only: the device test asks for the dropdown, which it
         * can read, instead of the keyboard's chips. A release build never reads this.
         */
        fun dropdownOnly(context: Context): Boolean =
            (context.applicationInfo.flags and android.content.pm.ApplicationInfo.FLAG_DEBUGGABLE) != 0 &&
                context.getSharedPreferences(TEST_PREFS, Context.MODE_PRIVATE).getBoolean(DROPDOWN_ONLY, false)

        const val TEST_PREFS = "pwm.test"
        const val DROPDOWN_ONLY = "dropdownOnly"

        /** At most this many logins in the list; the rest are in search. */
        const val MAX_SUGGESTIONS = 5

        /** The SHA-256 of each certificate the package is signed with now. */
        fun certificates(context: Context, packageName: String): List<String> = try {
            val info = context.packageManager.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES).signingInfo
            val signers = when {
                info == null -> emptyArray()
                info.hasMultipleSigners() -> info.apkContentsSigners
                else -> info.signingCertificateHistory
            }
            signers.map { sig -> MessageDigest.getInstance("SHA-256").digest(sig.toByteArray()).joinToString("") { "%02x".format(it) } }
        } catch (e: PackageManager.NameNotFoundException) {
            emptyList()
        }
    }
}

/** How an entry looks in the dropdown and in the keyboard (D58 §8): plain text, nothing a page can style. */
object Presentations {
    fun dropdown(context: Context, title: String, subtitle: String?): RemoteViews =
        RemoteViews(context.packageName, R.layout.autofill_item).apply {
            setTextViewText(R.id.autofill_title, title)
            setTextViewText(R.id.autofill_subtitle, subtitle ?: "")
            setViewVisibility(R.id.autofill_subtitle, if (subtitle.isNullOrEmpty()) View.GONE else View.VISIBLE)
        }

    fun inline(context: Context, spec: InlinePresentationSpec, title: String, subtitle: String?): InlinePresentation? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        // The keyboard's chip opens nothing by itself; this only attributes it to PWM.
        val attribution = PendingIntent.getActivity(
            context, 0, Intent(context, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val content = InlineSuggestionUi.newContentBuilder(attribution).setTitle(title).apply {
            if (!subtitle.isNullOrEmpty()) setSubtitle(subtitle)
        }.build()
        return InlinePresentation(content.slice, spec, false)
    }
}
